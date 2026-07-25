use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 开放 API 凭据（feature 047 US4）— key_hash 存 SHA-256，明文不入库
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiKey {
    pub id: i64,
    pub system_name: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub status: String,   // enabled | disabled
    pub quota_limit: i64, // 0 = 无限
    pub quota_used: i64,
    pub quota_reset_at: Option<NaiveDateTime>,
    pub rate_limit_qps: Option<i64>,
    pub created_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
    pub rotated_at: Option<NaiveDateTime>,
}

/// 脱敏视图（无 key_hash）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyView {
    pub id: i64,
    pub system_name: String,
    pub key_prefix: String,
    pub status: String,
    pub quota_limit: i64,
    pub quota_used: i64,
    pub created_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

impl From<ApiKey> for ApiKeyView {
    fn from(k: ApiKey) -> Self {
        Self {
            id: k.id,
            system_name: k.system_name,
            key_prefix: k.key_prefix,
            status: k.status,
            quota_limit: k.quota_limit,
            quota_used: k.quota_used,
            created_at: k.created_at,
            revoked_at: k.revoked_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKey {
    pub system_name: String,
    pub quota_limit: Option<i64>, // 默认 0 = 无限
    pub rate_limit_qps: Option<i64>,
}
