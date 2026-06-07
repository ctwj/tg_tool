// 图片转发队列管理 handler

use crate::errors::AppError;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    response::Json,
};
use serde_json::{Value, json};

pub async fn queue_status(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let status = crate::services::forward_queue::queue_status(&state.db).await?;
    Ok(Json(json!({ "success": true, "data": status })))
}

pub async fn retry_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    crate::services::forward_queue::retry_task(&state.db, id).await?;
    Ok(Json(
        json!({ "success": true, "message": "任务已重置为待转发" }),
    ))
}

pub async fn retry_all(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let count = crate::services::forward_queue::retry_all_failed(&state.db).await?;
    Ok(Json(json!({ "success": true, "retried": count })))
}
