// Collection service
// Full collection (batch fetch history) and real-time collection

use crate::errors::AppError;

/// Trigger full history collection for a collector
pub async fn full_collect(
    _collector_id: i64,
    _client_id: &str,
    _channel_id: i64,
) -> Result<usize, AppError> {
    // TODO: Implement with grammers-client
    // 1. Get TG client from tg_manager
    // 2. Call get_chat_history in a loop until all messages fetched
    // 3. For each message, check if already in collector_histories (unique constraint)
    // 4. Insert new messages into collector_histories
    // 5. Return count of new messages collected
    Ok(0)
}

/// Save a real-time collected message
pub async fn save_realtime_message(
    _collector_id: i64,
    _channel_id: i64,
    _message_id: i64,
    _raw_data: &str,
) -> Result<(), AppError> {
    // TODO: Insert into collector_histories if not exists
    Ok(())
}
