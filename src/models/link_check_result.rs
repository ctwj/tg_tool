use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 链接检测结果 — 以归一化 URL 的 hash 为唯一缓存键（跨资源/跨推送去重）。
/// 资源级有效性由其全部 URL 的结果在读取时聚合得出，不在此表存储资源级状态。
///
/// status 取值：valid / invalid / pending / unknown（见 data-model.md §1）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LinkCheckResult {
    pub id: i64,
    pub url_hash: String,
    pub normalized_url: String,
    pub platform: Option<String>,
    pub status: String,
    pub fail_reason: Option<String>,
    pub checked_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
}
