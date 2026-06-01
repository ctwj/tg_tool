use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PushHistory {
    pub id: i64,
    pub batch_id: String,
    pub target: Option<String>,
    pub status: String,
    pub data_count: i32,
    pub message: Option<String>,
    pub error_msg: Option<String>,
    pub pushed_at: NaiveDateTime,
}
