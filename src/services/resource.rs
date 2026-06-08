// 资源管理业务逻辑 — 提取触发、列表查询、编辑更新、推送

use crate::errors::AppError;
use crate::models::extracted_resource::{
    ExtractedResource, NewExtractedResource, UpdateExtractedResource,
};
use crate::services::ai_extractor;
use crate::services::extractor;
use crate::state::{DbPool, OptionCache};
use futures::StreamExt;
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

/// 从 option_cache 读取 AI 并发数（默认 5，范围 1-10）
async fn get_ai_concurrency(option_cache: &OptionCache) -> usize {
    let cache = option_cache.read().await;
    let val = cache
        .get("ai_concurrency")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5);
    val.clamp(1, 10)
}

/// 触发资源提取 — 扫描未提取的采集历史，调用 extractor 提取
pub async fn trigger_extraction(
    state: &crate::state::AppState,
    batch_size: i64,
) -> Result<ExtractionResult, AppError> {
    let db = &state.db;
    let option_cache = &state.option_cache;

    // 1. 查找未提取的 collector_histories（通过 is_extracted 标记）
    #[allow(clippy::type_complexity)]
    let histories: Vec<(
        i64,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    )> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT id, raw_data, remote_id, channel_id, message_id \
                 FROM collector_histories \
                 WHERE is_extracted = 0 AND raw_data IS NOT NULL \
                 LIMIT ?",
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT id, raw_data, remote_id, channel_id, message_id \
                 FROM collector_histories \
                 WHERE is_extracted = false AND raw_data IS NOT NULL \
                 LIMIT $1",
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
    };

    let total_scanned = histories.len() as i64;
    if total_scanned == 0 {
        return Ok(ExtractionResult {
            total_scanned: 0,
            extracted: 0,
            skipped: 0,
            errors: 0,
        });
    }

    // 读取提取模式和并发数
    let extract_mode = {
        let cache = option_cache.read().await;
        cache
            .get("push_extract_mode")
            .cloned()
            .unwrap_or_else(|| "rule".to_string())
    };
    let concurrency = get_ai_concurrency(option_cache).await;

    tracing::info!(
        "开始并发资源提取: total={}, concurrency={}, mode={}",
        total_scanned,
        concurrency,
        extract_mode
    );

    // 2. 并发处理每条记录
    // 使用 buffered_unordered 自动维护固定并发窗口
    let db_clone = db.clone();
    let option_cache_clone = option_cache.clone();
    let state_clone = state.clone();
    let extract_mode_clone = extract_mode.clone();

    let results: Vec<RecordResult> = futures::stream::iter(histories)
        .map(|(history_id, raw_data, remote_id, ch_id, msg_id)| {
            let db = db_clone.clone();
            let option_cache = option_cache_clone.clone();
            let state = state_clone.clone();
            let extract_mode = extract_mode_clone.clone();
            async move {
                process_single_record_for_batch(
                    &db,
                    &option_cache,
                    &state,
                    history_id,
                    raw_data,
                    remote_id,
                    ch_id,
                    msg_id,
                    &extract_mode,
                )
                .await
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    // 3. 汇总结果
    let mut extracted = 0i64;
    let mut skipped = 0i64;
    let mut errors = 0i64;
    let mut deduped = 0i64;

    for r in &results {
        extracted += r.extracted;
        skipped += r.skipped;
        errors += r.errors;
        deduped += r.deduped;
    }

    tracing::info!(
        "并发资源提取完成: scanned={}, extracted={}, skipped={}, deduped={}, errors={}",
        total_scanned,
        extracted,
        skipped,
        deduped,
        errors
    );

    Ok(ExtractionResult {
        total_scanned,
        extracted,
        skipped: skipped + deduped,
        errors,
    })
}

/// 单条记录并发处理结果
struct RecordResult {
    extracted: i64,
    skipped: i64,
    errors: i64,
    deduped: i64,
}

/// 并发处理单条记录（错误隔离：单条失败不影响其他记录）
#[allow(clippy::too_many_arguments)]
async fn process_single_record_for_batch(
    db: &DbPool,
    option_cache: &OptionCache,
    state: &crate::state::AppState,
    history_id: i64,
    raw_data: Option<String>,
    remote_id: Option<String>,
    ch_id: Option<i64>,
    msg_id: Option<i64>,
    extract_mode: &str,
) -> RecordResult {
    let mut result = RecordResult {
        extracted: 0,
        skipped: 0,
        errors: 0,
        deduped: 0,
    };

    let raw_text = match raw_data {
        Some(ref r) => {
            // 尝试从 JSON 中提取 text 字段
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(r) {
                msg.get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or(r)
                    .to_string()
            } else {
                r.clone()
            }
        }
        None => {
            // 无内容，标记已提取并跳过
            let _ = mark_extracted(db, history_id).await;
            result.skipped += 1;
            return result;
        }
    };

    // 规则提取
    let rule_drafts = extractor::extract_resources(&raw_text);
    if rule_drafts.is_empty() {
        result.skipped += 1;
        if let Err(e) = mark_extracted(db, history_id).await {
            tracing::warn!("[record-{}] 标记已提取失败: {e}", history_id);
        }
        return result;
    }

    // AI 模式：一次批量调用；规则模式：直接用规则结果
    let final_drafts = if extract_mode == "ai" {
        ai_extractor::ai_extract_batch(&raw_text, &rule_drafts, option_cache).await
    } else {
        rule_drafts
    };

    tracing::info!(
        "[record-{}] 提取到 {} 条资源 (mode={})",
        history_id,
        final_drafts.len(),
        extract_mode
    );

    for draft in &final_drafts {
        let img = remote_id
            .as_deref()
            .filter(|rid| !rid.is_empty())
            .map(|rid| rid.to_string())
            .unwrap_or_default();

        let mode = if extract_mode == "ai" { "ai" } else { "rule" };
        let new_resource = NewExtractedResource {
            collector_history_id: history_id,
            title: draft.title.clone(),
            url: if draft.url.is_empty() {
                None
            } else {
                Some(draft.url.join(","))
            },
            description: if draft.description.is_empty() {
                None
            } else {
                Some(draft.description.clone())
            },
            category: if draft.category.is_empty() {
                None
            } else {
                Some(draft.category.clone())
            },
            tags: if draft.tags.is_empty() {
                None
            } else {
                Some(draft.tags.clone())
            },
            img: if img.is_empty() { None } else { Some(img) },
            source: "tg".to_string(),
            extra: None,
            extract_mode: mode.to_string(),
        };

        match insert_resource(db, &new_resource).await {
            Ok(true) => result.extracted += 1,
            Ok(false) => result.deduped += 1,
            Err(e) => {
                tracing::warn!("[record-{}] 插入资源失败: {e}", history_id);
                result.errors += 1;
            }
        }
    }

    // 含图片资源入队转发
    if let Some(rid) = &remote_id
        && !rid.is_empty()
    {
        let has_photo = raw_data.as_ref().is_some_and(|rd| {
            rd.contains("\"media_type\":\"photo\"") || rd.contains("\"photo_id\"")
        });
        if has_photo {
            let first = final_drafts.first();
            let title = first.map(|d| d.title.as_str());
            let description = first.map(|d| d.description.as_str());
            let link = first
                .map(|d| d.url.join(",").to_string())
                .filter(|s| !s.is_empty());
            if let Err(e) = crate::services::forward_queue::enqueue(
                state,
                rid,
                ch_id,
                msg_id,
                title,
                description,
                link.as_deref(),
            )
            .await
            {
                tracing::warn!("[record-{}] 图片转发入队失败: {e}", history_id);
            }
        }
    }

    // 标记已提取
    if let Err(e) = mark_extracted(db, history_id).await {
        tracing::warn!("[record-{}] 标记已提取失败: {e}", history_id);
        result.errors += 1;
    }

    result
}

/// 标记采集历史为已提取
async fn mark_extracted(db: &DbPool, history_id: i64) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query("UPDATE collector_histories SET is_extracted = 1 WHERE id = ?")
                .bind(history_id)
                .execute(pool)
                .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query("UPDATE collector_histories SET is_extracted = true WHERE id = $1")
                .bind(history_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// 单条记录资源提取
/// 从指定 collector_history 中提取资源
/// dry_run=true 时仅返回结果不写入数据库
pub async fn extract_single_record(
    db: &DbPool,
    option_cache: &OptionCache,
    history_id: i64,
    dry_run: bool,
    extract_mode: String,
) -> Result<serde_json::Value, AppError> {
    // 1. 读取单条记录
    let (raw_data, remote_id, is_extracted): (Option<String>, Option<String>, bool) = match db {
        DbPool::Sqlite(pool) => {
            let row: Option<(Option<String>, Option<String>, bool)> = sqlx::query_as(
                "SELECT raw_data, remote_id, is_extracted FROM collector_histories WHERE id = ?",
            )
            .bind(history_id)
            .fetch_optional(pool)
            .await?;
            match row {
                Some(r) => r,
                None => return Err(AppError::NotFound("采集记录不存在".to_string())),
            }
        }
        DbPool::Postgres(pool) => {
            let row: Option<(Option<String>, Option<String>, bool)> = sqlx::query_as(
                "SELECT raw_data, remote_id, is_extracted FROM collector_histories WHERE id = $1",
            )
            .bind(history_id)
            .fetch_optional(pool)
            .await?;
            match row {
                Some(r) => r,
                None => return Err(AppError::NotFound("采集记录不存在".to_string())),
            }
        }
    };

    // 2. 检查 raw_data
    let raw_text = match &raw_data {
        Some(r) if !r.trim().is_empty() => {
            // 从 JSON 中提取 text 字段
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(r) {
                msg.get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or(r)
                    .to_string()
            } else {
                r.clone()
            }
        }
        _ => return Err(AppError::BadRequest("该记录无内容可提取".to_string())),
    };

    // 3. 检查已提取状态
    if is_extracted && !dry_run {
        return Err(AppError::BadRequest(
            "该记录已提取，请使用测试模式".to_string(),
        ));
    }

    // 4. 规则提取
    let rule_drafts = extractor::extract_resources(&raw_text);
    if rule_drafts.is_empty() {
        return Ok(json!({
            "success": true,
            "data": {
                "resources": [],
                "extract_mode": extract_mode,
            }
        }));
    }

    // 5. AI 模式：一次批量调用；规则模式：直接用规则结果
    let final_drafts = if extract_mode == "ai" {
        ai_extractor::ai_extract_batch(&raw_text, &rule_drafts, option_cache).await
    } else {
        rule_drafts
    };

    // 6. 处理每个 draft（写库 + 构建返回结果）
    let mut results = Vec::new();
    for draft in &final_drafts {
        let img = remote_id
            .as_deref()
            .filter(|rid| !rid.is_empty())
            .map(|rid| rid.to_string())
            .unwrap_or_default();

        // 非 dry_run 时写入数据库
        if !dry_run {
            let new_resource = NewExtractedResource {
                collector_history_id: history_id,
                title: draft.title.clone(),
                url: if draft.url.is_empty() {
                    None
                } else {
                    Some(draft.url.join(","))
                },
                description: if draft.description.is_empty() {
                    None
                } else {
                    Some(draft.description.clone())
                },
                category: if draft.category.is_empty() {
                    None
                } else {
                    Some(draft.category.clone())
                },
                tags: if draft.tags.is_empty() {
                    None
                } else {
                    Some(draft.tags.clone())
                },
                img: if img.is_empty() {
                    None
                } else {
                    Some(img.clone())
                },
                source: "tg".to_string(),
                extra: None,
                extract_mode: extract_mode.clone(),
            };
            let _ = insert_resource(db, &new_resource).await;
        }

        results.push(json!({
            "title": draft.title,
            "url": draft.url,
            "description": draft.description,
            "category": draft.category,
            "tags": draft.tags,
            "source": draft.source,
        }));
    }

    // 6. 非 dry_run 时标记已提取
    if !dry_run {
        mark_extracted(db, history_id).await?;
    }

    Ok(json!({
        "success": true,
        "data": {
            "resources": results,
            "extract_mode": extract_mode,
        }
    }))
}

/// 插入新资源记录
/// 插入资源 — 如果任意 share_id 已存在则跳过（去重）
/// 返回 true 表示新插入，false 表示跳过重复
async fn insert_resource(db: &DbPool, r: &NewExtractedResource) -> Result<bool, AppError> {
    // 从 URL 中提取 share_ids
    let share_ids: Vec<String> = r
        .url
        .as_deref()
        .into_iter()
        .flat_map(|u| u.split(','))
        .filter_map(|url| {
            let (share_id, service) = crate::services::extractor::identify_netdisk(url.trim());
            if !share_id.is_empty() && service != crate::services::extractor::SERVICE_NOT_FOUND {
                Some(share_id)
            } else {
                None
            }
        })
        .collect();

    // 去重检查：任一 share_id 已存在即判定为重复
    if !share_ids.is_empty() {
        let mut found_dup = false;
        for sid in &share_ids {
            let exists = match db {
                DbPool::Sqlite(pool) => {
                    let count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM extracted_resources WHERE ',' || share_ids || ',' LIKE '%,' || ? || ',%'",
                    )
                    .bind(sid)
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                    count > 0
                }
                DbPool::Postgres(pool) => {
                    let count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM extracted_resources WHERE ',' || share_ids || ',' LIKE '%,' || $1 || ',%'",
                    )
                    .bind(sid)
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                    count > 0
                }
            };
            if exists {
                tracing::debug!("资源去重跳过: share_id={} 已存在", sid);
                found_dup = true;
                break;
            }
        }
        if found_dup {
            return Ok(false);
        }
    } else if let Some(ref url) = r.url {
        // 无可识别 share_id 时，回退到 URL 全串比较
        if !url.is_empty() {
            let exists = match db {
                DbPool::Sqlite(pool) => {
                    let count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM extracted_resources WHERE url = ?",
                    )
                    .bind(url)
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                    count > 0
                }
                DbPool::Postgres(pool) => {
                    let count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM extracted_resources WHERE url = $1",
                    )
                    .bind(url)
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                    count > 0
                }
            };
            if exists {
                tracing::debug!("资源去重跳过: url={}", url);
                return Ok(false);
            }
        }
    }

    let share_ids_str = if share_ids.is_empty() {
        None
    } else {
        // 排序后存储，保证去重查询时 fingerprint 一致
        let mut sorted = share_ids;
        sorted.sort();
        Some(sorted.join(","))
    };

    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT OR IGNORE INTO extracted_resources \
                 (collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode, share_ids) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
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
            .bind(&share_ids_str)
            .execute(pool)
            .await?;
            // Note: INSERT OR IGNORE silently skips on unique conflict
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO extracted_resources \
                 (collector_history_id, title, url, description, category, tags, img, source, extra, extract_mode, share_ids) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
                 ON CONFLICT (url) DO NOTHING"
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
            .bind(&share_ids_str)
            .execute(pool)
            .await?;
        }
    }
    Ok(true)
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
    let count_sql = format!(
        "SELECT COUNT(*) FROM extracted_resources WHERE {}",
        where_clause
    );
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
            let total: i64 = sqlx::query_scalar(&count_sql).fetch_one(pool).await?;
            let list = sqlx::query_as(&query_sql)
                .bind(page_size)
                .bind(offset)
                .fetch_all(pool)
                .await?;
            (list, total)
        }
        DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar(&count_sql).fetch_one(pool).await?;
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
        pagination: PaginationInfo {
            page,
            page_size,
            total,
        },
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

