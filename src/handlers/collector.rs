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
    pub channel_id: Option<i64>,
    pub keyword: Option<String>,
}

pub async fn list_collectors(
    State(state): State<AppState>,
    Query(_params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let collectors: Vec<crate::models::collector::Collector> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, user_id, channel_id, channel_name, collector_type, is_active, remark, created_at, updated_at FROM collectors ORDER BY id DESC")
                .fetch_all(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, user_id, channel_id, channel_name, collector_type, is_active, remark, created_at, updated_at FROM collectors ORDER BY id DESC")
                .fetch_all(pool).await?
        }
    };
    Ok(Json(json!({ "success": true, "data": { "list": collectors } })))
}

pub async fn create_collector(
    State(state): State<AppState>,
    axum::extract::Extension(user): axum::extract::Extension<crate::models::user::User>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let channel_id = body.get("channel_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let channel_name = body
        .get("channel_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let collector_type = body
        .get("collector_type")
        .and_then(|v| v.as_str())
        .unwrap_or("origin");
    let is_active = body
        .get("is_active")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let remark = body.get("remark").and_then(|v| v.as_str()).unwrap_or("");

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active, remark) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(user.id).bind(channel_id).bind(channel_name).bind(collector_type).bind(is_active).bind(remark)
                .execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active, remark) VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(user.id).bind(channel_id).bind(channel_name).bind(collector_type).bind(is_active).bind(remark)
                .execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "采集器已创建" })))
}

pub async fn get_collector(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let collector: Option<crate::models::collector::Collector> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, user_id, channel_id, channel_name, collector_type, is_active, remark, created_at, updated_at FROM collectors WHERE id = ?")
                .bind(id).fetch_optional(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, user_id, channel_id, channel_name, collector_type, is_active, remark, created_at, updated_at FROM collectors WHERE id = $1")
                .bind(id).fetch_optional(pool).await?
        }
    };
    match collector {
        Some(c) => Ok(Json(json!({ "success": true, "data": c }))),
        None => Err(AppError::NotFound("采集器不存在".into())),
    }
}

pub async fn update_collector(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let channel_id = body.get("channel_id").and_then(|v| v.as_i64());
    let channel_name = body.get("channel_name").and_then(|v| v.as_str());
    let collector_type = body.get("collector_type").and_then(|v| v.as_str());
    let is_active = body.get("is_active").and_then(|v| v.as_bool());
    let remark = body.get("remark").and_then(|v| v.as_str());

    // Build dynamic SET clause
    let mut sets = Vec::new();
    if channel_id.is_some() { sets.push("channel_id = ?"); }
    if channel_name.is_some() { sets.push("channel_name = ?"); }
    if collector_type.is_some() { sets.push("collector_type = ?"); }
    if is_active.is_some() { sets.push("is_active = ?"); }
    if remark.is_some() { sets.push("remark = ?"); }
    if sets.is_empty() {
        return Ok(Json(json!({ "success": true, "message": "采集器已更新" })));
    }

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let set_str: String = sets.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
                + ", updated_at = CURRENT_TIMESTAMP";
            let sql = format!("UPDATE collectors SET {set_str} WHERE id = ?");
            let mut q = sqlx::query(&sql);
            if let Some(v) = channel_id { q = q.bind(v); }
            if let Some(v) = channel_name { q = q.bind(v); }
            if let Some(v) = collector_type { q = q.bind(v); }
            if let Some(v) = is_active { q = q.bind(v); }
            if let Some(v) = remark { q = q.bind(v); }
            q = q.bind(id);
            let result = q.execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("采集器不存在".into()));
            }
        }
        crate::state::DbPool::Postgres(pool) => {
            // Rebuild with $N placeholders for Postgres
            let mut pg_parts = Vec::new();
            let mut pg_idx = 1u32;
            if channel_id.is_some() { pg_parts.push(format!("channel_id = ${pg_idx}")); pg_idx += 1; }
            if channel_name.is_some() { pg_parts.push(format!("channel_name = ${pg_idx}")); pg_idx += 1; }
            if collector_type.is_some() { pg_parts.push(format!("collector_type = ${pg_idx}")); pg_idx += 1; }
            if is_active.is_some() { pg_parts.push(format!("is_active = ${pg_idx}")); pg_idx += 1; }
            if remark.is_some() { pg_parts.push(format!("remark = ${pg_idx}")); pg_idx += 1; }
            pg_parts.push("updated_at = CURRENT_TIMESTAMP".to_string());
            let sql = format!("UPDATE collectors SET {} WHERE id = ${pg_idx}", pg_parts.join(", "));
            let mut q = sqlx::query(&sql);
            if let Some(v) = channel_id { q = q.bind(v); }
            if let Some(v) = channel_name { q = q.bind(v); }
            if let Some(v) = collector_type { q = q.bind(v); }
            if let Some(v) = is_active { q = q.bind(v); }
            if let Some(v) = remark { q = q.bind(v); }
            q = q.bind(id);
            let result = q.execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("采集器不存在".into()));
            }
        }
    }
    Ok(Json(json!({ "success": true, "message": "采集器已更新" })))
}

