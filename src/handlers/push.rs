use crate::errors::AppError;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub batch_size: Option<i64>,
    pub push_config_id: Option<i64>,
}

pub async fn trigger_push(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    // [Deprecated] 兼容旧路由 — 查找第一个已启用配置执行推送
    let first_config: Option<crate::models::push_config::PushConfig> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT * FROM push_configs WHERE is_active = 1 AND api_url != '' ORDER BY id ASC LIMIT 1",
            )
            .fetch_optional(pool)
            .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT * FROM push_configs WHERE is_active = TRUE AND api_url != '' ORDER BY id ASC LIMIT 1",
            )
            .fetch_optional(pool)
            .await?
        }
    };

    let config = match first_config {
        Some(c) => c,
        None => {
            return Ok(Json(json!({
                "success": false,
                "message": "没有可用的推送配置，请先创建推送配置",
            })));
        }
    };

    let batch_size = body.get("batch_size").and_then(|v| v.as_i64());
    match crate::services::push_config::push_for_config(
        &state.db,
        &state.option_cache,
        config.id,
        batch_size,
    )
    .await
    {
        Ok(result) => Ok(Json(json!({ "success": true, "data": result }))),
        Err(e) => Ok(Json(
            json!({ "success": false, "message": format!("推送失败: {e}") }),
        )),
    }
}

pub async fn get_stats(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let stats = crate::services::push::get_stats(&state.db).await?;
    Ok(Json(json!({ "success": true, "data": stats })))
}

pub async fn list_histories(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let (list, total): (Vec<crate::models::push_history::PushHistory>, i64) = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let (total, list) = if let Some(config_id) = params.push_config_id {
                let total: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM push_histories WHERE push_config_id = ?",
                )
                .bind(config_id)
                .fetch_one(pool)
                .await?;
                let list = sqlx::query_as(
                    "SELECT id, batch_id, target, status, data_count, message, error_msg, pushed_count, skipped_image_count, skipped_link_count, pushed_at FROM push_histories WHERE push_config_id = ? ORDER BY id DESC LIMIT ? OFFSET ?"
                ).bind(config_id).bind(page_size).bind(offset).fetch_all(pool).await?;
                (total, list)
            } else {
                let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_histories")
                    .fetch_one(pool)
                    .await?;
                let list = sqlx::query_as(
                    "SELECT id, batch_id, target, status, data_count, message, error_msg, pushed_count, skipped_image_count, skipped_link_count, pushed_at FROM push_histories ORDER BY id DESC LIMIT ? OFFSET ?"
                ).bind(page_size).bind(offset).fetch_all(pool).await?;
                (total, list)
            };
            (list, total)
        }
        crate::state::DbPool::Postgres(pool) => {
            let (total, list) = if let Some(config_id) = params.push_config_id {
                let total: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM push_histories WHERE push_config_id = $1",
                )
                .bind(config_id)
                .fetch_one(pool)
                .await?;
                let list = sqlx::query_as(
                    "SELECT id, batch_id, target, status, data_count, message, error_msg, pushed_count, skipped_image_count, skipped_link_count, pushed_at FROM push_histories WHERE push_config_id = $1 ORDER BY id DESC LIMIT $2 OFFSET $3"
                ).bind(config_id).bind(page_size).bind(offset).fetch_all(pool).await?;
                (total, list)
            } else {
                let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_histories")
                    .fetch_one(pool)
                    .await?;
                let list = sqlx::query_as(
                    "SELECT id, batch_id, target, status, data_count, message, error_msg, pushed_count, skipped_image_count, skipped_link_count, pushed_at FROM push_histories ORDER BY id DESC LIMIT $1 OFFSET $2"
                ).bind(page_size).bind(offset).fetch_all(pool).await?;
                (total, list)
            };
            (list, total)
        }
    };
    Ok(Json(
        json!({ "success": true, "data": { "list": list, "pagination": { "page": page, "page_size": page_size, "total": total } } }),
    ))
}

/// GET /api/push/histories/{id} — 推送历史详情（含跳过明细 skip_records，Story3 AC2）
pub async fn get_push_history_detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let history: Option<crate::models::push_history::PushHistory> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT id, batch_id, target, status, data_count, message, error_msg, pushed_count, skipped_image_count, skipped_link_count, pushed_at \
                 FROM push_histories WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT id, batch_id, target, status, data_count, message, error_msg, pushed_count, skipped_image_count, skipped_link_count, pushed_at \
                 FROM push_histories WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
    };
    let history = history.ok_or_else(|| AppError::NotFound("推送历史不存在".into()))?;

    // 跳过明细（关联资源标题）
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, Option<String>, Option<String>, Option<String>)> =
        match &state.db {
            crate::state::DbPool::Sqlite(pool) => sqlx::query_as(
                "SELECT psr.resource_id, psr.skip_reason, psr.urls_invalid, psr.detail, er.title \
                 FROM push_skip_records psr \
                 LEFT JOIN extracted_resources er ON er.id = psr.resource_id \
                 WHERE psr.push_history_id = ? ORDER BY psr.id ASC",
            )
            .bind(id)
            .fetch_all(pool)
            .await?,
            crate::state::DbPool::Postgres(pool) => sqlx::query_as(
                "SELECT psr.resource_id, psr.skip_reason, psr.urls_invalid, psr.detail, er.title \
                 FROM push_skip_records psr \
                 LEFT JOIN extracted_resources er ON er.id = psr.resource_id \
                 WHERE psr.push_history_id = $1 ORDER BY psr.id ASC",
            )
            .bind(id)
            .fetch_all(pool)
            .await?,
        };
    let skip_records: Vec<Value> = rows
        .into_iter()
        .map(|(rid, reason, urls, detail, title)| {
            json!({
                "resource_id": rid,
                "title": title,
                "skip_reason": reason,
                "urls_invalid": urls,
                "detail": detail,
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "data": { "history": history, "skip_records": skip_records },
    })))
}

