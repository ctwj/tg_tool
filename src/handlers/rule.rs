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
    State(state): State<AppState>,
    Query(_params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let rules: Vec<crate::models::rule::Rule> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, user_id, source_chat_id, source_chat_name, forward_method, forward_config, forward_target, is_active, remark, forward_client_id, filter_mode, keywords, media_filter, created_at, updated_at FROM rules ORDER BY id DESC")
                .fetch_all(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, user_id, source_chat_id, source_chat_name, forward_method, forward_config, forward_target, is_active, remark, forward_client_id, filter_mode, keywords, media_filter, created_at, updated_at FROM rules ORDER BY id DESC")
                .fetch_all(pool).await?
        }
    };
    Ok(Json(json!({ "success": true, "data": { "list": rules } })))
}

pub async fn create_rule(
    State(state): State<AppState>,
    axum::extract::Extension(user): axum::extract::Extension<crate::models::user::User>,
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
    let forward_client_id = body.get("forward_client_id").and_then(|v| v.as_str());
    let filter_mode = body
        .get("filter_mode")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let keywords = body.get("keywords").and_then(|v| v.as_str());
    let media_filter = body
        .get("media_filter")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("INSERT INTO rules (user_id, source_chat_id, source_chat_name, forward_method, forward_config, forward_target, is_active, remark, forward_client_id, filter_mode, keywords, media_filter) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(user.id).bind(source_chat_id).bind(source_chat_name).bind(forward_method)
                .bind(&forward_config).bind(forward_target).bind(is_active).bind(remark)
                .bind(forward_client_id).bind(filter_mode).bind(keywords).bind(media_filter)
                .execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO rules (user_id, source_chat_id, source_chat_name, forward_method, forward_config, forward_target, is_active, remark, forward_client_id, filter_mode, keywords, media_filter) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)")
                .bind(user.id).bind(source_chat_id).bind(source_chat_name).bind(forward_method)
                .bind(&forward_config).bind(forward_target).bind(is_active).bind(remark)
                .bind(forward_client_id).bind(filter_mode).bind(keywords).bind(media_filter)
                .execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "规则已创建" })))
}

pub async fn get_rule(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let rule: Option<crate::models::rule::Rule> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, user_id, source_chat_id, source_chat_name, forward_method, forward_config, forward_target, is_active, remark, forward_client_id, filter_mode, keywords, media_filter, created_at, updated_at FROM rules WHERE id = ?")
                .bind(id).fetch_optional(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, user_id, source_chat_id, source_chat_name, forward_method, forward_config, forward_target, is_active, remark, forward_client_id, filter_mode, keywords, media_filter, created_at, updated_at FROM rules WHERE id = $1")
                .bind(id).fetch_optional(pool).await?
        }
    };
    match rule {
        Some(r) => Ok(Json(json!({ "success": true, "data": r }))),
        None => Err(AppError::NotFound("规则不存在".into())),
    }
}

