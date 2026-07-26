use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 网盘账号 — 凭据 AES-256-GCM 加密存储（feature 047）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PanAccount {
    pub id: i64,
    pub platform: String, // quark | uc | baidu
    pub display_name: String,
    pub credential_cipher: String,
    pub credential_nonce: String,
    pub status: String, // active | disabled | expired
    pub target_dir: String,
    pub capacity_bytes: Option<i64>,
    pub used_capacity_bytes: Option<i64>,
    pub last_checked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 账号脱敏视图 — 用于 API 响应/列表，不含密文（FR-002）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanAccountView {
    pub id: i64,
    pub platform: String,
    pub display_name: String,
    pub status: String,
    pub target_dir: String,
    pub capacity_bytes: Option<i64>,
    pub used_capacity_bytes: Option<i64>,
    pub last_checked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<PanAccount> for PanAccountView {
    fn from(a: PanAccount) -> Self {
        Self {
            id: a.id,
            platform: a.platform,
            display_name: a.display_name,
            status: a.status,
            target_dir: a.target_dir,
            capacity_bytes: a.capacity_bytes,
            used_capacity_bytes: a.used_capacity_bytes,
            last_checked_at: a.last_checked_at,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

/// 创建账号请求 — credential 为明文（服务层加密后落库）
#[derive(Debug, Deserialize)]
pub struct CreatePanAccount {
    pub platform: String,
    pub display_name: String,
    pub credential: String,
    pub target_dir: String,
}

/// 更新账号请求 — 字段均可选，credential 非空时重新加密
#[derive(Debug, Deserialize, Default)]
pub struct UpdatePanAccount {
    pub display_name: Option<String>,
    pub credential: Option<String>,
    pub target_dir: Option<String>,
}
