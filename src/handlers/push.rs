use crate::errors::AppError;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub batch_size: Option<i64>,
}

pub async fn trigger_push(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    // 配置校验 — 检查必要配置项
    let cache = state.option_cache.read().await;
    let api_url = cache.get("push_api_url").cloned().unwrap_or_default();
    let api_token = cache.get("push_api_token").cloned().unwrap_or_default();
    let target = cache.get("push_target").cloned().unwrap_or_default();
    let auth_type = cache
        .get("push_auth_type")
        .cloned()
        .unwrap_or_else(|| "custom_header".to_string());
    drop(cache);

    let mut missing = Vec::new();
    if api_url.is_empty() {
        missing.push("push_api_url");
    }
    // 仅在认证方式非 "none" 时要求 api_token
    if auth_type != "none" && api_token.is_empty() {
        missing.push("push_api_token");
    }

    if !missing.is_empty() {
        return Ok(Json(json!({
            "success": false,
            "message": "推送配置不完整",
            "data": { "missing": missing }
        })));
    }

    let batch_size: i64 = body
        .get("batch_size")
        .and_then(|v| v.as_i64())
        .unwrap_or(1000);

    match crate::services::push::trigger_push(
        &api_url,
        &api_token,
        &target,
        batch_size,
        &state.db,
        &state.option_cache,
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
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_histories")
                .fetch_one(pool)
                .await?;
            let list = sqlx::query_as(
                "SELECT id, batch_id, target, status, data_count, message, error_msg, pushed_at FROM push_histories ORDER BY id DESC LIMIT ? OFFSET ?"
            ).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
        crate::state::DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_histories")
                .fetch_one(pool)
                .await?;
            let list = sqlx::query_as(
                "SELECT id, batch_id, target, status, data_count, message, error_msg, pushed_at FROM push_histories ORDER BY id DESC LIMIT $1 OFFSET $2"
            ).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
    };
    Ok(Json(
        json!({ "success": true, "data": { "list": list, "pagination": { "page": page, "page_size": page_size, "total": total } } }),
    ))
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

/// 推送配置校验 — 检查必要配置是否完整（含通用推送配置）
pub async fn config_check(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let cache = state.option_cache.read().await;

    let mut missing = Vec::new();
    let mut hints = serde_json::Map::new();

    // 基本必填项
    let checks = [("push_api_url", "请配置推送 API 地址")];

    for (key, hint) in &checks {
        let val = cache.get(*key).cloned().unwrap_or_default();
        if val.is_empty() {
            missing.push(*key);
            hints.insert(key.to_string(), json!(hint));
        }
    }

    // 认证相关校验
    let auth_type = cache
        .get("push_auth_type")
        .cloned()
        .unwrap_or_else(|| "custom_header".to_string());
    if auth_type != "none" {
        let api_token = cache.get("push_api_token").cloned().unwrap_or_default();
        if api_token.is_empty() {
            missing.push("push_api_token");
            hints.insert("push_api_token".to_string(), json!("请配置认证凭证"));
        }
    }
    if auth_type == "custom_header" || auth_type == "query" {
        let auth_key = cache.get("push_auth_key").cloned().unwrap_or_default();
        if auth_key.is_empty() {
            missing.push("push_auth_key");
            hints.insert("push_auth_key".to_string(), json!("请配置认证字段 Key"));
        }
    }

    drop(cache);

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