pub async fn update_rule(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let source_chat_id = body.get("source_chat_id").and_then(|v| v.as_i64());
    let source_chat_name = body.get("source_chat_name").and_then(|v| v.as_str());
    let forward_method = body.get("forward_method").and_then(|v| v.as_str());
    let forward_config = body.get("forward_config").map(|v| v.to_string());
    let forward_target = body.get("forward_target").and_then(|v| v.as_str());
    let is_active = body.get("is_active").and_then(|v| v.as_bool());
    let remark = body.get("remark").and_then(|v| v.as_str());
    let forward_client_id = body.get("forward_client_id").and_then(|v| v.as_str());
    let filter_mode = body.get("filter_mode").and_then(|v| v.as_str());
    let keywords = body.get("keywords").and_then(|v| v.as_str());
    let media_filter = body.get("media_filter").and_then(|v| v.as_str());

    let has_update = source_chat_id.is_some()
        || source_chat_name.is_some()
        || forward_method.is_some()
        || forward_config.is_some()
        || forward_target.is_some()
        || is_active.is_some()
        || remark.is_some()
        || forward_client_id.is_some()
        || filter_mode.is_some()
        || keywords.is_some()
        || media_filter.is_some();
    if !has_update {
        return Ok(Json(json!({ "success": true, "message": "规则已更新" })));
    }

    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let mut sets = Vec::new();
            if source_chat_id.is_some() {
                sets.push("source_chat_id = ?");
            }
            if source_chat_name.is_some() {
                sets.push("source_chat_name = ?");
            }
            if forward_method.is_some() {
                sets.push("forward_method = ?");
            }
            if forward_config.is_some() {
                sets.push("forward_config = ?");
            }
            if forward_target.is_some() {
                sets.push("forward_target = ?");
            }
            if is_active.is_some() {
                sets.push("is_active = ?");
            }
            if remark.is_some() {
                sets.push("remark = ?");
            }
            if forward_client_id.is_some() {
                sets.push("forward_client_id = ?");
            }
            if filter_mode.is_some() {
                sets.push("filter_mode = ?");
            }
            if keywords.is_some() {
                sets.push("keywords = ?");
            }
            if media_filter.is_some() {
                sets.push("media_filter = ?");
            }
            sets.push("updated_at = CURRENT_TIMESTAMP");
            let sql = format!("UPDATE rules SET {} WHERE id = ?", sets.join(", "));
            let mut q = sqlx::query(&sql);
            if let Some(v) = source_chat_id {
                q = q.bind(v);
            }
            if let Some(v) = source_chat_name {
                q = q.bind(v);
            }
            if let Some(v) = forward_method {
                q = q.bind(v);
            }
            if let Some(v) = &forward_config {
                q = q.bind(v);
            }
            if let Some(v) = forward_target {
                q = q.bind(v);
            }
            if let Some(v) = is_active {
                q = q.bind(v);
            }
            if let Some(v) = remark {
                q = q.bind(v);
            }
            if let Some(v) = forward_client_id {
                q = q.bind(v);
            }
            if let Some(v) = filter_mode {
                q = q.bind(v);
            }
            if let Some(v) = keywords {
                q = q.bind(v);
            }
            if let Some(v) = media_filter {
                q = q.bind(v);
            }
            q = q.bind(id);
            let result = q.execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("规则不存在".into()));
            }
        }
        crate::state::DbPool::Postgres(pool) => {
            let mut pg_parts = Vec::new();
            let mut pg_idx = 1u32;
            if source_chat_id.is_some() {
                pg_parts.push(format!("source_chat_id = ${pg_idx}"));
                pg_idx += 1;
            }
            if source_chat_name.is_some() {
                pg_parts.push(format!("source_chat_name = ${pg_idx}"));
                pg_idx += 1;
            }
            if forward_method.is_some() {
                pg_parts.push(format!("forward_method = ${pg_idx}"));
                pg_idx += 1;
            }
            if forward_config.is_some() {
                pg_parts.push(format!("forward_config = ${pg_idx}"));
                pg_idx += 1;
            }
            if forward_target.is_some() {
                pg_parts.push(format!("forward_target = ${pg_idx}"));
                pg_idx += 1;
            }
            if is_active.is_some() {
                pg_parts.push(format!("is_active = ${pg_idx}"));
                pg_idx += 1;
            }
            if remark.is_some() {
                pg_parts.push(format!("remark = ${pg_idx}"));
                pg_idx += 1;
            }
            if forward_client_id.is_some() {
                pg_parts.push(format!("forward_client_id = ${pg_idx}"));
                pg_idx += 1;
            }
            if filter_mode.is_some() {
                pg_parts.push(format!("filter_mode = ${pg_idx}"));
                pg_idx += 1;
            }
            if keywords.is_some() {
                pg_parts.push(format!("keywords = ${pg_idx}"));
                pg_idx += 1;
            }
            if media_filter.is_some() {
                pg_parts.push(format!("media_filter = ${pg_idx}"));
                pg_idx += 1;
            }
            pg_parts.push("updated_at = CURRENT_TIMESTAMP".to_string());
            let sql = format!(
                "UPDATE rules SET {} WHERE id = ${pg_idx}",
                pg_parts.join(", ")
            );
            let mut q = sqlx::query(&sql);
            if let Some(v) = source_chat_id {
                q = q.bind(v);
            }
            if let Some(v) = source_chat_name {
                q = q.bind(v);
            }
            if let Some(v) = forward_method {
                q = q.bind(v);
            }
            if let Some(v) = &forward_config {
                q = q.bind(v);
            }
            if let Some(v) = forward_target {
                q = q.bind(v);
            }
            if let Some(v) = is_active {
                q = q.bind(v);
            }
            if let Some(v) = remark {
                q = q.bind(v);
            }
            if let Some(v) = forward_client_id {
                q = q.bind(v);
            }
            if let Some(v) = filter_mode {
                q = q.bind(v);
            }
            if let Some(v) = keywords {
                q = q.bind(v);
            }
            if let Some(v) = media_filter {
                q = q.bind(v);
            }
            q = q.bind(id);
            let result = q.execute(pool).await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("规则不存在".into()));
            }
        }
    }
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
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let (list, total): (Vec<crate::models::message::Message>, i64) = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE rule_id = ?")
                .bind(id)
                .fetch_one(pool)
                .await?;
            let list = sqlx::query_as(
                "SELECT id, rule_id, chat_id, message_id, content, raw_data, status, error_reason, created_at FROM messages WHERE rule_id = ? ORDER BY id DESC LIMIT ? OFFSET ?"
            ).bind(id).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
        crate::state::DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE rule_id = $1")
                .bind(id)
                .fetch_one(pool)
                .await?;
            let list = sqlx::query_as(
                "SELECT id, rule_id, chat_id, message_id, content, raw_data, status, error_reason, created_at FROM messages WHERE rule_id = $1 ORDER BY id DESC LIMIT $2 OFFSET $3"
            ).bind(id).bind(page_size).bind(offset).fetch_all(pool).await?;
            (list, total)
        }
    };
    Ok(Json(
        json!({ "success": true, "data": { "list": list, "pagination": { "page": page, "page_size": page_size, "total": total } } }),
    ))
}
