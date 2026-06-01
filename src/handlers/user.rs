use axum::{extract::{Path, Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::errors::AppError;
use crate::state::AppState;
use crate::services::crypto;

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub keyword: Option<String>,
}

pub async fn list_users(State(_state): State<AppState>, Query(params): Query<PaginationParams>) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(10);
    Ok(Json(json!({ "success": true, "data": { "list": [], "pagination": { "page": page, "page_size": page_size, "total": 0 } } })))
}

pub async fn create_user(State(state): State<AppState>, Json(body): Json<serde_json::Value>) -> Result<Json<Value>, AppError> {
    let username = body.get("username").and_then(|v| v.as_str()).ok_or_else(|| AppError::BadRequest("用户名必填".into()))?;
    let password = body.get("password").and_then(|v| v.as_str()).ok_or_else(|| AppError::BadRequest("密码必填".into()))?;
    let hash = crypto::hash_password(password)?;
    let email = body.get("email").and_then(|v| v.as_str());
    let display_name = body.get("display_name").and_then(|v| v.as_str());
    let role = body.get("role").and_then(|v| v.as_i64()).unwrap_or(1) as i32;

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("INSERT INTO users (username, password, email, display_name, role, status) VALUES (?, ?, ?, ?, ?, 1)")
                .bind(username).bind(&hash).bind(email).bind(display_name).bind(role)
                .execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO users (username, password, email, display_name, role, status) VALUES ($1, $2, $3, $4, $5, 1)")
                .bind(username).bind(&hash).bind(email).bind(display_name).bind(role)
                .execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "用户已创建" })))
}

pub async fn get_user(State(_state): State<AppState>, Path(_id): Path<i64>) -> Result<Json<Value>, AppError> {
    Err(AppError::NotFound("用户不存在".into()))
}

pub async fn update_user(State(_state): State<AppState>, Path(_id): Path<i64>, Json(_body): Json<serde_json::Value>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "用户已更新" })))
}

pub async fn delete_user(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Value>, AppError> {
    if id == 1 {
        return Err(AppError::Forbidden("不能删除 root 用户".into()));
    }
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => { sqlx::query("DELETE FROM users WHERE id = ?").bind(id).execute(pool).await?; }
        crate::state::DbPool::Postgres(pool) => { sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(pool).await?; }
    }
    Ok(Json(json!({ "success": true, "message": "用户已删除" })))
}
