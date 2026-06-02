use crate::errors::AppError;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

pub async fn list_clients(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let clients = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, crate::models::client::Client>(
                "SELECT id, user_id, client_type, phone, token, status, session_path, created_at, updated_at FROM clients ORDER BY created_at DESC",
            )
            .fetch_all(pool)
            .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as::<_, crate::models::client::Client>(
                "SELECT id, user_id, client_type, phone, token, status, session_path, created_at, updated_at FROM clients ORDER BY created_at DESC",
            )
            .fetch_all(pool)
            .await?
        }
    };
    Ok(Json(json!({ "success": true, "data": { "list": clients } })))
}

pub async fn add_client(
    State(state): State<AppState>,
    axum::extract::Extension(user): axum::extract::Extension<crate::models::user::User>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let id = uuid::Uuid::new_v4().to_string()[..16].to_string();
    let client_type = body
        .get("client_type")
        .and_then(|v| v.as_str())
        .unwrap_or("Client");
    let phone = body.get("phone").and_then(|v| v.as_str()).unwrap_or("");
    let token = body.get("token").and_then(|v| v.as_str()).unwrap_or("");

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("INSERT INTO clients (id, user_id, client_type, phone, token, status) VALUES (?, ?, ?, ?, ?, 'new')")
                .bind(&id).bind(user.id).bind(client_type).bind(phone).bind(token)
                .execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO clients (id, user_id, client_type, phone, token, status) VALUES ($1, $2, $3, $4, $5, 'new')")
                .bind(&id).bind(user.id).bind(client_type).bind(phone).bind(token)
                .execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "data": { "id": id } })))
}

pub async fn remove_client(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    // Stop and cleanup via TgManager
    state.tg_manager.remove_client(&id).await.ok();

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM clients WHERE id = ?")
                .bind(&id)
                .execute(pool)
                .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM clients WHERE id = $1")
                .bind(&id)
                .execute(pool)
                .await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "已删除" })))
}

pub async fn get_client_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let status = state.tg_manager.get_status(&id).await;
    Ok(Json(
        json!({ "success": true, "data": { "status": status } }),
    ))
}

pub async fn start_client(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let status = state.tg_manager.start_client(&id).await?;

    // Update status in database
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let result = sqlx::query("UPDATE clients SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(&status).bind(&id).execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("客户端不存在".into()));
            }
        }
        crate::state::DbPool::Postgres(pool) => {
            let result = sqlx::query("UPDATE clients SET status = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2")
                .bind(&status).bind(&id).execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("客户端不存在".into()));
            }
        }
    }

    let message = if status == "active" {
        "客户端已连接"
    } else {
        "客户端已连接，需要认证"
    };

    Ok(Json(json!({ "success": true, "data": { "status": status, "message": message } })))
}

pub async fn stop_client(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    state.tg_manager.stop_client(&id).await?;

    // Update status in database
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let result = sqlx::query("UPDATE clients SET status = 'offline', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(&id).execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("客户端不存在".into()));
            }
        }
        crate::state::DbPool::Postgres(pool) => {
            let result = sqlx::query("UPDATE clients SET status = 'offline', updated_at = CURRENT_TIMESTAMP WHERE id = $1")
                .bind(&id).execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("客户端不存在".into()));
            }
        }
    }

    Ok(Json(json!({ "success": true, "message": "客户端已停止" })))
}

pub async fn auth_client(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let auth_type = body.get("type").and_then(|v| v.as_str()).unwrap_or("code");
    let value = body.get("value").and_then(|v| v.as_str()).unwrap_or("");

    let auth_state = match auth_type {
        "phone" => {
            crate::services::tg_auth::request_login_code(
                &id, value, &state.tg_clients, &state.db,
            )
            .await?
        }
        "code" => {
            crate::services::tg_auth::submit_code(
                &id, value, &state.tg_clients, &state.db, &state.tg_manager,
            )
            .await?
        }
        "password" => {
            crate::services::tg_auth::submit_password(
                &id, value, &state.tg_clients, &state.db, &state.tg_manager,
            )
            .await?
        }
        "bot_token" => {
            crate::services::tg_auth::bot_sign_in(
                &id, value, &state.tg_clients, &state.db, &state.tg_manager,
            )
            .await?
        }
        _ => return Err(AppError::BadRequest("无效的认证类型".into())),
    };

    let status = match auth_state {
        crate::services::tg_auth::AuthState::WaitCode => "wait_code",
        crate::services::tg_auth::AuthState::WaitPassword => "wait_password",
        crate::services::tg_auth::AuthState::Ready => "active",
        crate::services::tg_auth::AuthState::Unauthenticated => "new",
    };

    let message = match auth_state {
        crate::services::tg_auth::AuthState::WaitCode => "验证码已发送",
        crate::services::tg_auth::AuthState::WaitPassword => "需要两步验证密码",
        crate::services::tg_auth::AuthState::Ready => "认证成功",
        crate::services::tg_auth::AuthState::Unauthenticated => "未认证",
    };

    Ok(Json(json!({ "success": true, "data": { "status": status, "message": message } })))
}

pub async fn get_chats(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let chats = crate::services::tg_api::get_chat_list(&id, &state.tg_clients).await?;
    Ok(Json(json!({ "success": true, "data": { "chats": chats } })))
}
