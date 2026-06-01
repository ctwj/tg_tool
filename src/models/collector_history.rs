use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CollectorHistory {
    pub id: i64,
    pub collector_id: i64,
    pub channel_id: i64,
    pub message_id: i64,
    pub post_time: Option<NaiveDateTime>,
    pub raw_data: Option<String>,
    pub is_auto_push: bool,
    pub remote_id: Option<String>,
    pub created_at: NaiveDateTime,
}
