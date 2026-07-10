// 转存任务 handlers（feature 047 US2）— 手动触发（异步执行）+ 查询
// 路由前缀 /api/pan/transfers（routes.rs admin_routes）

use crate::errors::AppError;
use crate::models::transfer_task::{CreateTransferTask, SOURCE_ORIGIN_MANUAL};
use crate::services::transfer;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

/// POST /api/pan/transfers — 提交转存/上传任务（幂等创建 + 异步执行）
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateTransferTask>,
) -> Result<Json<Value>, AppError> {
    let task = transfer::create_task(&state.db, SOURCE_ORIGIN_MANUAL, req).await?;
    // 异步执行（首期简单 spawn；FR-019 全局并发/限流后续完善）
    let db = state.db.clone();
    let key = state.config.pan_cred_key.clone();
    let staging_dir = state.config.pan_staging_dir.clone();
    let option_cache = state.option_cache.clone();
    let id = task.id;
    tokio::spawn(async move {
        if let Err(e) = transfer::run_task(&db, &key, &staging_dir, &option_cache, id).await {
            tracing::error!("转存任务 {id} 异步执行错误: {e}");
        }
    });
    Ok(Json(json!({ "success": true, "data": task })))
}

/// GET /api/pan/transfers/{id} — 查询任务状态/结果（成功时附带分享链接）
pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let task = transfer::get_task(&state.db, id).await?;
    let mut data = serde_json::to_value(&task)
        .map_err(|e| AppError::Internal(format!("序列化失败: {e}")))?;
    if let Some(sid) = task.share_id {
        if let Ok(s) = crate::services::share::get(&state.db, sid).await {
            data["share_url"] = json!(s.share_url);
            data["share_extract_code"] = json!(s.extract_code);
        }
    }
    Ok(Json(json!({ "success": true, "data": data })))
}

#[derive(Deserialize)]
pub struct TransferQuery {
    pub status: Option<String>,
    pub account_id: Option<i64>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// GET /api/pan/transfers — 任务历史列表（分页 + status/account 筛选）
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<TransferQuery>,
) -> Result<Json<Value>, AppError> {
    let page = q.page.unwrap_or(1);
    let page_size = q.page_size.unwrap_or(20);
    let (items, total) = transfer::list_tasks(
        &state.db,
        q.status.as_deref(),
        q.account_id,
        page,
        page_size,
    )
    .await?;
    Ok(Json(json!({
        "success": true,
        "data": { "items": items, "total": total, "page": page, "page_size": page_size }
    })))
}

/// POST /api/pan/transfers/{id}/retry — 重试 failed 任务
pub async fn retry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let task = transfer::retry_task(&state.db, id).await?;
    let db = state.db.clone();
    let key = state.config.pan_cred_key.clone();
    let staging = state.config.pan_staging_dir.clone();
    let option_cache = state.option_cache.clone();
    let tid = task.id;
    tokio::spawn(async move {
        if let Err(e) = transfer::run_task(&db, &key, &staging, &option_cache, tid).await {
            tracing::error!("重试任务 {tid} 执行错误: {e}");
        }
    });
    Ok(Json(json!({ "success": true, "data": task })))
}

/// POST /api/pan/transfers/cleanup — 手动清理过期任务（保留期从 option_cache 读，默认 90 天）
pub async fn cleanup(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let days = {
        let cache = state.option_cache.read().await;
        cache
            .get("pan_task_retention_days")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(90)
            .max(1)
    };
    let deleted = transfer::cleanup_expired(&state.db, days).await?;
    Ok(Json(json!({ "success": true, "data": { "deleted": deleted, "retention_days": days } })))
}
