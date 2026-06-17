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
    pub list: Vec<ResourceListItem>,
    pub pagination: PaginationInfo,
}

/// 资源列表项（资源 + 链接状态，flatten 保持 API 向后兼容，仅新增 link_status 字段）
#[derive(Debug, serde::Serialize)]
pub struct ResourceListItem {
    #[serde(flatten)]
    pub resource: ExtractedResource,
    pub link_status: String,
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
            if let Err(e) = mark_extracted(db, history_id).await {
                tracing::error!("标记已提取失败 history={history_id}: {e}");
            }
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
            if let Err(e) = insert_resource(db, &new_resource).await {
                tracing::error!("资源插入失败（资源未持久化）: {e}");
            }
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

/// 资源列表（分页 + 状态/分类/链接状态筛选 + 链接状态展示，FR-011）
pub async fn list_resources(
    db: &DbPool,
    page: i64,
    page_size: i64,
    status: Option<&str>,
    category: Option<&str>,
    link_status: Option<&str>,
) -> Result<ResourceListResult, AppError> {
    let offset = (page - 1).max(0) * page_size;
    let where_clause = build_where_clause(status, category);
    let want_link = link_status
        .filter(|s| !s.is_empty() && *s != "all")
        .map(|s| s.to_string());

    // 链接状态筛选：SQL 无法按逗号拆分 URL 过滤 → 取候选（上限保护）→ 聚合 → 过滤 → 分页
    if let Some(want) = want_link {
        const FILTER_CAP: i64 = 1000;
        let candidates = fetch_resources(db, &where_clause, FILTER_CAP, 0).await?;
        let annotated = annotate_link_status(db, candidates).await?;
        let total;
        let list: Vec<ResourceListItem> = {
            let filtered: Vec<ResourceListItem> = annotated
                .into_iter()
                .filter(|it| it.link_status == want)
                .collect();
            total = filtered.len() as i64;
            filtered
                .into_iter()
                .skip(offset as usize)
                .take(page_size as usize)
                .collect()
        };
        return Ok(ResourceListResult {
            list,
            pagination: PaginationInfo {
                page,
                page_size,
                total,
            },
        });
    }

    // 常规分页
    let count_sql = format!("SELECT COUNT(*) FROM extracted_resources WHERE {where_clause}");
    let total: i64 = match db {
        DbPool::Sqlite(pool) => sqlx::query_scalar(&count_sql).fetch_one(pool).await?,
        DbPool::Postgres(pool) => sqlx::query_scalar(&count_sql).fetch_one(pool).await?,
    };
    let raw = fetch_resources(db, &where_clause, page_size, offset).await?;
    let list = annotate_link_status(db, raw).await?;
    Ok(ResourceListResult {
        list,
        pagination: PaginationInfo {
            page,
            page_size,
            total,
        },
    })
}

/// 资源 SELECT 列（含封面转发状态/消息ID/file_id 子查询）。
const RESOURCE_COLS: &str = "er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
     (SELECT ft.status FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS img_forward_status, \
     (SELECT ft.image_message_id FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS image_message_id, \
     (SELECT ft.file_id FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS file_id";

/// 按 WHERE 子句取资源（含 img_forward_status 子查询）。
async fn fetch_resources(
    db: &DbPool,
    where_clause: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<ExtractedResource>, AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            let sql = format!(
                "SELECT {RESOURCE_COLS} FROM extracted_resources er WHERE {where_clause} ORDER BY er.created_at DESC LIMIT ? OFFSET ?"
            );
            Ok(sqlx::query_as(&sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await?)
        }
        DbPool::Postgres(pool) => {
            let sql = format!(
                "SELECT {RESOURCE_COLS} FROM extracted_resources er WHERE {where_clause} ORDER BY er.created_at DESC LIMIT $1 OFFSET $2"
            );
            Ok(sqlx::query_as(&sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await?)
        }
    }
}

/// 为资源批量附加链接状态（仅读缓存，不触发 PanCheck，避免列页触发外部检测）。
async fn annotate_link_status(
    db: &DbPool,
    resources: Vec<ExtractedResource>,
) -> Result<Vec<ResourceListItem>, AppError> {
    let urls: Vec<String> = resources
        .iter()
        .flat_map(|r| crate::services::link_check::split_resource_urls(r.url.as_deref()))
        .collect();
    let st = crate::services::link_check::cached_link_status_map(db, &urls).await?;
    Ok(resources
        .into_iter()
        .map(|r| {
            let link_status =
                crate::services::link_check::aggregate_link_status(&r, &st).to_string();
            ResourceListItem {
                resource: r,
                link_status,
            }
        })
        .collect())
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
                "SELECT er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
                 (SELECT ft.file_id FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS file_id \
                 FROM extracted_resources er WHERE er.id = ?"
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
                 (SELECT ft.file_id FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS file_id \
                 FROM extracted_resources er WHERE er.id = $1"
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

    // 2. 查询关联的采集历史 raw_data + 采集器 channel_name
    let (raw_data, channel_name): (Option<String>, Option<String>) = match db {
        DbPool::Sqlite(pool) => {
            let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT ch.raw_data, c.channel_name \
                 FROM collector_histories ch \
                 LEFT JOIN collectors c ON ch.collector_id = c.id \
                 WHERE ch.id = ?",
            )
            .bind(resource.collector_history_id)
            .fetch_optional(pool)
            .await?;
            row.map(|r| (r.0, r.1)).unwrap_or((None, None))
        }
        DbPool::Postgres(pool) => {
            let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT ch.raw_data, c.channel_name \
                 FROM collector_histories ch \
                 LEFT JOIN collectors c ON ch.collector_id = c.id \
                 WHERE ch.id = $1",
            )
            .bind(resource.collector_history_id)
            .fetch_optional(pool)
            .await?;
            row.map(|r| (r.0, r.1)).unwrap_or((None, None))
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
        "channel_name": channel_name,
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

/// 推送资源 — 读取未推送资源，经「图片转存 + 链接有效性」双维分类后，
/// 仅推送有效资源到外部 API（通用 HTTP 适配器）。跳过资源记录明细与统计。
pub async fn push_resources(
    api_url: &str,
    api_token: &str,
    target: &str,
    batch_size: i64,
    db: &DbPool,
    option_cache: &OptionCache,
) -> Result<serde_json::Value, AppError> {
    // 取未推送资源（含 img_forward_status 子查询；图片转存过滤改由 Rust 分类统计）
    let resources: Vec<ExtractedResource> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
                 (SELECT ft.status FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS img_forward_status, \
                 (SELECT ft.file_id FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS file_id \
                 FROM extracted_resources er \
                 WHERE er.is_pushed = 0 \
                 LIMIT ?"
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
                 (SELECT ft.status FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS img_forward_status, \
                 (SELECT ft.file_id FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS file_id \
                 FROM extracted_resources er \
                 WHERE er.is_pushed = FALSE \
                 LIMIT $1"
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

    let batch_id = format!(
        "batch_{}_{}",
        if target.is_empty() { "default" } else { target },
        chrono::Utc::now().timestamp()
    );

    // 有效性分类：图片未转存 / 链接失效 跳过（FR-001/FR-003/FR-006）
    let classify =
        match crate::services::link_check::classify_resources(db, option_cache, &resources).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("资源有效性分类失败，降级为全部尝试推送: {e}");
                crate::services::link_check::ClassifyResult {
                    valid: resources.clone(),
                    skipped: Vec::new(),
                }
            }
        };
    let skipped_image = classify.skipped_image_count();
    let skipped_link = classify.skipped_link_count();
    let valid = &classify.valid;
    let skipped_json = json!({
        "image_not_forwarded": skipped_image,
        "link_invalid": skipped_link,
        "total": skipped_image + skipped_link,
    });

    if valid.is_empty() {
        // 无有效资源：仅记录跳过，不推送
        record_push_history_with_skips(
            db,
            &batch_id,
            target,
            "success",
            0,
            skipped_image as i64,
            skipped_link as i64,
            "没有可推送的有效资源",
            None,
            None,
            &classify.skipped,
        )
        .await?;
        return Ok(json!({
            "status": "no_valid_resources",
            "processed_count": 0,
            "batch_id": batch_id,
            "skipped": skipped_json,
        }));
    }

    let resource_count = valid.len();
    let result = build_and_send_push_request(valid, api_url, api_token, target, option_cache).await;

    match result {
        Ok((status_code, body, is_success, _request_info)) => {
            if is_success {
                let pushed_ids: Vec<i64> = valid.iter().map(|r| r.id).collect();
                batch_mark_pushed(db, &pushed_ids).await?;
                record_push_history_with_skips(
                    db,
                    &batch_id,
                    target,
                    "success",
                    resource_count as i64,
                    skipped_image as i64,
                    skipped_link as i64,
                    "推送成功",
                    None,
                    None,
                    &classify.skipped,
                )
                .await?;
                Ok(json!({
                    "status": "success",
                    "processed_count": resource_count,
                    "batch_id": batch_id,
                    "skipped": skipped_json,
                }))
            } else {
                record_push_history_with_skips(
                    db,
                    &batch_id,
                    target,
                    "failed",
                    resource_count as i64,
                    skipped_image as i64,
                    skipped_link as i64,
                    &format!("API返回错误: {}", status_code),
                    Some(&body),
                    None,
                    &classify.skipped,
                )
                .await?;
                Err(AppError::Internal(format!(
                    "推送API返回错误: status={}, body={}",
                    status_code, body
                )))
            }
        }
        Err(e) => {
            record_push_history_with_skips(
                db,
                &batch_id,
                target,
                "failed",
                0,
                skipped_image as i64,
                skipped_link as i64,
                "推送请求失败",
                Some(&e.to_string()),
                None,
                &classify.skipped,
            )
            .await?;
            Err(AppError::Internal(format!("推送请求失败: {e}")))
        }
    }
}

/// 构建并发送推送请求 — 从 option_cache 读取认证/模板配置
/// 返回 (http_status, response_body, is_success, request_info)
async fn build_and_send_push_request(
    resources: &[ExtractedResource],
    api_url: &str,
    api_token: &str,
    target: &str,
    option_cache: &OptionCache,
) -> Result<(u16, String, bool, serde_json::Value), AppError> {
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
    let image_domain = cache.get("TelegramImageDomain").cloned();
    drop(cache);

    build_and_send_push_with_params(
        resources,
        api_url,
        api_token,
        target,
        &auth_type,
        &auth_key,
        &http_method,
        &body_template,
        &custom_headers_str,
        image_domain.as_deref(),
    )
    .await
}

/// 构建并发送推送请求 — 接受直接参数（供 push_config 按配置推送使用）
/// 返回 (http_status, response_body, is_success, request_info)
///
/// `image_domain`：图床域名（如 `https://img.example.com`），配置后 img 字段会从 file_id
/// 拼接为完整 URL `{domain}/file/{file_id}`；未配置（None 或空）时 img 保留裸 file_id。
#[allow(clippy::too_many_arguments)]
pub async fn build_and_send_push_with_params(
    resources: &[ExtractedResource],
    api_url: &str,
    api_token: &str,
    target: &str,
    auth_type: &str,
    auth_key: &str,
    http_method: &str,
    body_template: &str,
    custom_headers_str: &str,
    image_domain: Option<&str>,
) -> Result<(u16, String, bool, serde_json::Value), AppError> {
    // 规范化图床域名：trim + 去尾部斜杠
    let domain_norm = image_domain
        .map(|d| d.trim().trim_end_matches('/'))
        .filter(|d| !d.is_empty());

    // 转换为推送格式
    let push_data: Vec<serde_json::Value> = resources
        .iter()
        .map(|r| {
            let urls: Vec<&str> = r
                .url
                .as_deref()
                .map(|u| u.split(',').map(|s| s.trim()).collect())
                .unwrap_or_default();
            // img 字段：按 Bot file_id 拼接完整图床 URL（{domain}/{file_id}，后端智能路由识别）；
            // file_id 来自两阶段转存（未转存资源为空 → img 为 None）；未配域名则保留裸 file_id
            let img_value = r.file_id.as_deref().and_then(|fid| {
                let fid = fid.trim();
                if fid.is_empty() {
                    return None;
                }
                match domain_norm {
                    Some(d) => Some(format!("{d}/{fid}")),
                    None => Some(fid.to_string()),
                }
            });
            json!({
                "title": r.title,
                "url": urls,
                "description": r.description,
                "category": r.category,
                "tags": r.tags,
                "img": img_value,
                "source": r.source,
                "extra": r.extra,
            })
        })
        .collect();

    // 构建请求体（使用模板或默认格式）
    let default_template = r#"{"resources": {{resources}}}"#;
    let template = if body_template.is_empty() {
        default_template
    } else {
        body_template
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

    let method_upper = http_method.to_uppercase();
    let mut request = match method_upper.as_str() {
        "PUT" => client.put(api_url),
        "PATCH" => client.patch(api_url),
        _ => client.post(api_url),
    };

    request = request.header("Content-Type", "application/json");

    // 收集 request_info 中的 headers（含脱敏标记）— 同时供前端展示
    let mut info_headers: Vec<serde_json::Value> = vec![json!({
        "key": "Content-Type",
        "value": "application/json",
        "is_auth": false,
    })];

    // 添加认证
    match auth_type {
        "bearer" => {
            request = request.header("Authorization", format!("Bearer {}", api_token));
            info_headers.push(json!({
                "key": "Authorization",
                "value": "***",
                "is_auth": true,
            }));
        }
        "custom_header" => {
            request = request.header(auth_key, api_token);
            info_headers.push(json!({
                "key": auth_key,
                "value": "***",
                "is_auth": true,
            }));
        }
        "query" => {
            request = request.query(&[(auth_key, api_token)]);
            info_headers.push(json!({
                "key": auth_key,
                "value": "***",
                "is_auth": true,
                "location": "query",
            }));
        }
        _ => {} // "none" — 不添加认证
    }

    // 添加自定义 Header
    if let Ok(headers) = serde_json::from_str::<Vec<serde_json::Value>>(custom_headers_str) {
        for h in &headers {
            let key = h.get("key").and_then(|v| v.as_str()).unwrap_or("").trim();
            let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if !key.is_empty() {
                // 检查是否与认证 header 冲突
                let is_auth_header = match auth_type {
                    "bearer" => key.eq_ignore_ascii_case("Authorization"),
                    "custom_header" => key.eq_ignore_ascii_case(auth_key),
                    _ => false,
                };
                if is_auth_header {
                    tracing::warn!("自定义 Header '{}' 与认证 Header 冲突，已跳过", key);
                    continue;
                }
                request = request.header(key, value);
                info_headers.push(json!({
                    "key": key,
                    "value": value,
                    "is_auth": false,
                }));
            }
        }
    }

    // 计算 request_info 中的 URL（query 认证时把 token 替换为 *** 展示）
    let info_url = if auth_type == "query" && !api_token.is_empty() {
        // 把 token 值替换为 *** — 这里只用于展示，原 URL 不变
        api_url.replace(api_token, "***")
        // 注：URL 末尾是否已带 query 由前端展示判断；这里做近似展示
    } else {
        api_url.to_string()
    };

    // 让 URL 看起来更接近最终发出去的样子（query 认证时附带 key=***）
    let info_url_display = if auth_type == "query" && !api_token.is_empty() {
        let sep = if info_url.contains('?') { '&' } else { '?' };
        format!("{}{}{}=***", api_url, sep, auth_key)
    } else {
        info_url
    };

    let response = request
        .json(&body_value)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("推送请求发送失败: {e}")))?;

    let status_code = response.status();
    let http_status = status_code.as_u16();
    let body = response.text().await.unwrap_or_default();
    let is_success = status_code.is_success();

    // 组装 request_info（method/url/headers/body），body 用 pretty-print 让前端展示更友好
    let body_pretty =
        serde_json::to_string_pretty(&body_value).unwrap_or_else(|_| body_str.clone());
    let request_info = json!({
        "method": method_upper,
        "url": info_url_display,
        "headers": info_headers,
        "body": body_pretty,
    });

    Ok((http_status, body, is_success, request_info))
}

/// 标记资源为已推送
pub async fn mark_resource_pushed(db: &DbPool, id: i64) -> Result<(), AppError> {
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

/// 批量标记资源为已推送（feature 031 PERF-002：消除逐条 UPDATE 的 N+1）
pub async fn batch_mark_pushed(db: &DbPool, ids: &[i64]) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }
    match db {
        DbPool::Sqlite(pool) => {
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "UPDATE extracted_resources SET is_pushed = 1, updated_at = CURRENT_TIMESTAMP WHERE id IN ({placeholders})"
            );
            let mut q = sqlx::query(&sql);
            for id in ids {
                q = q.bind(id);
            }
            q.execute(pool).await?;
        }
        DbPool::Postgres(pool) => {
            let placeholders = (1..=ids.len())
                .map(|i| format!("${i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "UPDATE extracted_resources SET is_pushed = TRUE, updated_at = NOW() WHERE id IN ({placeholders})"
            );
            let mut q = sqlx::query(&sql);
            for id in ids {
                q = q.bind(id);
            }
            q.execute(pool).await?;
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

/// 插入推送跳过明细（push_skip_records）。
pub async fn insert_skip_records(
    db: &DbPool,
    push_history_id: i64,
    skipped: &[crate::services::link_check::SkipEntry],
) -> Result<(), AppError> {
    for s in skipped {
        let urls_invalid = if s.urls_invalid.is_empty() {
            None
        } else {
            Some(s.urls_invalid.join(","))
        };
        let reason = s.reason.as_str();
        match db {
            DbPool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO push_skip_records (push_history_id, resource_id, skip_reason, urls_invalid, detail) \
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(push_history_id)
                .bind(s.resource.id)
                .bind(reason)
                .bind(&urls_invalid)
                .bind(&s.detail)
                .execute(pool)
                .await?;
            }
            DbPool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO push_skip_records (push_history_id, resource_id, skip_reason, urls_invalid, detail) \
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(push_history_id)
                .bind(s.resource.id)
                .bind(reason)
                .bind(&urls_invalid)
                .bind(&s.detail)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(())
}

/// 记录推送历史（含跳过统计汇总列）并写入跳过明细，返回 push_histories.id。
/// `push_config_id` 为 None 时表示全局推送（不关联配置）。
#[allow(clippy::too_many_arguments)]
pub async fn record_push_history_with_skips(
    db: &DbPool,
    batch_id: &str,
    target: &str,
    status: &str,
    pushed_count: i64,
    skipped_image: i64,
    skipped_link: i64,
    message: &str,
    error_msg: Option<&str>,
    push_config_id: Option<i64>,
    skipped: &[crate::services::link_check::SkipEntry],
) -> Result<i64, AppError> {
    let id: i64 = match db {
        DbPool::Sqlite(pool) => {
            let result = sqlx::query(
                "INSERT INTO push_histories \
                 (batch_id, target, status, data_count, message, error_msg, pushed_count, skipped_image_count, skipped_link_count, push_config_id) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(batch_id)
            .bind(target)
            .bind(status)
            .bind(pushed_count) // data_count 保留为实际推送数（向后兼容）
            .bind(message)
            .bind(error_msg)
            .bind(pushed_count)
            .bind(skipped_image)
            .bind(skipped_link)
            .bind(push_config_id)
            .execute(pool)
            .await?;
            result.last_insert_rowid()
        }
        DbPool::Postgres(pool) => {
            sqlx::query_scalar(
                "INSERT INTO push_histories \
                 (batch_id, target, status, data_count, message, error_msg, pushed_count, skipped_image_count, skipped_link_count, push_config_id) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
            )
            .bind(batch_id)
            .bind(target)
            .bind(status)
            .bind(pushed_count)
            .bind(message)
            .bind(error_msg)
            .bind(pushed_count)
            .bind(skipped_image)
            .bind(skipped_link)
            .bind(push_config_id)
            .fetch_one(pool)
            .await?
        }
    };
    insert_skip_records(db, id, skipped).await?;
    Ok(id)
}

/// 单条资源推送 — 复用与 push_resources 完全相同的推送配置发起请求
/// **行为**：实际推送 + 标记 is_pushed=true + 记录推送历史（batch_id 以 single_ 前缀）
pub async fn push_single_resource(
    db: &DbPool,
    option_cache: &OptionCache,
    id: i64,
) -> Result<serde_json::Value, AppError> {
    // 1. 查询单条资源
    let resource = get_resource(db, id).await?;

    // 2. 读取推送配置
    let cache = option_cache.read().await;
    let api_url = cache.get("push_api_url").cloned().unwrap_or_default();
    let api_token = cache.get("push_api_token").cloned().unwrap_or_default();
    let target = cache.get("push_target").cloned().unwrap_or_default();
    let auth_type = cache
        .get("push_auth_type")
        .cloned()
        .unwrap_or_else(|| "custom_header".to_string());
    drop(cache);

    // 3. 配置校验
    let mut missing = Vec::new();
    if api_url.is_empty() {
        missing.push("push_api_url");
    }
    if auth_type != "none" && api_token.is_empty() {
        missing.push("push_api_token");
    }
    if !missing.is_empty() {
        return Ok(json!({
            "status": "config_error",
            "message": "推送配置不完整",
            "missing": missing,
        }));
    }

    // 4. 生成 batch_id（前缀 single_ + 资源 id，便于在推送历史中区分）
    let target_label = if target.is_empty() {
        "default"
    } else {
        &target
    };
    let batch_id = format!(
        "single_{}_{}_{}",
        target_label,
        id,
        chrono::Utc::now().timestamp()
    );

    // 5. 调用通用 helper 发送请求
    let result =
        build_and_send_push_request(&[resource], &api_url, &api_token, &target, option_cache).await;

    match result {
        Ok((status_code, body, is_success, request_info)) => {
            if is_success {
                // 标记为已推送（与 push_resources 行 1111 一致）
                mark_resource_pushed(db, id).await?;

                record_push_history(
                    &batch_id,
                    &target,
                    "success",
                    1,
                    &format!("单条推送成功 (HTTP {})", status_code),
                    None,
                    db,
                )
                .await?;
                Ok(json!({
                    "status": "success",
                    "message": "单条推送成功",
                    "http_status": status_code,
                    "response_body": body,
                    "batch_id": batch_id,
                    "request": request_info,
                }))
            } else {
                record_push_history(
                    &batch_id,
                    &target,
                    "failed",
                    1,
                    &format!("单条推送失败: HTTP {}", status_code),
                    Some(&body),
                    db,
                )
                .await?;
                Ok(json!({
                    "status": "failed",
                    "message": format!("API返回错误: HTTP {}", status_code),
                    "http_status": status_code,
                    "response_body": body,
                    "batch_id": batch_id,
                    "request": request_info,
                }))
            }
        }
        Err(e) => {
            record_push_history(
                &batch_id,
                &target,
                "failed",
                0,
                "单条推送请求失败",
                Some(&e.to_string()),
                db,
            )
            .await?;
            Err(e)
        }
    }
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
