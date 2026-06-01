// Message listener and dispatcher
// Receives UpdateNewMessage from grammers clients, matches active Rules and Collectors

use crate::errors::AppError;

/// Handle a new incoming message from Telegram
/// This is called by tg_manager when a new message update is received
pub async fn handle_new_message(
    _client_id: &str,
    _chat_id: i64,
    _message_id: i64,
    _text: &str,
) -> Result<(), AppError> {
    // TODO: Implement message dispatch logic
    // 1. Look up active rules where source_chat_id matches
    // 2. For each matching rule, call forwarder::forward_message()
    // 3. Look up active collectors where channel_id matches
    // 4. For each matching collector, save to collector_histories
    Ok(())
}
