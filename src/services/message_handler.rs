// Message listener and dispatcher
// Receives Update::NewMessage from grammers clients, matches active Rules and Collectors

use crate::errors::AppError;
use crate::state::{DbPool, PeerCache, TgClientMap};
use grammers_client::types::Message;

/// Matched rule row: (id, method, target, config, forward_client_id, filter_mode, keywords, media_filter, source_client_id)
type RuleRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Check if a message passes keyword filtering
/// filter_mode: "none" | "include" (whitelist) | "exclude" (blacklist)
/// keywords: comma-separated keywords
fn keyword_pass(text: &str, filter_mode: Option<&str>, keywords: Option<&str>) -> bool {
    let mode = filter_mode.unwrap_or("none");
    match mode {
        "none" | "" => true,
        "include" => {
            // Whitelist: message must contain at least one keyword
            if let Some(kws) = keywords {
                let text_lower = text.to_lowercase();
                kws.split(',').any(|kw| {
                    !kw.trim().is_empty() && text_lower.contains(&kw.trim().to_lowercase())
                })
            } else {
                false
            }
        }
        "exclude" => {
            // Blacklist: message must NOT contain any keyword
            if let Some(kws) = keywords {
                let text_lower = text.to_lowercase();
                !kws.split(',').any(|kw| {
                    !kw.trim().is_empty() && text_lower.contains(&kw.trim().to_lowercase())
                })
            } else {
                true
            }
        }
        _ => true,
    }
}

/// Check if a message passes media type filtering
/// media_filter: "all" | "photo" | "document" | "text"
fn media_pass(msg: &Message, media_filter: Option<&str>) -> bool {
    let filter = media_filter.unwrap_or("all");
    match filter {
        "all" | "" => true,
        "photo" => matches!(msg.media(), Some(grammers_client::types::Media::Photo(_))),
        "document" => matches!(
            msg.media(),
            Some(grammers_client::types::Media::Document(_))
        ),
        "text" => msg.media().is_none(),
        _ => true,
    }
}

/// Handle a new incoming message from Telegram
/// Called by tg_manager update listener when a new message is received
pub async fn handle_new_message(
    client_id: &str,
    msg: &Message,
    outgoing: bool,
    db: &DbPool,
    tg_clients: &TgClientMap,
    peer_cache: &PeerCache,
) -> Result<(), AppError> {
    let chat_id = msg.chat().id();
    let message_id = msg.id() as i64;
    let text = msg.text();

    tracing::debug!(
        "New message: client={}, chat={}, msg_id={}, outgoing={}, text_len={}",
        client_id,
        chat_id,
        message_id,
        outgoing,
        text.len()
    );

    // 1. Match active forwarding rules (skip outgoing to avoid loops)
    if !outgoing {
        // forwarding rules matching — includes filter fields + source_client_id
        let rules: Vec<RuleRow> = match db {
            crate::state::DbPool::Sqlite(pool) => {
                sqlx::query_as(
                "SELECT id, forward_method, forward_target, forward_config, forward_client_id, filter_mode, keywords, media_filter, source_client_id FROM rules WHERE source_chat_id = ? AND is_active = 1",
            )
            .bind(chat_id)
            .fetch_all(pool)
            .await
            .unwrap_or_default()
            }
            crate::state::DbPool::Postgres(pool) => {
                sqlx::query_as(
                "SELECT id, forward_method, forward_target, forward_config, forward_client_id, filter_mode, keywords, media_filter, source_client_id FROM rules WHERE source_chat_id = $1 AND is_active = true",
            )
            .bind(chat_id)
            .fetch_all(pool)
            .await
            .unwrap_or_default()
            }
        };

        for (rule_id, method, target, config, _fcid, fmode, kws, mfilt, rule_source_client_id) in
            &rules
        {
            // Keyword filter
            if !keyword_pass(text, fmode.as_deref(), kws.as_deref()) {
                tracing::debug!("Rule {rule_id}: skipped by keyword filter");
                continue;
            }
            // Media type filter
            if !media_pass(msg, mfilt.as_deref()) {
                tracing::debug!("Rule {rule_id}: skipped by media filter");
                continue;
            }

            // Determine which client to use for forwarding
            // Priority: rule's source_client_id > current client_id
            let forward_client_id = rule_source_client_id.as_deref().unwrap_or(client_id);

            if let Err(e) = crate::services::forwarder::forward_message(
                *rule_id,
                method,
                target,
                config.as_deref(),
                msg,
                tg_clients,
                peer_cache,
                db,
                forward_client_id,
            )
            .await
            {
                tracing::warn!("Forward failed for rule {}: {e}", rule_id);
            }
        }
    } // end if !outgoing

    // 2. Match active collectors (collect ALL messages including outgoing)
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

/// Serialize a grammers Message to JSON manually (with media info)
fn serialize_message(msg: &Message) -> String {
    let mut json = serde_json::json!({
        "id": msg.id(),
        "date": msg.date().timestamp(),
        "text": crate::services::collector::message_text_with_links(msg),
        "outgoing": msg.outgoing(),
        "chat_id": msg.chat().id(),
    });
    if let Some(media) = msg.media() {
        match media {
            grammers_client::types::Media::Photo(photo) => {
                json["media_type"] = serde_json::json!("photo");
                json["photo_id"] = serde_json::json!(format!("{}", photo.id()));
            }
            grammers_client::types::Media::Document(doc) => {
                json["media_type"] = serde_json::json!("document");
                json["document_name"] = serde_json::json!(doc.name());
            }
            _ => {}
        }
    }
    json.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_pass_none_mode() {
        assert!(keyword_pass("任意文本", Some("none"), Some("广告")));
        assert!(keyword_pass("任意文本", None, None));
        assert!(keyword_pass("任意文本", Some(""), None));
    }

    #[test]
    fn test_keyword_pass_exclude() {
        // Blacklist: contains keyword → blocked
        assert!(!keyword_pass(
            "这是一条广告消息",
            Some("exclude"),
            Some("广告,推广")
        ));
        assert!(!keyword_pass(
            "推广活动",
            Some("exclude"),
            Some("广告,推广")
        ));
        // Does not contain → pass
        assert!(keyword_pass("正常消息", Some("exclude"), Some("广告,推广")));
        // Empty keywords → pass
        assert!(keyword_pass("广告", Some("exclude"), Some("")));
        assert!(keyword_pass("广告", Some("exclude"), None));
    }

    #[test]
    fn test_keyword_pass_include() {
        // Whitelist: contains keyword → pass
        assert!(keyword_pass("资源分享", Some("include"), Some("资源,分享")));
        assert!(keyword_pass("分享链接", Some("include"), Some("资源,分享")));
        // Does not contain → blocked
        assert!(!keyword_pass(
            "普通消息",
            Some("include"),
            Some("资源,分享")
        ));
        // Empty keywords → blocked (nothing to match)
        assert!(!keyword_pass("资源", Some("include"), None));
    }

    #[test]
    fn test_keyword_pass_case_insensitive() {
        assert!(!keyword_pass("SALE SALE", Some("exclude"), Some("sale")));
        assert!(keyword_pass("SALE SALE", Some("include"), Some("sale")));
    }
}