pub async fn retry_push(
    State(state): State<AppState>,
    Query(_params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    // Re-push failed records
    let count = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("UPDATE push_histories SET status = 'pending' WHERE status = 'failed'")
                .execute(pool)
                .await?
                .rows_affected()
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("UPDATE push_histories SET status = 'pending' WHERE status = 'failed'")
                .execute(pool)
                .await?
                .rows_affected()
        }
    };
    Ok(Json(
        json!({ "success": true, "message": format!("已标记 {} 条失败记录待重试", count) }),
    ))
}

pub async fn update_scheduler(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    // Save scheduler config to option_cache and database
    if let Some(obj) = body.as_object() {
        let mut cache = state.option_cache.write().await;
        for (key, value) in obj {
            let val_str = value.as_str().unwrap_or("").to_string();
            let cache_key = format!("push_{}", key);

            match &state.db {
                crate::state::DbPool::Sqlite(pool) => {
                    sqlx::query("INSERT OR REPLACE INTO options (key, value) VALUES (?, ?)")
                        .bind(&cache_key)
                        .bind(&val_str)
                        .execute(pool)
                        .await?;
                }
                crate::state::DbPool::Postgres(pool) => {
                    sqlx::query("INSERT INTO options (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = $2")
                        .bind(&cache_key).bind(&val_str)
                        .execute(pool).await?;
                }
            }
            cache.insert(cache_key, val_str);
        }
    }
    drop(body);

    // Read effective config values and apply to scheduler
    let cache = state.option_cache.read().await;
    let auto_push = cache.get("push_auto_push").cloned().unwrap_or_default();
    let interval: u64 = cache
        .get("push_interval")
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let api_url = cache.get("push_api_url").cloned().unwrap_or_default();
    let api_token = cache.get("push_api_token").cloned().unwrap_or_default();
    let target = cache.get("push_target").cloned().unwrap_or_default();
    let batch_size: i64 = cache
        .get("push_batch_size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    drop(cache);

    if auto_push == "1" || auto_push.eq_ignore_ascii_case("true") {
        crate::services::scheduler::update_scheduler(
            state.scheduler.clone(),
            interval,
            api_url,
            api_token,
            target,
            batch_size,
            state.db.clone(),
            state.option_cache.clone(),
        )
        .await;
    } else {
        crate::services::scheduler::stop_scheduler(state.scheduler.clone()).await;
    }

    let sched = state.scheduler.read().await;
    let next_run = if sched.running {
        let elapsed = sched
            .last_run_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        let remaining = sched.interval_minutes * 60;
        let next_secs = remaining.saturating_sub(elapsed);
        Some(chrono::Local::now() + chrono::Duration::seconds(next_secs as i64))
    } else {
        None
    };
    drop(sched);

    Ok(Json(json!({
        "success": true,
        "message": "调度配置已更新",
        "next_run_at": next_run.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
    })))
}

/// 推送配置校验 — 检查是否有已启用的推送配置含 api_url
pub async fn config_check(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let has_valid_config: bool = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM push_configs WHERE is_active = 1 AND api_url != ''",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            count > 0
        }
        crate::state::DbPool::Postgres(pool) => {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM push_configs WHERE is_active = TRUE AND api_url != ''",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            count > 0
        }
    };

    let mut missing = Vec::new();
    let mut hints = serde_json::Map::new();

    if !has_valid_config {
        missing.push("push_config");
        hints.insert(
            "push_config".to_string(),
            json!("请创建并启用至少一个推送配置"),
        );
    }

    let is_valid = missing.is_empty();

    Ok(Json(json!({
        "success": true,
        "data": {
            "is_valid": is_valid,
            "missing": missing,
            "hints": hints,
        }
    })))
}

// ─── 推送配置 CRUD ──────────────────────────────────────────────────────────

/// GET /api/push/configs — 获取推送配置列表
pub async fn list_push_configs(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let configs = crate::services::push_config::list_configs(&state.db).await?;
    Ok(Json(
        json!({ "success": true, "data": { "list": configs } }),
    ))
}

