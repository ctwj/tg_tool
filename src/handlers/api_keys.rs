// API Key 后台管理（feature 047 US4）— admin 端点 /api/pan/api-keys

use crate::errors::AppError;
use crate::models::api_key::CreateApiKey;
use crate::services::api_key;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

/// GET /api/pan/api-keys — Key 列表（脱敏）
pub async fn list(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let keys = api_key::list(&state.db).await?;
    Ok(Json(json!({ "success": true, "data": keys })))
}

/// POST /api/pan/api-keys — 签发新 Key（明文仅此一次返回）
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateApiKey>,
) -> Result<Json<Value>, AppError> {
    let (view, plaintext) = api_key::create(&state.db, req).await?;
    Ok(Json(json!({
        "success": true,
        "data": { "api_key": view, "plaintext": plaintext },
        "message": "明文 Key 仅此一次显示，请立即妥善保存"
    })))
}

/// POST /api/pan/api-keys/{id}/revoke — 吊销
pub async fn revoke(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let view = api_key::revoke(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "data": view })))
}

/// POST /api/pan/api-keys/{id}/rotate — 轮换（旧 Key 失效，返回新明文）
pub async fn rotate(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let (view, plaintext) = api_key::rotate(&state.db, id).await?;
    Ok(Json(json!({
        "success": true,
        "data": { "api_key": view, "plaintext": plaintext },
        "message": "新明文 Key 仅此一次显示，请立即妥善保存"
    })))
}
