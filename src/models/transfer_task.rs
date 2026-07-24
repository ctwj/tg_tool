use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 转存/上传任务（状态机 + 幂等，feature 047）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TransferTask {
    pub id: i64,
    pub source_url: String,
    pub source_type: String, // pan_share | direct_link
    pub source_platform: Option<String>,
    pub extract_code: Option<String>,
    pub target_account_id: i64,
    pub status: String,
    pub failure_reason: Option<String>,
    pub share_id: Option<i64>,
    pub source_origin: String, // manual | api | resource_integration
    pub idempotency_key: String,
    pub retry_count: i64,
    pub created_at: NaiveDateTime,
    pub started_at: Option<NaiveDateTime>,
    pub completed_at: Option<NaiveDateTime>,
}

/// 创建转存任务请求（手动/API 共用）
#[derive(Debug, Deserialize)]
pub struct CreateTransferTask {
    pub source_url: String,
    pub extract_code: Option<String>,
    pub target_account_id: i64,
}

// 状态机常量
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_PROCESSING: &str = "processing";
pub const STATUS_SUCCEEDED: &str = "succeeded";
pub const STATUS_FAILED: &str = "failed";

pub const SOURCE_ORIGIN_MANUAL: &str = "manual";
pub const SOURCE_ORIGIN_API: &str = "api";

/// 计算幂等键：归一化(source_url + target_account_id + source_type)
pub fn idempotency_key(source_url: &str, target_account_id: i64, source_type: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(source_url.trim().as_bytes());
    h.update(b"|");
    h.update(target_account_id.to_le_bytes());
    h.update(b"|");
    h.update(source_type.as_bytes());
    let digest = h.finalize();
    format!("{:x}", digest)
}