/// GET /api/push/configs/{id} — 获取推送配置详情（含关联的采集器 ID）
pub async fn get_push_config(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let config = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
    };

    let config = config.ok_or_else(|| AppError::NotFound("推送配置不存在".into()))?;

    // 获取关联的采集器 ID
    let collector_ids: Vec<i64> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let rows: Vec<(i64,)> = sqlx::query_as(
                "SELECT collector_id FROM push_config_collectors WHERE push_config_id = ?",
            )
            .bind(id)
            .fetch_all(pool)
            .await?;
            rows.into_iter().map(|r| r.0).collect()
        }
        crate::state::DbPool::Postgres(pool) => {
            let rows: Vec<(i64,)> = sqlx::query_as(
                "SELECT collector_id FROM push_config_collectors WHERE push_config_id = $1",
            )
            .bind(id)
            .fetch_all(pool)
            .await?;
            rows.into_iter().map(|r| r.0).collect()
        }
    };

    let mut config_val = serde_json::to_value(&config).unwrap_or_default();
    config_val["collector_ids"] = serde_json::json!(collector_ids);

    Ok(Json(json!({ "success": true, "data": config_val })))
}

/// POST /api/push/configs — 创建推送配置
pub async fn create_push_config(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let api_url = body.get("api_url").and_then(|v| v.as_str()).unwrap_or("");
    let api_token = body.get("api_token").and_then(|v| v.as_str());
    let target = body.get("target").and_then(|v| v.as_str()).unwrap_or("");
    let auth_type = body
        .get("auth_type")
        .and_then(|v| v.as_str())
        .unwrap_or("custom_header");
    let auth_key = body
        .get("auth_key")
        .and_then(|v| v.as_str())
        .unwrap_or("X-API-Token");
    let http_method = body
        .get("http_method")
        .and_then(|v| v.as_str())
        .unwrap_or("POST");
    let body_template = body.get("body_template").and_then(|v| v.as_str());
    let custom_headers = body
        .get("custom_headers")
        .and_then(|v| v.as_str())
        .unwrap_or("[]");
    let batch_size = body
        .get("batch_size")
        .and_then(|v| v.as_i64())
        .unwrap_or(1000);
    let data_source_type = body
        .get("data_source_type")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let collector_ids: Vec<i64> = body
        .get("collector_ids")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();
    let auto_push = body
        .get("auto_push")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let push_interval = body
        .get("push_interval")
        .and_then(|v| v.as_i64())
        .unwrap_or(30);
    let link_check_before_push = body
        .get("link_check_before_push")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let id = crate::services::push_config::create_config(
        &state.db,
        name,
        api_url,
        api_token,
        target,
        auth_type,
        auth_key,
        http_method,
        body_template,
        custom_headers,
        batch_size,
        data_source_type,
        &collector_ids,
        auto_push,
        push_interval,
        link_check_before_push,
    )
    .await?;

    Ok(Json(json!({ "success": true, "data": { "id": id } })))
}

/// PUT /api/push/configs/{id} — 更新推送配置
pub async fn update_push_config(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    crate::services::push_config::update_config(&state.db, id, &body).await?;
    Ok(Json(json!({ "success": true, "message": "配置已更新" })))
}

/// DELETE /api/push/configs/{id} — 删除推送配置
pub async fn delete_push_config(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    crate::services::push_config::delete_config(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "message": "配置已删除" })))
}

/// PUT /api/push/configs/{id}/toggle — 切换启用/禁用
pub async fn toggle_push_config(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    crate::services::push_config::toggle_config(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "message": "状态已切换" })))
}

/// POST /api/push/configs/{id}/duplicate — 复制推送配置
pub async fn duplicate_push_config(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let new_id = crate::services::push_config::duplicate_config(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "data": { "id": new_id } })))
}

/// POST /api/push/configs/{id}/trigger — 按配置手动推送
pub async fn trigger_push_for_config(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let batch_size = body.get("batch_size").and_then(|v| v.as_i64());
    match crate::services::push_config::push_for_config(
        &state.db,
        &state.option_cache,
        id,
        batch_size,
    )
    .await
    {
        Ok(result) => Ok(Json(json!({ "success": true, "data": result }))),
        Err(e) => Ok(Json(
            json!({ "success": false, "message": format!("推送失败: {e}") }),
        )),
    }
}

/// POST /api/push/configs/{id}/check-links — 按配置批量链接检测（FR-010 ch2，不推送）
pub async fn check_links_for_config(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let ignore_cache = body
        .get("ignore_cache")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match crate::services::push_config::check_links_for_config(
        &state.db,
        &state.option_cache,
        id,
        ignore_cache,
    )
    .await
    {
        Ok(result) => Ok(Json(json!({ "success": true, "data": result }))),
        Err(e) => Ok(Json(
            json!({ "success": false, "message": format!("检测失败: {e}") }),
        )),
    }
}
