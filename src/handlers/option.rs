use crate::errors::AppError;
use crate::state::AppState;
use axum::{Json, extract::State};
use serde_json::{Value, json};

pub async fn get_options(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let cache = state.option_cache.read().await;
    Ok(Json(json!({ "success": true, "data": *cache })))
}

pub async fn update_options(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    if let Some(obj) = body.as_object() {
        let mut cache = state.option_cache.write().await;
        for (key, value) in obj {
            let val_str = value.as_str().unwrap_or("").to_string();

            match &state.db {
                crate::state::DbPool::Sqlite(pool) => {
                    sqlx::query("INSERT OR REPLACE INTO options (key, value) VALUES (?, ?)")
                        .bind(key)
                        .bind(&val_str)
                        .execute(pool)
                        .await?;
                }
                crate::state::DbPool::Postgres(pool) => {
                    sqlx::query("INSERT INTO options (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = $2")
                        .bind(key).bind(&val_str)
                        .execute(pool).await?;
                }
            }

            cache.insert(key.clone(), val_str);
        }
    }
    Ok(Json(json!({ "success": true, "message": "配置已更新" })))
}
