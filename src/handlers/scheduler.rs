// 调度可视化相关 handler — 提取历史查询

use crate::errors::AppError;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct ExtractHistoryParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// GET /api/extract-histories — 提取历史列表（分页）
pub async fn list_extract_histories(
    State(state): State<AppState>,
    Query(params): Query<ExtractHistoryParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let result = crate::services::extract_history::list(&state.db, page, page_size).await?;
    Ok(Json(json!({ "success": true, "data": result })))
}

/// GET /api/extract-histories/stats — 提取历史统计
pub async fn get_extract_histories_stats(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let stats = crate::services::extract_history::stats(&state.db).await?;
    Ok(Json(json!({ "success": true, "data": stats })))
}
