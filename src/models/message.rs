use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Message {
    pub id: i64,
    pub rule_id: i64,
    pub chat_id: Option<i64>,
    pub message_id: Option<i64>,
    pub content: Option<String>,
    pub raw_data: Option<String>,
    pub status: String,
    pub error_reason: Option<String>,
    pub created_at: NaiveDateTime,
}
