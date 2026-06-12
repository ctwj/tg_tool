use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 推送跳过明细 — 每次推送中被跳过的资源及其原因（Story3 详情）。
///
/// skip_reason 取值：image_not_forwarded / link_invalid
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PushSkipRecord {
    pub id: i64,
    pub push_history_id: i64,
    pub resource_id: i64,
    pub skip_reason: String,
    /// link_invalid 时记录失效 URL 列表（逗号分隔）；image_not_forwarded 时为 None
    pub urls_invalid: Option<String>,
    pub detail: Option<String>,
    pub created_at: NaiveDateTime,
}
