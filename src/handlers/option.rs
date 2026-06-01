use axum::{Json, extract::State};
use serde_json::{Value, json};

use crate::errors::AppError;
use crate::state::AppState;

pub async fn get_options(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let cache = state.option_cache.read().await;
    let data: serde_json::Map<String, Value> =
        cache.iter().map(|(k, v)| (k.clone(), json!(v))).collect();

    // Build env_defaults: show what env vars provide (hide sensitive values)
    let env_defaults = json!({
        "tg_app_id": if state.config.tg_app_id != 0 {
            state.config.tg_app_id.to_string()
        } else {
            String::new()
        },
        "tg_app_hash": if state.config.tg_app_hash.is_empty() {
            String::new()
        } else {
            "已配置".to_string()
        },
        "proxy_url": "",
    });

    Ok(Json(json!({
        "success": true,
        "data": data,
        "env_defaults": env_defaults,
    })))
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
                    sqlx::query(
                        "INSERT INTO options (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = $2",
                    )
                    .bind(key)
                    .bind(&val_str)
                    .execute(pool)
                    .await?;
                }
            }

            cache.insert(key.clone(), val_str);
        }
    }
    Ok(Json(json!({ "success": true, "message": "配置已更新" })))
}

/// 测试代理连通性
pub async fn test_proxy(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let proxy_url = state.proxy_url().await;

    let proxy_url = match proxy_url {
        Some(url) if !url.is_empty() => url,
        _ => {
            return Ok(Json(json!({
                "success": false,
                "message": "未配置代理地址",
            })));
        }
    };

    // Build reqwest client with proxy
    let proxy = reqwest::Proxy::all(&proxy_url)
        .map_err(|e| AppError::BadRequest(format!("代理配置无效: {e}")))?;

    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(format!("创建 HTTP 客户端失败: {e}")))?;

    let start = std::time::Instant::now();

    let result = client
        .get("https://httpbin.org/ip")
        .send()
        .await
        .and_then(|resp| resp.error_for_status());

    let elapsed = start.elapsed().as_millis();

    match result {
        Ok(_) => Ok(Json(json!({
            "success": true,
            "message": format!("代理连接成功，耗时 {}ms", elapsed),
            "latency_ms": elapsed,
        }))),
        Err(e) => Ok(Json(json!({
            "success": false,
            "message": format!("代理连接失败: {e}"),
            "latency_ms": elapsed,
        }))),
    }
}
