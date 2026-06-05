use crate::errors::AppError;
use crate::models::user::{CreateUserRequest, LoginRequest, User, UserInfo};
use crate::services::crypto;
use crate::state::AppState;
use axum::{Json, extract::State, http::HeaderMap};
use serde_json::{Value, json};
use std::time::Instant;

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<Value>, AppError> {
    // Check if registration is allowed
    {
        let cache = state.option_cache.read().await;
        let allowed = cache
            .get("allow_register")
            .map(|v| v != "false")
            .unwrap_or(true);
        drop(cache);
        if !allowed {
            return Err(AppError::Forbidden("注册已关闭".into()));
        }
    }

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
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Json<Value>, AppError> {
    let ip = extract_client_ip(&headers);

    // Check if captcha is required for this IP
    let fail_count = state
        .login_attempts
        .get(&ip)
        .map(|entry| entry.value().0)
        .unwrap_or(0);

    let captcha_required = fail_count >= 3;

    if captcha_required {
        // Validate captcha
        match (&req.captcha_key, &req.captcha_code) {
            (Some(key), Some(code)) => {
                // Remove captcha entry (single-use)
                let entry = state.captcha_store.remove(key);
                match entry {
                    Some((_, captcha_entry)) => {
                        // Check expiry (5 minutes)
                        if captcha_entry.created_at.elapsed().as_secs() > 300 {
                            return Ok(Json(json!({
                                "success": false,
                                "message": "验证码已过期，请刷新",
                                "data": { "captcha_required": true }
                            })));
                        }
                        // Case-insensitive comparison
                        if captcha_entry.answer != code.to_lowercase() {
                            return Ok(Json(json!({
                                "success": false,
                                "message": "验证码错误",
                                "data": { "captcha_required": true }
                            })));
                        }
                        // Captcha valid, proceed to login
                    }
                    None => {
                        return Ok(Json(json!({
                            "success": false,
                            "message": "验证码已失效，请刷新",
                            "data": { "captcha_required": true }
                        })));
                    }
                }
            }
            _ => {
                return Ok(Json(json!({
                    "success": false,
                    "message": "请输入验证码",
                    "data": { "captcha_required": true }
                })));
            }
        }
    }

    // Normal login flow
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
        // Increment fail count
        let mut entry = state.login_attempts.entry(ip.clone()).or_insert((0, Instant::now()));
        entry.0 += 1;
        let new_count = entry.0;
        drop(entry);

        if new_count >= 3 {
            return Ok(Json(json!({
                "success": false,
                "message": "用户名或密码错误",
                "data": { "captcha_required": true }
            })));
        }
        return Err(AppError::Unauthorized("用户名或密码错误".into()));
    }

    // Login success — clear fail count
    state.login_attempts.remove(&ip);

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

/// Public endpoint: query whether registration is allowed (no auth required)
pub async fn register_status(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let cache = state.option_cache.read().await;
    let allow = cache
        .get("allow_register")
        .map(|v| v != "false")
        .unwrap_or(true);
    Ok(Json(json!({
        "success": true,
        "data": { "allow_register": allow }
    })))
}

/// Extract client IP from headers
fn extract_client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("unknown").trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Public endpoint: check if captcha is required for this IP
pub async fn captcha_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let ip = extract_client_ip(&headers);
    let required = state
        .login_attempts
        .get(&ip)
        .map(|entry| entry.value().0 >= 3)
        .unwrap_or(false);
    Ok(Json(json!({
        "success": true,
        "data": { "required": required }
    })))
}

/// Public endpoint: generate a captcha image
pub async fn captcha_image(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    use base64::Engine;
    use captcha_rs::CaptchaBuilder;
    use image::ImageFormat;
    use std::io::Cursor;

    let captcha = CaptchaBuilder::new()
        .length(4)
        .width(160)
        .height(60)
        .complexity(5)
        .build();

    let answer = captcha.text.to_lowercase();
    let captcha_key = uuid::Uuid::new_v4().to_string();

    // Encode image to PNG
    let mut png_buf = Cursor::new(Vec::new());
    captcha
        .image
        .write_to(&mut png_buf, ImageFormat::Png)
        .map_err(|e| AppError::Internal(format!("验证码图片编码失败: {e}")))?;

    let base64_image = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png_buf.into_inner())
    );

    state.captcha_store.insert(
        captcha_key.clone(),
        crate::state::CaptchaEntry {
            answer,
            created_at: Instant::now(),
        },
    );

    Ok(Json(json!({
        "success": true,
        "data": {
            "captcha_key": captcha_key,
            "captcha_image": base64_image
        }
    })))
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
