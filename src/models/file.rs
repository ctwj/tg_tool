use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FileRecord {
    pub id: i64,
    pub filename: String,
    pub uploader_id: i64,
    pub link: Option<String>,
    pub created_at: NaiveDateTime,
}