pub async fn delete_collector(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM collectors WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM collectors WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "采集器已删除" })))
}

pub async fn toggle_collector(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("UPDATE collectors SET is_active = NOT is_active, updated_at = CURRENT_TIMESTAMP WHERE id = ?").bind(id).execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("UPDATE collectors SET is_active = NOT is_active, updated_at = CURRENT_TIMESTAMP WHERE id = $1").bind(id).execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "状态已切换" })))
}

pub async fn fetch_history(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    // Look up collector to get channel_id and client info
    let collector: Option<crate::models::collector::Collector> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, user_id, channel_id, channel_name, collector_type, is_active, remark, created_at, updated_at FROM collectors WHERE id = ?")
                .bind(id).fetch_optional(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, user_id, channel_id, channel_name, collector_type, is_active, remark, created_at, updated_at FROM collectors WHERE id = $1")
                .bind(id).fetch_optional(pool).await?
        }
    };
    let collector = match collector {
        Some(c) => c,
        None => return Err(AppError::NotFound("采集器不存在".into())),
    };

    // Find an active client to use for fetching
    let clients = state.tg_clients.read().await;
    let client_id = clients.iter().find(|(_, e)| e.status == "active").map(|(id, _)| id.clone());
    drop(clients);

    let client_id = match client_id {
        Some(cid) => cid,
        None => return Err(AppError::BadRequest("没有可用的活跃客户端".into())),
    };

    // Trigger collection via service
    let count = crate::services::collector::full_collect(
        id,
        &client_id,
        collector.channel_id,
        &state.tg_clients,
        &state.db,
        &state.option_cache,
    )
    .await?;
    Ok(Json(json!({ "success": true, "data": { "message": format!("采集完成，新增 {} 条", count) } })))
}

pub async fn list_histories(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let (list, total): (Vec<crate::models::collector_history::CollectorHistory>, i64) = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collector_histories")
                .fetch_one(pool).await?;
            let list = sqlx::query_as(
                "SELECT id, collector_id, channel_id, message_id, post_time, raw_data, is_auto_push, remote_id, created_at FROM collector_histories ORDER BY id DESC LIMIT ? OFFSET ?"
            ).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
        crate::state::DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collector_histories")
                .fetch_one(pool).await?;
            let list = sqlx::query_as(
                "SELECT id, collector_id, channel_id, message_id, post_time, raw_data, is_auto_push, remote_id, created_at FROM collector_histories ORDER BY id DESC LIMIT $1 OFFSET $2"
            ).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
    };
    Ok(Json(json!({ "success": true, "data": { "list": list, "pagination": { "page": page, "page_size": page_size, "total": total } } })))
}
