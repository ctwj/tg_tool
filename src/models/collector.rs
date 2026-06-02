use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Collector {
    pub id: i64,
    pub user_id: i64,
    pub client_id: Option<String>,
    pub channel_id: i64,
    pub channel_name: Option<String>,
    pub collector_type: String,
    pub is_active: bool,
    pub remark: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
