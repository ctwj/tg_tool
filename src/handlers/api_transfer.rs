// 开放转存 API（feature 047 US4）— /api/v1/*，X-API-Key 鉴权
// 复用 transfer service（与后台手动触发同一编排内核）

use crate::errors::AppError;
use crate::models::transfer_task::{CreateTransferTask, SOURCE_ORIGIN_API};
use crate::services::{pan_account, transfer};
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

/// POST /api/v1/transfer/tasks — 提交转存任务（异步执行）
pub async fn create_task(
    State(state): State<AppState>,
    Json(req): Json<CreateTransferTask>,
) -> Result<Json<Value>, AppError> {
    let task = transfer::create_task(&state.db, SOURCE_ORIGIN_API, req).await?;
    let db = state.db.clone();
    let key = state.config.pan_cred_key.clone();
    let staging = state.config.pan_staging_dir.clone();
    let option_cache = state.option_cache.clone();
    let id = task.id;
    tokio::spawn(async move {
        if let Err(e) = transfer::run_task(&db, &key, &staging, &option_cache, id).await {
            tracing::error!("开放 API 转存任务 {id} 执行错误: {e}");
        }
    });
    Ok(Json(json!({ "success": true, "data": task })))
}

/// GET /api/v1/transfer/tasks/{id} — 查询任务结果
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let task = transfer::get_task(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "data": task })))
}

/// GET /api/v1/accounts — 列出可用目标账号（供调用方选 target_account_id）
pub async fn list_accounts(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let accounts = pan_account::list_accounts(&state.db).await?;
    let active: Vec<_> = accounts
        .into_iter()
        .filter(|a| a.status == "active")
        .collect();
    Ok(Json(json!({ "success": true, "data": active })))
}
