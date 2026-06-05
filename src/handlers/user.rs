use crate::errors::AppError;
use crate::services::crypto;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub keyword: Option<String>,
}

pub async fn list_users(
    State(state): State<AppState>,
    Query(_params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let users: Vec<crate::models::user::User> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, username, password, display_name, email, role, status, access_token, created_at, updated_at FROM users ORDER BY id DESC")
                .fetch_all(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, username, password, display_name, email, role, status, access_token, created_at, updated_at FROM users ORDER BY id DESC")
                .fetch_all(pool).await?
        }
    };
    let infos: Vec<crate::models::user::UserInfo> = users.into_iter().map(|u| u.into()).collect();
    Ok(Json(json!({ "success": true, "data": { "list": infos } })))
}

pub async fn create_user(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let username = body
        .get("username")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("用户名必填".into()))?;
    let password = body
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("密码必填".into()))?;
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

pub async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let user: Option<crate::models::user::User> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, username, password, display_name, email, role, status, access_token, created_at, updated_at FROM users WHERE id = ?")
                .bind(id).fetch_optional(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, username, password, display_name, email, role, status, access_token, created_at, updated_at FROM users WHERE id = $1")
                .bind(id).fetch_optional(pool).await?
        }
    };
    match user {
        Some(u) => {
            let info: crate::models::user::UserInfo = u.into();
            Ok(Json(json!({ "success": true, "data": info })))
        }
        None => Err(AppError::NotFound("用户不存在".into())),
    }
}

pub async fn update_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let display_name = body.get("display_name").and_then(|v| v.as_str());
    let email = body.get("email").and_then(|v| v.as_str());
    let password = body.get("password").and_then(|v| v.as_str());
    let role = body.get("role").and_then(|v| v.as_i64());
    let status = body.get("status").and_then(|v| v.as_i64());

    // Build dynamic SET clause
    let mut sets = Vec::new();
    if display_name.is_some() {
        sets.push("display_name = ?");
    }
    if email.is_some() {
        sets.push("email = ?");
    }
    if password.is_some() {
        sets.push("password = ?");
    }
    if role.is_some() {
        sets.push("role = ?");
    }
    if status.is_some() {
        sets.push("status = ?");
    }
    if sets.is_empty() {
        return Ok(Json(json!({ "success": true, "message": "用户已更新" })));
    }

    // Hash password if provided
    let hashed = match password {
        Some(pw) => Some(crypto::hash_password(pw)?),
        None => None,
    };

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let set_str = sets
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
                + ", updated_at = CURRENT_TIMESTAMP";
            let sql = format!("UPDATE users SET {set_str} WHERE id = ?");
            let mut q = sqlx::query(&sql);
            if let Some(v) = display_name {
                q = q.bind(v);
            }
            if let Some(v) = email {
                q = q.bind(v);
            }
            if let Some(v) = &hashed {
                q = q.bind(v);
            }
            if let Some(v) = role {
                q = q.bind(v as i32);
            }
            if let Some(v) = status {
                q = q.bind(v as i32);
            }
            q = q.bind(id);
            let result = q.execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("用户不存在".into()));
            }
        }
        crate::state::DbPool::Postgres(pool) => {
            let mut pg_parts = Vec::new();
            let mut pg_idx = 1u32;
            if display_name.is_some() {
                pg_parts.push(format!("display_name = ${pg_idx}"));
                pg_idx += 1;
            }
            if email.is_some() {
                pg_parts.push(format!("email = ${pg_idx}"));
                pg_idx += 1;
            }
            if hashed.is_some() {
                pg_parts.push(format!("password = ${pg_idx}"));
                pg_idx += 1;
            }
            if role.is_some() {
                pg_parts.push(format!("role = ${pg_idx}"));
                pg_idx += 1;
            }
            if status.is_some() {
                pg_parts.push(format!("status = ${pg_idx}"));
                pg_idx += 1;
            }
            pg_parts.push("updated_at = CURRENT_TIMESTAMP".to_string());
            let sql = format!(
                "UPDATE users SET {} WHERE id = ${pg_idx}",
                pg_parts.join(", ")
            );
            let mut q = sqlx::query(&sql);
            if let Some(v) = display_name {
                q = q.bind(v);
            }
            if let Some(v) = email {
                q = q.bind(v);
            }
            if let Some(v) = &hashed {
                q = q.bind(v);
            }
            if let Some(v) = role {
                q = q.bind(v as i32);
            }
            if let Some(v) = status {
                q = q.bind(v as i32);
            }
            q = q.bind(id);
            let result = q.execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("用户不存在".into()));
            }
        }
    }
    Ok(Json(json!({ "success": true, "message": "用户已更新" })))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    if id == 1 {
        return Err(AppError::Forbidden("不能删除 root 用户".into()));
    }
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM users WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "用户已删除" })))
}
