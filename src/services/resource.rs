// 资源管理业务逻辑 — 提取触发、列表查询、编辑更新、推送

use crate::errors::AppError;
use crate::models::extracted_resource::{ExtractedResource, NewExtractedResource, UpdateExtractedResource};
use crate::services::ai_extractor;
use crate::services::extractor;
use crate::state::{DbPool, OptionCache};
use serde_json::json;

/// 提取结果
#[derive(Debug, serde::Serialize)]
pub struct ExtractionResult {
    pub total_scanned: i64,
    pub extracted: i64,
    pub skipped: i64,
    pub errors: i64,
}

/// 资源列表结果
#[derive(Debug, serde::Serialize)]
pub struct ResourceListResult {
    pub list: Vec<ExtractedResource>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, serde::Serialize)]
pub struct PaginationInfo {
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

/// 触发资源提取 — 扫描未提取的采集历史，调用 extractor 提取
pub async fn trigger_extraction(
    db: &DbPool,
    option_cache: &OptionCache,
    batch_size: i64,
) -> Result<ExtractionResult, AppError> {
    // 1. 查找没有对应 extracted_resources 记录的 collector_histories
    let histories: Vec<(i64, Option<String>, Option<String>)> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT ch.id, ch.raw_data, ch.remote_id \
                 FROM collector_histories ch \
                 LEFT JOIN extracted_resources er ON er.collector_history_id = ch.id \
                 WHERE er.id IS NULL AND ch.raw_data IS NOT NULL \
                 LIMIT ?"
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT ch.id, ch.raw_data, ch.remote_id \
                 FROM collector_histories ch \
                 LEFT JOIN extracted_resources er ON er.collector_history_id = ch.id \
                 WHERE er.id IS NULL AND ch.raw_data IS NOT NULL \
                 LIMIT $1"
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
    };

    let total_scanned = histories.len() as i64;
    let mut extracted = 0i64;
    let mut skipped = 0i64;
    let mut errors = 0i64;

    // 读取提取模式
    let extract_mode = {
        let cache = option_cache.read().await;
        cache.get("push_extract_mode").cloned().unwrap_or_else(|| "rule".to_string())
    };

    // 读取图床域名
    let image_domain = {
        let cache = option_cache.read().await;
        cache.get("TelegramImageDomain")
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_default()
    };

