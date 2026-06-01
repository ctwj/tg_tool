use crate::errors::AppError;
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
}

pub async fn list_rules(
    State(_state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(10);
    Ok(Json(
        json!({ "success": true, "data": { "list": [], "pagination": { "page": page, "page_size": page_size, "total": 0 } } }),
    ))
}

pub async fn create_rule(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let source_chat_id = body
        .get("source_chat_id")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let source_chat_name = body
        .get("source_chat_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let forward_method = body
        .get("forward_method")
        .and_then(|v| v.as_str())
        .unwrap_or("Chat");
    let forward_config = body.get("forward_config").map(|v| v.to_string());
    let forward_target = body
        .get("forward_target")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let is_active = body
        .get("is_active")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let remark = body.get("remark").and_then(|v| v.as_str()).unwrap_or("");

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("INSERT INTO rules (user_id, source_chat_id, source_chat_name, forward_method, forward_config, forward_target, is_active, remark) VALUES (1, ?, ?, ?, ?, ?, ?, ?)")
                .bind(source_chat_id).bind(source_chat_name).bind(forward_method)
                .bind(&forward_config).bind(forward_target).bind(is_active).bind(remark)
                .execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO rules (user_id, source_chat_id, source_chat_name, forward_method, forward_config, forward_target, is_active, remark) VALUES (1, $1, $2, $3, $4, $5, $6, $7)")
                .bind(source_chat_id).bind(source_chat_name).bind(forward_method)
                .bind(&forward_config).bind(forward_target).bind(is_active).bind(remark)
                .execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "规则已创建" })))
}

pub async fn get_rule(
    State(_state): State<AppState>,
    Path(_id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    Err(AppError::NotFound("规则不存在".into()))
}

pub async fn update_rule(
    State(_state): State<AppState>,
    Path(_id): Path<i64>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "message": "规则已更新" })))
}

pub async fn delete_rule(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM rules WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM rules WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "规则已删除" })))
}

pub async fn toggle_rule(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("UPDATE rules SET is_active = NOT is_active, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(id).execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("UPDATE rules SET is_active = NOT is_active, updated_at = CURRENT_TIMESTAMP WHERE id = $1")
                .bind(id).execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "状态已切换" })))
}

pub async fn get_rule_messages(
    State(_state): State<AppState>,
    Path(_id): Path<i64>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(10);
    Ok(Json(
        json!({ "success": true, "data": { "list": [], "pagination": { "page": page, "page_size": page_size, "total": 0 } } }),
    ))
}
