use axum::{extract::{Path, Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::errors::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub channel_id: Option<i64>,
    pub keyword: Option<String>,
}

pub async fn list_collectors(State(_state): State<AppState>, Query(params): Query<PaginationParams>) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(10);
    Ok(Json(json!({ "success": true, "data": { "list": [], "pagination": { "page": page, "page_size": page_size, "total": 0 } } })))
}

pub async fn create_collector(State(state): State<AppState>, Json(body): Json<serde_json::Value>) -> Result<Json<Value>, AppError> {
    let channel_id = body.get("channel_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let channel_name = body.get("channel_name").and_then(|v| v.as_str()).unwrap_or("");
    let collector_type = body.get("collector_type").and_then(|v| v.as_str()).unwrap_or("origin");
    let is_active = body.get("is_active").and_then(|v| v.as_bool()).unwrap_or(true);
    let remark = body.get("remark").and_then(|v| v.as_str()).unwrap_or("");

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active, remark) VALUES (1, ?, ?, ?, ?, ?)")
                .bind(channel_id).bind(channel_name).bind(collector_type).bind(is_active).bind(remark)
                .execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active, remark) VALUES (1, $1, $2, $3, $4, $5)")
                .bind(channel_id).bind(channel_name).bind(collector_type).bind(is_active).bind(remark)
                .execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "采集器已创建" })))
}

pub async fn get_collector(State(_state): State<AppState>, Path(_id): Path<i64>) -> Result<Json<Value>, AppError> {
    Err(AppError::NotFound("采集器不存在".into()))
}

pub async fn update_collector(State(_state): State<AppState>, Path(_id): Path<i64>, Json(_body): Json<serde_json::Value>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "采集器已更新" })))
}

pub async fn delete_collector(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Value>, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => { sqlx::query("DELETE FROM collectors WHERE id = ?").bind(id).execute(pool).await?; }
        crate::state::DbPool::Postgres(pool) => { sqlx::query("DELETE FROM collectors WHERE id = $1").bind(id).execute(pool).await?; }
    }
    Ok(Json(json!({ "success": true, "message": "采集器已删除" })))
}

pub async fn toggle_collector(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Value>, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => { sqlx::query("UPDATE collectors SET is_active = NOT is_active, updated_at = CURRENT_TIMESTAMP WHERE id = ?").bind(id).execute(pool).await?; }
        crate::state::DbPool::Postgres(pool) => { sqlx::query("UPDATE collectors SET is_active = NOT is_active, updated_at = CURRENT_TIMESTAMP WHERE id = $1").bind(id).execute(pool).await?; }
    }
    Ok(Json(json!({ "success": true, "message": "状态已切换" })))
}

pub async fn fetch_history(State(_state): State<AppState>, Path(_id): Path<i64>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "data": { "message": "采集已开始" } })))
}

pub async fn list_histories(State(_state): State<AppState>, Query(params): Query<PaginationParams>) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(10);
    Ok(Json(json!({ "success": true, "data": { "list": [], "pagination": { "page": page, "page_size": page_size, "total": 0 } } })))
}
