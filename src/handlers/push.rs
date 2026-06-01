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
    State(_state): State<AppState>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "推送已触发" })))
}

pub async fn get_stats(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let total: i64 = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM push_histories")
                .fetch_one(pool)
                .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM push_histories")
                .fetch_one(pool)
                .await?
        }
    };
    let success: i64 = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM push_histories WHERE status = 'success'")
                .fetch_one(pool)
                .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM push_histories WHERE status = 'success'")
                .fetch_one(pool)
                .await?
        }
    };
    let failed: i64 = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM push_histories WHERE status = 'failed'")
                .fetch_one(pool)
                .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM push_histories WHERE status = 'failed'")
                .fetch_one(pool)
                .await?
        }
    };
    Ok(Json(
        json!({ "success": true, "data": { "total": total, "success": success, "failed": failed } }),
    ))
}

pub async fn list_histories(
    State(_state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(10);
    Ok(Json(
        json!({ "success": true, "data": { "list": [], "pagination": { "page": page, "page_size": page_size, "total": 0 } } }),
    ))
}

pub async fn retry_push(
    State(_state): State<AppState>,
    Query(_params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "重试已触发" })))
}

pub async fn update_scheduler(
    State(_state): State<AppState>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(
        json!({ "success": true, "message": "调度配置已更新" }),
    ))
}