    for (history_id, raw_data, remote_id) in &histories {
        let raw_text = match raw_data {
            Some(r) => {
                // 尝试从 JSON 中提取 text 字段
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(r) {
                    msg.get("text").and_then(|t| t.as_str()).unwrap_or(r).to_string()
                } else {
                    r.clone()
                }
            }
            None => continue,
        };

        // 规则提取
        let drafts = extractor::extract_resources(&raw_text);
        if drafts.is_empty() {
            skipped += 1;
            continue;
        }

        for draft in drafts {
            // 图片 URL
            let img = if let Some(rid) = remote_id {
                if !rid.is_empty() && !image_domain.is_empty() {
                    format!("{}/{}", image_domain, rid)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            // AI 增强模式
            let (final_draft, mode) = if extract_mode == "ai" {
                let enhanced = ai_extractor::ai_extract(&raw_text, &draft, option_cache).await;
                (enhanced, "ai".to_string())
            } else {
                (draft, "rule".to_string())
            };

            let new_resource = NewExtractedResource {
                collector_history_id: *history_id,
                title: final_draft.title,
                url: if final_draft.url.is_empty() { None } else { Some(final_draft.url.join(",")) },
                description: if final_draft.description.is_empty() { None } else { Some(final_draft.description) },
                category: if final_draft.category.is_empty() { None } else { Some(final_draft.category) },
                tags: if final_draft.tags.is_empty() { None } else { Some(final_draft.tags) },
                img: if img.is_empty() { None } else { Some(img) },
                source: "tg".to_string(),
                extra: None,
                extract_mode: mode,
            };

            match insert_resource(db, &new_resource).await {
                Ok(_) => extracted += 1,
                Err(e) => {
                    tracing::warn!("插入资源失败 (history_id={}): {e}", history_id);
                    errors += 1;
                }
            }
        }
    }

    tracing::info!(
        "资源提取完成: scanned={}, extracted={}, skipped={}, errors={}",
        total_scanned, extracted, skipped, errors
    );

    Ok(ExtractionResult {
        total_scanned,
        extracted,
        skipped,
        errors,
    })
}

/// 插入新资源记录
async fn insert_resource(db: &DbPool, r: &NewExtractedResource) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO extracted_resources \
                 (collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(r.collector_history_id)
            .bind(&r.title)
            .bind(&r.url)
            .bind(&r.description)
            .bind(&r.category)
            .bind(&r.tags)
            .bind(&r.img)
            .bind(&r.source)
            .bind(&r.extra)
            .bind(&r.extract_mode)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO extracted_resources \
                 (collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
            )
            .bind(r.collector_history_id)
            .bind(&r.title)
            .bind(&r.url)
            .bind(&r.description)
            .bind(&r.category)
            .bind(&r.tags)
            .bind(&r.img)
            .bind(&r.source)
            .bind(&r.extra)
            .bind(&r.extract_mode)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 资源列表（分页 + 状态筛选）
pub async fn list_resources(
    db: &DbPool,
    page: i64,
    page_size: i64,
    status: Option<&str>,
    category: Option<&str>,
) -> Result<ResourceListResult, AppError> {
    let offset = (page - 1).max(0) * page_size;

    let where_clause = build_where_clause(status, category);
    let count_sql = format!("SELECT COUNT(*) FROM extracted_resources WHERE {}", where_clause);
    let query_sql = format!(
        "SELECT id, collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode, is_pushed, is_edited, created_at, updated_at \
         FROM extracted_resources WHERE {} ORDER BY created_at DESC LIMIT ? OFFSET ?",
        where_clause
    );
    let query_sql_pg = format!(
        "SELECT id, collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode, is_pushed, is_edited, created_at, updated_at \
         FROM extracted_resources WHERE {} ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        where_clause
    );

    let (list, total): (Vec<ExtractedResource>, i64) = match db {
        DbPool::Sqlite(pool) => {
            let total: i64 = sqlx::query_scalar(&count_sql)
                .fetch_one(pool)
                .await?;
            let list = sqlx::query_as(&query_sql)
                .bind(page_size)
                .bind(offset)
                .fetch_all(pool)
                .await?;
            (list, total)
        }
        DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar(&count_sql)
                .fetch_one(pool)
                .await?;
            let list = sqlx::query_as(&query_sql_pg)
                .bind(page_size)
                .bind(offset)
                .fetch_all(pool)
                .await?;
            (list, total)
        }
    };

    Ok(ResourceListResult {
        list,
        pagination: PaginationInfo { page, page_size, total },
    })
}

/// 构建 WHERE 子句
fn build_where_clause(status: Option<&str>, category: Option<&str>) -> String {
    let mut conditions = vec!["1=1".to_string()];

    if let Some(s) = status {
        match s {
            "unpushed" => conditions.push("is_pushed = 0".to_string()),
            "pushed" => conditions.push("is_pushed = 1".to_string()),
            _ => {} // "all" or anything else → no filter
        }
    }

    if let Some(c) = category
        && !c.is_empty()
    {
        conditions.push(format!("category = '{}'", c.replace('\'', "''")));
    }

    conditions.join(" AND ")
}

/// 获取单条资源
pub async fn get_resource(db: &DbPool, id: i64) -> Result<ExtractedResource, AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT id, collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode, is_pushed, is_edited, created_at, updated_at \
                 FROM extracted_resources WHERE id = ?"
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT id, collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode, is_pushed, is_edited, created_at, updated_at \
                 FROM extracted_resources WHERE id = $1"
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
    }
    .ok_or_else(|| AppError::NotFound("资源不存在".to_string()))
}