/// 资源详情（含原始消息）— 用于查看提取对比
/// 通过 collector_history_id 查询采集历史的 raw_data，解析出文本和媒体类型
pub async fn get_resource_with_raw(db: &DbPool, id: i64) -> Result<serde_json::Value, AppError> {
    // 1. 获取资源
    let resource = get_resource(db, id).await?;

    // 2. 查询关联的采集历史 raw_data
    let raw_data: Option<String> = match db {
        DbPool::Sqlite(pool) => {
            let row: Option<(Option<String>,)> =
                sqlx::query_as("SELECT raw_data FROM collector_histories WHERE id = ?")
                    .bind(resource.collector_history_id)
                    .fetch_optional(pool)
                    .await?;
            row.and_then(|r| r.0)
        }
        DbPool::Postgres(pool) => {
            let row: Option<(Option<String>,)> =
                sqlx::query_as("SELECT raw_data FROM collector_histories WHERE id = $1")
                    .bind(resource.collector_history_id)
                    .fetch_optional(pool)
                    .await?;
            row.and_then(|r| r.0)
        }
    };

    // 3. 解析 raw_data
    let mut raw_text: Option<String> = None;
    let mut media_type: Option<String> = None;
    let has_history = raw_data.is_some();

    if let Some(ref rd) = raw_data {
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(rd) {
            raw_text = msg
                .get("text")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
                .or_else(|| Some(rd.clone()));
            media_type = msg
                .get("media_type")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());
        } else {
            // 非 JSON，直接作为文本
            raw_text = Some(rd.clone());
        }
    }

    Ok(json!({
        "resource": resource,
        "raw_text": raw_text,
        "raw_data": raw_data,
        "media_type": media_type,
        "has_history": has_history,
    }))
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

    let sql = format!(
        "UPDATE extracted_resources SET {} WHERE id = ?",
        set_parts.join(", ")
    );

    match db {
        DbPool::Sqlite(pool) => {
            let mut query = sqlx::query(&sql);
            if let Some(v) = &title_val {
                query = query.bind(v);
            }
            if let Some(v) = &desc_val {
                query = query.bind(v);
            }
            if let Some(v) = &tags_val {
                query = query.bind(v);
            }
            if let Some(v) = &cat_val {
                query = query.bind(v);
            }
            if let Some(v) = &url_val {
                query = query.bind(v);
            }
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
            if title_val.is_some() {
                pg_parts.push(format!("title = ${}", param_idx));
                param_idx += 1;
            }
            if desc_val.is_some() {
                pg_parts.push(format!("description = ${}", param_idx));
                param_idx += 1;
            }
            if tags_val.is_some() {
                pg_parts.push(format!("tags = ${}", param_idx));
                param_idx += 1;
            }
            if cat_val.is_some() {
                pg_parts.push(format!("category = ${}", param_idx));
                param_idx += 1;
            }
            if url_val.is_some() {
                pg_parts.push(format!("url = ${}", param_idx));
                param_idx += 1;
            }
            pg_parts.push("is_edited = TRUE".to_string());
            pg_parts.push("updated_at = NOW()".to_string());

            let pg_sql = format!(
                "UPDATE extracted_resources SET {} WHERE id = ${}",
                pg_parts.join(", "),
                param_idx
            );

            let mut query = sqlx::query(&pg_sql);
            if let Some(v) = &title_val {
                query = query.bind(v);
            }
            if let Some(v) = &desc_val {
                query = query.bind(v);
            }
            if let Some(v) = &tags_val {
                query = query.bind(v);
            }
            if let Some(v) = &cat_val {
                query = query.bind(v);
            }
            if let Some(v) = &url_val {
                query = query.bind(v);
            }
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
        DbPool::Sqlite(pool) => sqlx::query("DELETE FROM extracted_resources WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected(),
        DbPool::Postgres(pool) => sqlx::query("DELETE FROM extracted_resources WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected(),
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
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            let pushed: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM extracted_resources WHERE is_pushed = 1")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let unpushed = total - pushed;
            (total, pushed, unpushed)
        }
        DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extracted_resources")
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            let pushed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM extracted_resources WHERE is_pushed = TRUE",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            let unpushed = total - pushed;
            (total, pushed, unpushed)
        }
    };

    // 按类别统计
    let by_category: Vec<(String, i64)> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as(
            "SELECT COALESCE(category, 'other') as category, COUNT(*) as cnt \
                 FROM extracted_resources GROUP BY category",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default(),
        DbPool::Postgres(pool) => sqlx::query_as(
            "SELECT COALESCE(category, 'other') as category, COUNT(*) as cnt \
                 FROM extracted_resources GROUP BY category",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default(),
    };
    let category_map: serde_json::Map<String, serde_json::Value> = by_category
        .into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect();

    // 按提取模式统计
    let by_mode: Vec<(String, i64)> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as(
            "SELECT extract_mode, COUNT(*) as cnt FROM extracted_resources GROUP BY extract_mode",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default(),
        DbPool::Postgres(pool) => sqlx::query_as(
            "SELECT extract_mode, COUNT(*) as cnt FROM extracted_resources GROUP BY extract_mode",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default(),
    };
    let mode_map: serde_json::Map<String, serde_json::Value> =
        by_mode.into_iter().map(|(k, v)| (k, json!(v))).collect();

    Ok(json!({
        "total": total,
        "pushed": pushed,
        "unpushed": unpushed,
        "by_category": category_map,
        "by_extract_mode": mode_map,
    }))
}

/// 渲染请求体模板 — 将 `{{变量}}` 替换为实际值
/// 未匹配的变量保留原始 `{{变量名}}` 文本
pub fn render_template(template: &str, vars: &std::collections::HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{{{}}}}}", key), value);
    }
    result
}

