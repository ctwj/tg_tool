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
    pub collector_id: Option<i64>,
    pub keyword: Option<String>,
    pub is_extracted: Option<bool>,
}

const SELECT_COLLECTOR: &str = "SELECT id, user_id, client_id, channel_id, channel_name, collector_type, is_active, remark, created_at, updated_at FROM collectors";

pub async fn list_collectors(
    State(state): State<AppState>,
    Query(_params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let collectors: Vec<crate::models::collector::Collector> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as(&format!("{SELECT_COLLECTOR} ORDER BY id DESC"))
                .fetch_all(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(&format!("{SELECT_COLLECTOR} ORDER BY id DESC"))
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
    let client_id = body.get("client_id").and_then(|v| v.as_str()).unwrap_or("");
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

    if client_id.is_empty() {
        return Err(AppError::BadRequest("请选择客户端".into()));
    }
    if channel_id == 0 {
        return Err(AppError::BadRequest("请选择频道/群组".into()));
    }

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("INSERT INTO collectors (user_id, client_id, channel_id, channel_name, collector_type, is_active, remark) VALUES (?, ?, ?, ?, ?, ?, ?)")
                .bind(user.id).bind(client_id).bind(channel_id).bind(channel_name).bind(collector_type).bind(is_active).bind(remark)
                .execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO collectors (user_id, client_id, channel_id, channel_name, collector_type, is_active, remark) VALUES ($1, $2, $3, $4, $5, $6, $7)")
                .bind(user.id).bind(client_id).bind(channel_id).bind(channel_name).bind(collector_type).bind(is_active).bind(remark)
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
            sqlx::query_as(&format!("{SELECT_COLLECTOR} WHERE id = ?"))
                .bind(id).fetch_optional(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(&format!("{SELECT_COLLECTOR} WHERE id = $1"))
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
    let client_id = body.get("client_id").and_then(|v| v.as_str());
    let channel_id = body.get("channel_id").and_then(|v| v.as_i64());
    let channel_name = body.get("channel_name").and_then(|v| v.as_str());
    let collector_type = body.get("collector_type").and_then(|v| v.as_str());
    let is_active = body.get("is_active").and_then(|v| v.as_bool());
    let remark = body.get("remark").and_then(|v| v.as_str());

    // Build dynamic SET clause
    let mut sets = Vec::new();
    if client_id.is_some() { sets.push("client_id = ?"); }
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
            if let Some(v) = client_id { q = q.bind(v); }
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
            if client_id.is_some() { pg_parts.push(format!("client_id = ${pg_idx}")); pg_idx += 1; }
            if channel_id.is_some() { pg_parts.push(format!("channel_id = ${pg_idx}")); pg_idx += 1; }
            if channel_name.is_some() { pg_parts.push(format!("channel_name = ${pg_idx}")); pg_idx += 1; }
            if collector_type.is_some() { pg_parts.push(format!("collector_type = ${pg_idx}")); pg_idx += 1; }
            if is_active.is_some() { pg_parts.push(format!("is_active = ${pg_idx}")); pg_idx += 1; }
            if remark.is_some() { pg_parts.push(format!("remark = ${pg_idx}")); pg_idx += 1; }
            pg_parts.push("updated_at = CURRENT_TIMESTAMP".to_string());
            let sql = format!("UPDATE collectors SET {} WHERE id = ${pg_idx}", pg_parts.join(", "));
            let mut q = sqlx::query(&sql);
            if let Some(v) = client_id { q = q.bind(v); }
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
    body: Option<Json<serde_json::Value>>,
) -> Result<Json<Value>, AppError> {
    // Read limit from optional request body
    let limit: i64 = body
        .and_then(|Json(v)| v.get("limit").and_then(|v| v.as_i64()))
        .unwrap_or(1000)
        .clamp(1, 10000);
    // Look up collector to get channel_id and client info
    let collector: Option<crate::models::collector::Collector> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as(&format!("{SELECT_COLLECTOR} WHERE id = ?"))
                .bind(id).fetch_optional(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(&format!("{SELECT_COLLECTOR} WHERE id = $1"))
                .bind(id).fetch_optional(pool).await?
        }
    };
    let collector = match collector {
        Some(c) => c,
        None => return Err(AppError::NotFound("采集器不存在".into())),
    };

    // Use the collector's client_id, or fall back to any active client
    let client_id = if let Some(ref cid) = collector.client_id {
        // Verify the client is active
        let clients = state.tg_clients.read().await;
        let entry = clients.get(cid);
        let is_active = entry.map(|e| e.status == "active").unwrap_or(false);
        drop(clients);
        if is_active {
            cid.clone()
        } else {
            // Fall back to any active client
            let clients = state.tg_clients.read().await;
            let fallback = clients.iter().find(|(_, e)| e.status == "active").map(|(id, _)| id.clone());
            drop(clients);
            fallback.ok_or_else(|| AppError::BadRequest("没有可用的活跃客户端".into()))?
        }
    } else {
        // Legacy: find any active client
        let clients = state.tg_clients.read().await;
        let fallback = clients.iter().find(|(_, e)| e.status == "active").map(|(id, _)| id.clone());
        drop(clients);
        fallback.ok_or_else(|| AppError::BadRequest("没有可用的活跃客户端".into()))?
    };

    // Trigger collection via service
    let count = crate::services::collector::full_collect(
        id,
        &client_id,
        collector.channel_id,
        limit,
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

    // Build is_extracted WHERE clause
    let ext_sql = match params.is_extracted {
        Some(v) => format!(" AND is_extracted = {}", if v { 1 } else { 0 }),
        None => String::new(),
    };

    let (list, total): (Vec<crate::models::collector_history::CollectorHistory>, i64) = match (&state.db, &params.collector_id) {
        (crate::state::DbPool::Sqlite(pool), Some(cid)) => {
            let total: i64 = sqlx::query_scalar(
                &format!("SELECT COUNT(*) FROM collector_histories WHERE collector_id = ?{ext_sql}")
            ).bind(cid).fetch_one(pool).await?;
            let list = sqlx::query_as(
                &format!("SELECT id, collector_id, channel_id, message_id, post_time, raw_data, is_auto_push, remote_id, created_at, is_extracted FROM collector_histories WHERE collector_id = ?{ext_sql} ORDER BY id DESC LIMIT ? OFFSET ?")
            ).bind(cid).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
        (crate::state::DbPool::Postgres(pool), Some(cid)) => {
            let ext_sql_pg = match params.is_extracted {
                Some(v) => format!(" AND is_extracted = {}", v),
                None => String::new(),
            };
            let total: i64 = sqlx::query_scalar(
                &format!("SELECT COUNT(*) FROM collector_histories WHERE collector_id = $1{ext_sql_pg}")
            ).bind(cid).fetch_one(pool).await?;
            let list = sqlx::query_as(
                &format!("SELECT id, collector_id, channel_id, message_id, post_time, raw_data, is_auto_push, remote_id, created_at, is_extracted FROM collector_histories WHERE collector_id = $1{ext_sql_pg} ORDER BY id DESC LIMIT $2 OFFSET $3")
            ).bind(cid).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
        (crate::state::DbPool::Sqlite(pool), None) => {
            let where_clause = if ext_sql.is_empty() { String::new() } else { format!(" WHERE 1=1{ext_sql}") };
            let total: i64 = sqlx::query_scalar(
                &format!("SELECT COUNT(*) FROM collector_histories{where_clause}")
            ).fetch_one(pool).await?;
            let list = sqlx::query_as(
                &format!("SELECT id, collector_id, channel_id, message_id, post_time, raw_data, is_auto_push, remote_id, created_at, is_extracted FROM collector_histories{where_clause} ORDER BY id DESC LIMIT ? OFFSET ?")
            ).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
        (crate::state::DbPool::Postgres(pool), None) => {
            let ext_sql_pg = match params.is_extracted {
                Some(v) => format!(" WHERE is_extracted = {}", v),
                None => String::new(),
            };
            let total: i64 = sqlx::query_scalar(
                &format!("SELECT COUNT(*) FROM collector_histories{ext_sql_pg}")
            ).fetch_one(pool).await?;
            let list = sqlx::query_as(
                &format!("SELECT id, collector_id, channel_id, message_id, post_time, raw_data, is_auto_push, remote_id, created_at, is_extracted FROM collector_histories{ext_sql_pg} ORDER BY id DESC LIMIT $1 OFFSET $2")
            ).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
    };
    Ok(Json(json!({ "success": true, "data": { "list": list, "pagination": { "page": page, "page_size": page_size, "total": total } } })))
}