/// 更新资源 — 更新后标记 is_edited = true
pub async fn update_resource(
    db: &DbPool,
    id: i64,
    updates: &UpdateExtractedResource,
) -> Result<(), AppError> {
    // 动态构建 SET 子句
    let mut set_parts = Vec::new();
    let mut title_val: Option<String> = None;
    let mut desc_val: Option<String> = None;
    let mut tags_val: Option<String> = None;
    let mut cat_val: Option<String> = None;
    let mut url_val: Option<String> = None;

    if let Some(ref v) = updates.title {
        set_parts.push("title = ?");
        title_val = Some(v.clone());
    }
    if let Some(ref v) = updates.description {
        set_parts.push("description = ?");
        desc_val = Some(v.clone());
    }
    if let Some(ref v) = updates.tags {
        set_parts.push("tags = ?");
        tags_val = Some(v.clone());
    }
    if let Some(ref v) = updates.category {
        set_parts.push("category = ?");
        cat_val = Some(v.clone());
    }
    if let Some(ref v) = updates.url {
        set_parts.push("url = ?");
        url_val = Some(v.clone());
    }

    if set_parts.is_empty() {
        return Ok(());
    }

    set_parts.push("is_edited = 1");
    set_parts.push("updated_at = CURRENT_TIMESTAMP");

    let sql = format!("UPDATE extracted_resources SET {} WHERE id = ?", set_parts.join(", "));

    match db {
        DbPool::Sqlite(pool) => {
            let mut query = sqlx::query(&sql);
            if let Some(v) = &title_val { query = query.bind(v); }
            if let Some(v) = &desc_val { query = query.bind(v); }
            if let Some(v) = &tags_val { query = query.bind(v); }
            if let Some(v) = &cat_val { query = query.bind(v); }
            if let Some(v) = &url_val { query = query.bind(v); }
            query = query.bind(id);
            let result = query.execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("资源不存在".to_string()));
            }
        }
        DbPool::Postgres(pool) => {
            // 重新构建 PostgreSQL SQL（使用 $N 占位符）
            let mut pg_parts = Vec::new();
            let mut param_idx = 1;
            if title_val.is_some() { pg_parts.push(format!("title = ${}", param_idx)); param_idx += 1; }
            if desc_val.is_some() { pg_parts.push(format!("description = ${}", param_idx)); param_idx += 1; }
            if tags_val.is_some() { pg_parts.push(format!("tags = ${}", param_idx)); param_idx += 1; }
            if cat_val.is_some() { pg_parts.push(format!("category = ${}", param_idx)); param_idx += 1; }
            if url_val.is_some() { pg_parts.push(format!("url = ${}", param_idx)); param_idx += 1; }
            pg_parts.push("is_edited = TRUE".to_string());
            pg_parts.push("updated_at = NOW()".to_string());

            let pg_sql = format!("UPDATE extracted_resources SET {} WHERE id = ${}", pg_parts.join(", "), param_idx);

            let mut query = sqlx::query(&pg_sql);
            if let Some(v) = &title_val { query = query.bind(v); }
            if let Some(v) = &desc_val { query = query.bind(v); }
            if let Some(v) = &tags_val { query = query.bind(v); }
            if let Some(v) = &cat_val { query = query.bind(v); }
            if let Some(v) = &url_val { query = query.bind(v); }
            query = query.bind(id);
            let result = query.execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("资源不存在".to_string()));
            }
        }
    }
    Ok(())
}

/// 删除资源
pub async fn delete_resource(db: &DbPool, id: i64) -> Result<(), AppError> {
    let affected = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM extracted_resources WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected()
        }
        DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM extracted_resources WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected()
        }
    };
    if affected == 0 {
        return Err(AppError::NotFound("资源不存在".to_string()));
    }
    Ok(())
}

/// 资源统计
pub async fn get_resource_stats(db: &DbPool) -> Result<serde_json::Value, AppError> {
    let (total, pushed, unpushed): (i64, i64, i64) = match db {
        DbPool::Sqlite(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extracted_resources")
                .fetch_one(pool).await.unwrap_or(0);
            let pushed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extracted_resources WHERE is_pushed = 1")
                .fetch_one(pool).await.unwrap_or(0);
            let unpushed = total - pushed;
            (total, pushed, unpushed)
        }
        DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extracted_resources")
                .fetch_one(pool).await.unwrap_or(0);
            let pushed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extracted_resources WHERE is_pushed = TRUE")
                .fetch_one(pool).await.unwrap_or(0);
            let unpushed = total - pushed;
            (total, pushed, unpushed)
        }
    };

    // 按类别统计
    let by_category: Vec<(String, i64)> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT COALESCE(category, 'other') as category, COUNT(*) as cnt \
                 FROM extracted_resources GROUP BY category"
            ).fetch_all(pool).await.unwrap_or_default()
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT COALESCE(category, 'other') as category, COUNT(*) as cnt \
                 FROM extracted_resources GROUP BY category"
            ).fetch_all(pool).await.unwrap_or_default()
        }
    };
    let category_map: serde_json::Map<String, serde_json::Value> = by_category.into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect();

    // 按提取模式统计
    let by_mode: Vec<(String, i64)> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT extract_mode, COUNT(*) as cnt FROM extracted_resources GROUP BY extract_mode"
            ).fetch_all(pool).await.unwrap_or_default()
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT extract_mode, COUNT(*) as cnt FROM extracted_resources GROUP BY extract_mode"
            ).fetch_all(pool).await.unwrap_or_default()
        }
    };
    let mode_map: serde_json::Map<String, serde_json::Value> = by_mode.into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect();

    Ok(json!({
        "total": total,
        "pushed": pushed,
        "unpushed": unpushed,
        "by_category": category_map,
        "by_extract_mode": mode_map,
    }))
}

