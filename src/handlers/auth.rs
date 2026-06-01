use axum::{extract::State, Json};
use serde_json::{json, Value};
use crate::errors::AppError;
use crate::state::AppState;
use crate::models::user::{LoginRequest, CreateUserRequest, User, UserInfo};
use crate::services::crypto;

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<Value>, AppError> {
    let hash = crypto::hash_password(&req.password)?;

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let result = sqlx::query(
                "INSERT INTO users (username, password, email, display_name, role, status) VALUES (?, ?, ?, ?, 1, 1)"
            )
            .bind(&req.username)
            .bind(&hash)
            .bind(&req.email)
            .bind(&req.display_name)
            .execute(pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("UNIQUE") {
                    AppError::BadRequest("用户名已存在".into())
                } else {
                    AppError::Database(e)
                }
            })?;

            let user_id = result.last_insert_rowid();
            let token = crypto::generate_token(user_id, &req.username, 1, &state.config.session_secret)?;
            Ok(Json(json!({ "success": true, "data": { "token": token } })))
        }
        crate::state::DbPool::Postgres(pool) => {
            let row = sqlx::query_scalar::<_, i64>(
                "INSERT INTO users (username, password, email, display_name, role, status) VALUES ($1, $2, $3, $4, 1, 1) RETURNING id"
            )
            .bind(&req.username)
            .bind(&hash)
            .bind(&req.email)
            .bind(&req.display_name)
            .fetch_one(pool)
            .await?;

            let token = crypto::generate_token(row, &req.username, 1, &state.config.session_secret)?;
            Ok(Json(json!({ "success": true, "data": { "token": token } })))
        }
    }
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<Value>, AppError> {
    let user: User = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ? AND status = 1")
                .bind(&req.username)
                .fetch_optional(pool)
                .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1 AND status = 1")
                .bind(&req.username)
                .fetch_optional(pool)
                .await?
        }
    }
    .ok_or_else(|| AppError::Unauthorized("用户名或密码错误".into()))?;

    if !crypto::verify_password(&req.password, &user.password)? {
        return Err(AppError::Unauthorized("用户名或密码错误".into()));
    }

    let token = crypto::generate_token(user.id, &user.username, user.role, &state.config.session_secret)?;
    Ok(Json(json!({ "success": true, "data": { "token": token } })))
}

pub async fn logout() -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "已退出登录" })))
}

pub async fn get_me(
    State(_state): State<AppState>,
    axum::extract::Extension(user): axum::extract::Extension<User>,
) -> Result<Json<Value>, AppError> {
    let info: UserInfo = user.into();
    Ok(Json(json!({ "success": true, "data": info })))
}

pub async fn update_me(
    State(_state): State<AppState>,
    axum::extract::Extension(_user): axum::extract::Extension<User>,
    Json(_req): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    // TODO: implement update profile
    Ok(Json(json!({ "success": true, "message": "更新成功" })))
}

pub async fn generate_api_token(
    State(state): State<AppState>,
    axum::extract::Extension(user): axum::extract::Extension<User>,
) -> Result<Json<Value>, AppError> {
    let token = crypto::generate_api_token();
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("UPDATE users SET access_token = ? WHERE id = ?")
                .bind(&token)
                .bind(user.id)
                .execute(pool)
                .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("UPDATE users SET access_token = $1 WHERE id = $2")
                .bind(&token)
                .bind(user.id)
                .execute(pool)
                .await?;
        }
    }
    Ok(Json(json!({ "success": true, "data": { "token": token } })))
}