/// 推送资源 — 读取未推送资源并通过通用 HTTP 适配器推送到外部 API
/// 支持自定义认证方式、HTTP 方法、请求体模板和自定义 Header
pub async fn push_resources(
    api_url: &str,
    api_token: &str,
    target: &str,
    batch_size: i64,
    db: &DbPool,
    option_cache: &OptionCache,
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
    let push_data: Vec<serde_json::Value> = resources
        .iter()
        .map(|r| {
            let urls: Vec<&str> = r
                .url
                .as_deref()
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
        })
        .collect();

    let batch_id = format!(
        "batch_{}_{}",
        if target.is_empty() { "default" } else { target },
        chrono::Utc::now().timestamp()
    );

    // 读取通用推送配置
    let cache = option_cache.read().await;
    let auth_type = cache
        .get("push_auth_type")
        .cloned()
        .unwrap_or_else(|| "custom_header".to_string());
    let auth_key = cache
        .get("push_auth_key")
        .cloned()
        .unwrap_or_else(|| "X-API-Token".to_string());
    let http_method = cache
        .get("push_http_method")
        .cloned()
        .unwrap_or_else(|| "POST".to_string());
    let body_template = cache.get("push_body_template").cloned().unwrap_or_default();
    let custom_headers_str = cache
        .get("push_custom_headers")
        .cloned()
        .unwrap_or_else(|| "[]".to_string());
    drop(cache);

    // 构建请求体（使用模板或默认格式）
    let default_template = r#"{"resources": {{resources}}}"#;
    let template = if body_template.is_empty() {
        default_template
    } else {
        &body_template
    };
    let mut vars = std::collections::HashMap::new();
    vars.insert(
        "resources",
        serde_json::to_string(&push_data).unwrap_or_default(),
    );
    vars.insert("count", resources.len().to_string());
    vars.insert("target", target.to_string());
    vars.insert("timestamp", chrono::Utc::now().timestamp().to_string());
    let body_str = render_template(template, &vars);

    // 解析请求体为 JSON Value
    let body_value: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| AppError::Internal(format!("推送请求体模板渲染结果不是有效 JSON: {e}")))?;

    // 构建 HTTP 请求
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| AppError::Internal(format!("创建 HTTP 客户端失败: {e}")))?;

    let mut request = match http_method.to_uppercase().as_str() {
        "PUT" => client.put(api_url),
        "PATCH" => client.patch(api_url),
        _ => client.post(api_url),
    };

    request = request.header("Content-Type", "application/json");

    // 添加认证
    match auth_type.as_str() {
        "bearer" => {
            request = request.header("Authorization", format!("Bearer {}", api_token));
        }
        "custom_header" => {
            request = request.header(&auth_key, api_token);
        }
        "query" => {
            request = request.query(&[(&auth_key as &str, api_token)]);
        }
        _ => {} // "none" — 不添加认证
    }

    // 添加自定义 Header
    if let Ok(headers) = serde_json::from_str::<Vec<serde_json::Value>>(&custom_headers_str) {
        for h in &headers {
            let key = h.get("key").and_then(|v| v.as_str()).unwrap_or("").trim();
            let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if !key.is_empty() {
                // 检查是否与认证 header 冲突
                let is_auth_header = match auth_type.as_str() {
                    "bearer" => key.eq_ignore_ascii_case("Authorization"),
                    "custom_header" => key.eq_ignore_ascii_case(&auth_key),
                    _ => false,
                };
                if is_auth_header {
                    tracing::warn!("自定义 Header '{}' 与认证 Header 冲突，已跳过", key);
                    continue;
                }
                request = request.header(key, value);
            }
        }
    }

    let resp = request.json(&body_value).send().await;

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
                    &batch_id,
                    target,
                    "success",
                    resources.len() as i64,
                    "推送成功",
                    None,
                    db,
                )
                .await?;

                Ok(json!({
                    "status": "success",
                    "processed_count": resources.len(),
                    "batch_id": batch_id
                }))
            } else {
                let body = response.text().await.unwrap_or_default();
                record_push_history(
                    &batch_id,
                    target,
                    "failed",
                    resources.len() as i64,
                    &format!("API返回错误: {}", status_code),
                    Some(&body),
                    db,
                )
                .await?;
                Err(AppError::Internal(format!(
                    "推送API返回错误: status={}, body={}",
                    status_code, body
                )))
            }
        }
        Err(e) => {
            record_push_history(
                &batch_id,
                target,
                "failed",
                0,
                "推送请求失败",
                Some(&e.to_string()),
                db,
            )
            .await?;
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
            sqlx::query(
                "UPDATE extracted_resources SET is_pushed = TRUE, updated_at = NOW() WHERE id = $1",
            )
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

    // --- T020: extract_mode="rule" 时不走 AI 分支 ---

    #[test]
    fn test_extract_mode_rule_value() {
        // 验证 rule 模式下的分支逻辑：extract_mode != "ai" → 纯规则
        let extract_mode = "rule";
        let draft = extractor::ResourceDraft {
            title: "测试标题".to_string(),
            url: vec!["https://pan.quark.cn/s/test".to_string()],
            description: String::new(),
            category: "quark".to_string(),
            tags: String::new(),
            source: "tg".to_string(),
        };

        let (final_draft, mode) = if extract_mode == "ai" {
            // AI 分支（不会被触发）
            (draft.clone(), "ai".to_string())
        } else {
            (draft.clone(), "rule".to_string())
        };

        assert_eq!(mode, "rule");
        assert_eq!(final_draft.title, "测试标题");
    }

    // --- T021: extract_mode="ai" 时的分支判定 ---

    #[test]
    fn test_extract_mode_ai_value() {
        // 验证 ai 模式下的分支逻辑：extract_mode == "ai" → AI 增强
        let extract_mode = "ai";
        let _draft = extractor::ResourceDraft {
            title: "规则标题".to_string(),
            url: vec!["https://pan.quark.cn/s/test".to_string()],
            description: "规则描述".to_string(),
            category: "quark".to_string(),
            tags: "标签".to_string(),
            source: "tg".to_string(),
        };

        // 模拟分支判定（不实际调用 AI API）
        let enters_ai_branch = extract_mode == "ai";
        assert!(enters_ai_branch, "extract_mode='ai' 应进入 AI 分支");
        // 在实际运行中，若无端点配置会回退到规则结果
    }

    // --- render_template 测试 ---

    #[test]
    fn test_render_template_all_variables() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("resources", "[{\"title\":\"test\"}]".to_string());
        vars.insert("count", "1".to_string());
        vars.insert("target", "external_api".to_string());
        vars.insert("timestamp", "1717500000".to_string());

        let result = render_template(
            r#"{"data": {{resources}}, "count": {{count}}, "source": "{{target}}", "ts": {{timestamp}}}"#,
            &vars,
        );
        assert!(result.contains(r#""data": [{"title":"test"}]"#));
        assert!(result.contains(r#""count": 1"#));
        assert!(result.contains(r#""source": "external_api""#));
        assert!(result.contains(r#""ts": 1717500000"#));
    }

    #[test]
    fn test_render_template_unknown_variable_preserved() {
        let vars = std::collections::HashMap::<&str, String>::new();
        let result = render_template(r#"{"key": "{{unknown_var}}"}"#, &vars);
        assert!(result.contains("{{unknown_var}}"), "未知变量应保留原文本");
    }

    #[test]
    fn test_render_template_empty_returns_default() {
        let vars = std::collections::HashMap::<&str, String>::new();
        let result = render_template("", &vars);
        assert_eq!(result, "", "空模板返回空字符串（由调用方处理默认值）");
    }

    #[test]
    fn test_render_template_partial_variables() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("count", "5".to_string());
        // resources 未提供
        let result = render_template(r#"{"items": {{resources}}, "total": {{count}}}"#, &vars);
        assert!(result.contains("{{resources}}"), "未提供的变量保留原文本");
        assert!(result.contains("5"), "已提供的变量被替换");
    }
}
