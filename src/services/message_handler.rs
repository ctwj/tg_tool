// Message listener and dispatcher
// Receives Update::NewMessage from grammers clients, matches active Rules and Collectors

use crate::errors::AppError;
use crate::state::{DbPool, PeerCache, TgClientMap};
use grammers_client::types::Message;

/// Handle a new incoming message from Telegram
/// Called by tg_manager update listener when a new message is received
pub async fn handle_new_message(
    client_id: &str,
    msg: &Message,
    db: &DbPool,
    tg_clients: &TgClientMap,
    peer_cache: &PeerCache,
) -> Result<(), AppError> {
    let chat_id = msg.chat().id();
    let message_id = msg.id() as i64;
    let text = msg.text();

    tracing::debug!(
        "New message: client={}, chat={}, msg_id={}, text_len={}",
        client_id,
        chat_id,
        message_id,
        text.len()
    );

    // 1. Match active forwarding rules
    let rules = match db {
        crate::state::DbPool::Sqlite(pool) => {
            let rows: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
                "SELECT id, forward_method, forward_target, forward_config FROM rules WHERE source_chat_id = ? AND is_active = 1",
            )
            .bind(chat_id)
            .fetch_all(pool)
            .await
            .unwrap_or_default();
            rows
        }
        crate::state::DbPool::Postgres(pool) => {
            let rows: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
                "SELECT id, forward_method, forward_target, forward_config FROM rules WHERE source_chat_id = $1 AND is_active = true",
            )
            .bind(chat_id)
            .fetch_all(pool)
            .await
            .unwrap_or_default();
            rows
        }
    };

    for (rule_id, method, target, config) in &rules {
        if let Err(e) = crate::services::forwarder::forward_message(
            *rule_id,
            method,
            target,
            config.as_deref(),
            text,
            tg_clients,
            peer_cache,
            db,
        )
        .await
        {
            tracing::warn!("Forward failed for rule {}: {e}", rule_id);
        }
    }

    // 2. Match active collectors
    let collectors: Vec<(i64, i64)> = match db {
        crate::state::DbPool::Sqlite(pool) => sqlx::query_as(
            "SELECT id, channel_id FROM collectors WHERE channel_id = ? AND is_active = 1",
        )
        .bind(chat_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default(),
        crate::state::DbPool::Postgres(pool) => sqlx::query_as(
            "SELECT id, channel_id FROM collectors WHERE channel_id = $1 AND is_active = true",
        )
        .bind(chat_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default(),
    };

    // Serialize message to JSON manually (grammers Message doesn't impl Serialize)
    let raw_data = serialize_message(msg);
    let post_time = msg.date().naive_utc();

    // 提取封面 photo_id
    let remote_id = crate::services::collector::extract_photo_id(msg);

    for (collector_id, channel_id) in collectors {
        if let Err(e) = crate::services::collector::save_realtime_message(
            collector_id,
            channel_id,
            message_id,
            &raw_data,
            post_time,
            remote_id.as_deref(),
            db,
        )
        .await
        {
            tracing::warn!(
                "Save realtime message failed for collector {}: {e}",
                collector_id
            );
        }
    }

    Ok(())
}

/// Serialize a grammers Message to JSON manually
fn serialize_message(msg: &Message) -> String {
    serde_json::json!({
        "id": msg.id(),
        "date": msg.date().timestamp(),
        "text": msg.text(),
        "outgoing": msg.outgoing(),
        "chat_id": msg.chat().id(),
    })
    .to_string()
}
