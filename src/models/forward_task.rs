use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ForwardTask {
    pub id: i64,
    pub remote_id: String,
    pub channel_id: Option<i64>,
    pub message_id: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub link: Option<String>,
    pub file_id: Option<String>,
    pub status: String,
    pub retry_count: i64,
    pub error: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
