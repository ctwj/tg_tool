// 网盘账号管理 handlers（feature 047 US1）— admin 端点，复用 auth_guard/admin_guard
// 路由前缀 /api/pan/accounts（routes.rs admin_routes）

use crate::errors::AppError;
use crate::models::pan_account::{CreatePanAccount, UpdatePanAccount};
use crate::services::pan_account;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

/// GET /api/pan/accounts — 账号列表（脱敏）
pub async fn list(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let accounts = pan_account::list_accounts(&state.db).await?;
    Ok(Json(json!({ "success": true, "data": accounts })))
}

/// POST /api/pan/accounts — 新增账号（凭据加密落库 + 创建即健康校验）
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreatePanAccount>,
) -> Result<Json<Value>, AppError> {
    let acc = pan_account::create_account(&state.db, &state.config.pan_cred_key, req).await?;
    Ok(Json(json!({ "success": true, "data": acc })))
}

/// GET /api/pan/accounts/{id} — 账号详情（脱敏）
pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let acc = pan_account::get_account_view(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "data": acc })))
}

/// PUT /api/pan/accounts/{id} — 更新账号（含凭据则重新加密 + 重新校验）
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdatePanAccount>,
) -> Result<Json<Value>, AppError> {
    let acc = pan_account::update_account(&state.db, &state.config.pan_cred_key, id, req).await?;
    Ok(Json(json!({ "success": true, "data": acc })))
}

/// DELETE /api/pan/accounts/{id} — 删除账号（凭据随之移除）
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    pan_account::delete_account(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "message": format!("账号 {id} 已删除") })))
}

/// POST /api/pan/accounts/{id}/check — 手动健康校验（回写 status/capacity/last_checked_at）
pub async fn check(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let acc = pan_account::check_account(&state.db, &state.config.pan_cred_key, id).await?;
    Ok(Json(json!({ "success": true, "data": acc })))
}
