use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 分享记录 — 由我方账号生成（feature 047）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ShareRecord {
    pub id: i64,
    pub account_id: i64,
    pub file_name: String,
    pub share_url: String,
    pub extract_code: Option<String>,
    pub remote_file_id: Option<String>,
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}
