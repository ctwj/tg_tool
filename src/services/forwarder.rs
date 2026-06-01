// Message forwarding service
// Supports Chat (grammers send_message) and Webhook (reqwest POST) modes

use crate::errors::AppError;

/// Forward a message to the target
pub async fn forward_message(
    _rule_id: i64,
    _method: &str,      // "Chat" or "Webhook"
    _target: &str,       // chat_id or webhook_url
    _config: Option<&str>, // JSON config for webhook
    _content: &str,
) -> Result<(), AppError> {
    // TODO: Implement forwarding
    // Chat mode: call tg_api::send_message with target chat_id
    // Webhook mode: call reqwest POST to target URL
    Ok(())
}
