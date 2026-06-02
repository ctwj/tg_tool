use crate::errors::AppError;
use crate::models::user::{CreateUserRequest, LoginRequest, User, UserInfo};
use crate::services::crypto;
use crate::state::AppState;
use axum::{Json, extract::State};
use serde_json::{Value, json};

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
            let token =
                crypto::generate_token(user_id, &req.username, 1, &state.config.session_secret)?;
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

            let token =
                crypto::generate_token(row, &req.username, 1, &state.config.session_secret)?;
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

    let token = crypto::generate_token(
        user.id,
        &user.username,
        user.role,
        &state.config.session_secret,
    )?;
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
    State(state): State<AppState>,
    axum::extract::Extension(user): axum::extract::Extension<User>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let display_name = body.get("display_name").and_then(|v| v.as_str());
    let email = body.get("email").and_then(|v| v.as_str());
    let password = body.get("password").and_then(|v| v.as_str());

    // Build dynamic SET clause
    let has_update = display_name.is_some() || email.is_some() || password.is_some();
    if !has_update {
        return Ok(Json(json!({ "success": true, "message": "更新成功" })));
    }

    let hashed = match password {
        Some(pw) => Some(crypto::hash_password(pw)?),
        None => None,
    };

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let mut sets = Vec::new();
            if display_name.is_some() { sets.push("display_name = ?"); }
            if email.is_some() { sets.push("email = ?"); }
            if hashed.is_some() { sets.push("password = ?"); }
            sets.push("updated_at = CURRENT_TIMESTAMP");
            let sql = format!("UPDATE users SET {} WHERE id = ?", sets.join(", "));
            let mut q = sqlx::query(&sql);
            if let Some(v) = display_name { q = q.bind(v); }
            if let Some(v) = email { q = q.bind(v); }
            if let Some(v) = &hashed { q = q.bind(v); }
            q = q.bind(user.id);
            q.execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            let mut pg_parts = Vec::new();
            let mut pg_idx = 1u32;
            if display_name.is_some() { pg_parts.push(format!("display_name = ${pg_idx}")); pg_idx += 1; }
            if email.is_some() { pg_parts.push(format!("email = ${pg_idx}")); pg_idx += 1; }
            if hashed.is_some() { pg_parts.push(format!("password = ${pg_idx}")); pg_idx += 1; }
            pg_parts.push("updated_at = CURRENT_TIMESTAMP".to_string());
            let sql = format!("UPDATE users SET {} WHERE id = ${pg_idx}", pg_parts.join(", "));
            let mut q = sqlx::query(&sql);
            if let Some(v) = display_name { q = q.bind(v); }
            if let Some(v) = email { q = q.bind(v); }
            if let Some(v) = &hashed { q = q.bind(v); }
            q = q.bind(user.id);
            q.execute(pool).await?;
        }
    }
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
