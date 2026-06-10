// Message forwarding service
// Supports Chat (grammers send_message) and Webhook (reqwest POST) modes

use crate::errors::AppError;
use crate::state::{DbPool, PeerCache, TgClientMap};

/// Forward a message to the target
#[allow(clippy::too_many_arguments)]
pub async fn forward_message(
    rule_id: i64,
    method: &str,
    target: &str,
    config: Option<&str>,
    content: &str,
    tg_clients: &TgClientMap,
    peer_cache: &PeerCache,
    db: &DbPool,
    forward_client_id: Option<&str>,
) -> Result<(), AppError> {
    let result = match method {
        "WebHook" | "Webhook" => forward_webhook(target, config, content).await,
        "Chat" => forward_chat(target, forward_client_id, content, tg_clients, peer_cache).await,
        _ => Err(AppError::BadRequest(format!("未知的转发方式: {method}"))),
    };

    // Record the forwarding result
    let (status, error_reason) = match &result {
        Ok(()) => ("success", None::<String>),
        Err(e) => ("failed", Some(e.to_string())),
    };

    match db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO messages (rule_id, content, status, error_reason) VALUES (?, ?, ?, ?)",
            )
            .bind(rule_id)
            .bind(content)
            .bind(status)
            .bind(&error_reason)
            .execute(pool)
            .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO messages (rule_id, content, status, error_reason) VALUES ($1, $2, $3, $4)",
            )
            .bind(rule_id)
            .bind(content)
            .bind(status)
            .bind(&error_reason)
            .execute(pool)
            .await?;
        }
    }

    result
}

async fn forward_webhook(
    target: &str,
    config: Option<&str>,
    content: &str,
) -> Result<(), AppError> {
    let (webhook_url, http_method) = if let Some(config_str) = config {
        let cfg: serde_json::Value = serde_json::from_str(config_str)
            .map_err(|e| AppError::BadRequest(format!("转发配置解析失败: {e}")))?;
        let url = cfg["webhook_url"].as_str().unwrap_or(target).to_string();
        let method = cfg["method"].as_str().unwrap_or("POST").to_string();
        (url, method)
    } else {
        (target.to_string(), "POST".to_string())
    };

    if webhook_url.is_empty() {
        return Err(AppError::BadRequest("Webhook URL 为空".into()));
    }

    let payload = serde_json::json!({
        "message": content
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Internal(format!("创建 HTTP 客户端失败: {e}")))?;

    let resp = client
        .request(
            http_method.parse().unwrap_or(reqwest::Method::POST),
            &webhook_url,
        )
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Webhook 请求失败: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Webhook 返回错误: status={}, body={}",
            status, body
        )));
    }

    Ok(())
}

async fn forward_chat(
    target: &str,
    forward_client_id: Option<&str>,
    content: &str,
    tg_clients: &TgClientMap,
    peer_cache: &PeerCache,
) -> Result<(), AppError> {
    let chat_id: i64 = target
        .parse()
        .map_err(|e| AppError::BadRequest(format!("无效的转发目标: {e}")))?;

    // Resolve peer using cache
    let packed = crate::services::tg_api::resolve_peer(chat_id, tg_clients, peer_cache).await?;

    // Use specified client if forward_client_id is provided, otherwise fall back to any active
    let clients = tg_clients.read().await;
    let client = match forward_client_id {
        Some(id) => clients
            .get(id)
            .filter(|e| e.status == "active" && e.client.is_some())
            .and_then(|e| e.client.clone())
            .ok_or_else(|| AppError::Internal(format!("转发客户端 {id} 不可用（离线或未登录）")))?,
        None => clients
            .values()
            .find(|e| e.status == "active" && e.client.is_some())
            .and_then(|e| e.client.clone())
            .ok_or_else(|| AppError::NotFound("没有可用的在线客户端".into()))?,
    };
    drop(clients);

    client
        .send_message(packed, content)
        .await
        .map_err(|e| AppError::Internal(format!("发送消息失败: {e}")))?;

    Ok(())
}
