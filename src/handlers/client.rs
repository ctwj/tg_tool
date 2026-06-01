use axum::{extract::{Path, State}, Json};
use serde_json::{json, Value};
use crate::errors::AppError;
use crate::state::AppState;

pub async fn list_clients(State(_state): State<AppState>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "data": { "list": [] } })))
}

pub async fn add_client(State(state): State<AppState>, Json(body): Json<serde_json::Value>) -> Result<Json<Value>, AppError> {
    let id = uuid::Uuid::new_v4().to_string()[..16].to_string();
    let client_type = body.get("client_type").and_then(|v| v.as_str()).unwrap_or("Client");
    let phone = body.get("phone").and_then(|v| v.as_str()).unwrap_or("");
    let token = body.get("token").and_then(|v| v.as_str()).unwrap_or("");

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("INSERT INTO clients (id, user_id, client_type, phone, token, status) VALUES (?, 1, ?, ?, ?, 'new')")
                .bind(&id).bind(client_type).bind(phone).bind(token)
                .execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO clients (id, user_id, client_type, phone, token, status) VALUES ($1, 1, $2, $3, $4, 'new')")
                .bind(&id).bind(client_type).bind(phone).bind(token)
                .execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "data": { "id": id } })))
}

pub async fn remove_client(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM clients WHERE id = ?").bind(&id).execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM clients WHERE id = $1").bind(&id).execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "已删除" })))
}

pub async fn get_client_status(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, AppError> {
    let tg_clients = state.tg_clients.read().await;
    let status = tg_clients.get(&id).map(|e| e.status.clone()).unwrap_or_else(|| "new".to_string());
    Ok(Json(json!({ "success": true, "data": { "status": status } })))
}

pub async fn start_client(State(_state): State<AppState>, Path(_id): Path<String>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "客户端启动中" })))
}

pub async fn stop_client(State(_state): State<AppState>, Path(_id): Path<String>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "客户端已停止" })))
}

pub async fn auth_client(State(_state): State<AppState>, Path(_id): Path<String>, Json(_body): Json<serde_json::Value>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "认证中" })))
}

pub async fn get_chats(State(_state): State<AppState>, Path(_id): Path<String>) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "data": { "chats": [] } })))
}