/// 推送资源 — 读取未推送资源并推送到外部 API
pub async fn push_resources(
    api_url: &str,
    api_token: &str,
    target: &str,
    batch_size: i64,
    db: &DbPool,
    _option_cache: &OptionCache,
) -> Result<serde_json::Value, AppError> {
    // 读取 is_pushed=false 的资源
    let resources: Vec<ExtractedResource> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT id, collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode, is_pushed, is_edited, created_at, updated_at \
                 FROM extracted_resources WHERE is_pushed = 0 LIMIT ?"
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT id, collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode, is_pushed, is_edited, created_at, updated_at \
                 FROM extracted_resources WHERE is_pushed = FALSE LIMIT $1"
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
    };

    if resources.is_empty() {
        return Ok(json!({
            "status": "success",
            "message": "没有需要推送的资源",
            "count": 0
        }));
    }

    // 转换为推送格式
    let push_data: Vec<serde_json::Value> = resources.iter().map(|r| {
        let urls: Vec<&str> = r.url.as_deref()
            .map(|u| u.split(',').map(|s| s.trim()).collect())
            .unwrap_or_default();
        json!({
            "title": r.title,
            "url": urls,
            "description": r.description,
            "category": r.category,
            "tags": r.tags,
            "img": r.img,
            "source": r.source,
            "extra": r.extra,
        })
    }).collect();

    let batch_id = format!("batch_{}_{}", target, chrono::Utc::now().timestamp());

    // 推送到外部 API
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Internal(format!("创建 HTTP 客户端失败: {e}")))?;

    let payload = json!({ "resources": push_data });

    let resp = client
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("X-API-Token", api_token)
        .json(&payload)
        .send()
        .await;

    match resp {
        Ok(response) => {
            let status_code = response.status();
            if status_code.is_success() {
                // 标记为已推送
                for r in &resources {
                    mark_resource_pushed(db, r.id).await?;
                }

                // 记录推送历史
                record_push_history(
                    &batch_id, target, "success",
                    resources.len() as i64,
                    "推送成功", None, db,
                ).await?;

                Ok(json!({
                    "status": "success",
                    "processed_count": resources.len(),
                    "batch_id": batch_id
                }))
            } else {
                let body = response.text().await.unwrap_or_default();
                record_push_history(
                    &batch_id, target, "failed",
                    resources.len() as i64,
                    &format!("API返回错误: {}", status_code),
                    Some(&body), db,
                ).await?;
                Err(AppError::Internal(format!(
                    "推送API返回错误: status={}, body={}",
                    status_code, body
                )))
            }
        }
        Err(e) => {
            record_push_history(
                &batch_id, target, "failed",
                0, "推送请求失败",
                Some(&e.to_string()), db,
            ).await?;
            Err(AppError::Internal(format!("推送请求失败: {e}")))
        }
    }
}

/// 标记资源为已推送
async fn mark_resource_pushed(db: &DbPool, id: i64) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query("UPDATE extracted_resources SET is_pushed = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query("UPDATE extracted_resources SET is_pushed = TRUE, updated_at = NOW() WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// 记录推送历史（复用 push_histories 表）
async fn record_push_history(
    batch_id: &str,
    target: &str,
    status: &str,
    data_count: i64,
    message: &str,
    error_msg: Option<&str>,
    db: &DbPool,
) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO push_histories (batch_id, target, status, data_count, message, error_msg) VALUES (?, ?, ?, ?, ?, ?)"
            )
            .bind(batch_id).bind(target).bind(status)
            .bind(data_count).bind(message).bind(error_msg)
            .execute(pool).await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO push_histories (batch_id, target, status, data_count, message, error_msg) VALUES ($1, $2, $3, $4, $5, $6)"
            )
            .bind(batch_id).bind(target).bind(status)
            .bind(data_count).bind(message).bind(error_msg)
            .execute(pool).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_where_clause_all() {
        let clause = build_where_clause(None, None);
        assert_eq!(clause, "1=1");
    }

    #[test]
    fn test_build_where_clause_unpushed() {
        let clause = build_where_clause(Some("unpushed"), None);
        assert!(clause.contains("is_pushed = 0"));
    }

    #[test]
    fn test_build_where_clause_with_category() {
        let clause = build_where_clause(Some("pushed"), Some("quark"));
        assert!(clause.contains("is_pushed = 1"));
        assert!(clause.contains("category = 'quark'"));
    }

    #[test]
    fn test_extraction_result_serialization() {
        let result = ExtractionResult {
            total_scanned: 100,
            extracted: 42,
            skipped: 55,
            errors: 3,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"total_scanned\":100"));
        assert!(json.contains("\"extracted\":42"));
    }
}
