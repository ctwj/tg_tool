use axum::{extract::{Path, Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::errors::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

pub async fn list_files(State(_state): State<AppState>, Query(params): Query<PaginationParams>) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(10);
    Ok(Json(json!({ "success": true, "data": { "list": [], "pagination": { "page": page, "page_size": page_size, "total": 0 } } })))
}

pub async fn upload_file(State(_state): State<AppState>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "文件上传功能待实现" })))
}

pub async fn delete_file(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Value>, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => { sqlx::query("DELETE FROM files WHERE id = ?").bind(id).execute(pool).await?; }
        crate::state::DbPool::Postgres(pool) => { sqlx::query("DELETE FROM files WHERE id = $1").bind(id).execute(pool).await?; }
    }
    Ok(Json(json!({ "success": true, "message": "文件已删除" })))
}

pub async fn download_file(Path(_filename): Path<String>) -> Result<axum::response::Response, AppError> {
    Err(AppError::NotFound("文件不存在".into()))
}
