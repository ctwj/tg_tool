// Message forwarding service
// Supports Chat (copy_media + send_album for media, send_message for text) and Webhook modes

use crate::errors::AppError;
use crate::state::{DbPool, PeerCache, TgClientMap};
use grammers_client::InputMedia;
use grammers_client::types::Message;

/// Forward a message to the target
#[allow(clippy::too_many_arguments)]
pub async fn forward_message(
    rule_id: i64,
    method: &str,
    target: &str,
    config: Option<&str>,
    msg: &Message,
    tg_clients: &TgClientMap,
    peer_cache: &PeerCache,
    db: &DbPool,
    source_client_id: &str,
) -> Result<(), AppError> {
    // 隐藏超链接（TextUrl entity）的 URL 不在纯文本中，需展开后再交给下游
    let text_with_links = crate::services::collector::message_text_with_links(msg);

    let result = match method {
        "WebHook" | "Webhook" => forward_webhook(target, config, &text_with_links).await,
        "Chat" => forward_chat(target, source_client_id, msg, tg_clients, peer_cache).await,
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
            .bind(&text_with_links)
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
            .bind(&text_with_links)
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

/// Forward a message using the same client that received it.
/// Uses `InputMedia::copy_media` to re-send media by remote ID (no download needed).
/// Falls back to `send_message` for text-only messages.
async fn forward_chat(
    target: &str,
    source_client_id: &str,
    msg: &Message,
    tg_clients: &TgClientMap,
    peer_cache: &PeerCache,
) -> Result<(), AppError> {
    let target_chat_id: i64 = target
        .parse()
        .map_err(|e| AppError::BadRequest(format!("无效的转发目标: {e}")))?;

    // Get the source client instance
    let clients = tg_clients.read().await;
    let client = clients
        .get(source_client_id)
        .filter(|e| e.status == "active" && e.client.is_some())
        .and_then(|e| e.client.clone())
        .ok_or_else(|| {
            AppError::Internal(format!("客户端 {source_client_id} 不可用（离线或未登录）"))
        })?;
    drop(clients);

    // Resolve target peer
    let packed =
        crate::services::tg_api::resolve_peer(target_chat_id, tg_clients, peer_cache).await?;

    // Check if message has media
    if let Some(media) = msg.media() {
        // Use copy_media to reference the existing media by remote ID
        // send_album accepts a Vec<InputMedia>, sends as a new message (no "forwarded from")
        // fmt_entities 随迁：caption 中的隐藏超链接/粗体等格式在目标 chat 原生保留
        let mut input_media = InputMedia::caption(msg.text()).copy_media(&media);
        if let Some(entities) = msg.fmt_entities() {
            input_media = input_media.fmt_entities(entities.clone());
        }
        client
            .send_album(packed, vec![input_media])
            .await
            .map_err(|e| AppError::Internal(format!("转发媒体消息失败: {e}")))?;
    } else {
        // Text-only: send as plain message, reusing original entities
        // so hidden hyperlinks (TextUrl) render as clickable links, not lost
        let mut input = grammers_client::InputMessage::text(msg.text());
        if let Some(entities) = msg.fmt_entities() {
            input = input.fmt_entities(entities.clone());
        }
        client
            .send_message(packed, input)
            .await
            .map_err(|e| AppError::Internal(format!("发送消息失败: {e}")))?;
    }

    Ok(())
}
