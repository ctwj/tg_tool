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

/// 诊断结果（综合检测：cookie 有效性 + 容量 + 根目录样本 + 能力清单）
/// 用于 /api/pan/accounts/:id/diagnose 端点，比 check 更详细，便于管理员一次性验证全部能力
#[derive(Debug, Clone, Serialize)]
pub struct AccountDiagnosis {
    pub account_id: i64,
    pub platform: String,
    pub valid: bool,
    pub message: Option<String>,
    /// 总容量（字节）
    pub capacity_bytes: Option<i64>,
    /// 已用容量（字节）
    pub used_capacity_bytes: Option<i64>,
    /// 根目录前 N 个文件样本（best-effort，失败则空数组）
    pub root_files_sample: Vec<DiagnoseFileItem>,
    /// 根目录文件总数（metadata._total，失败为 0）
    pub root_files_total: u64,
    /// 根目录列文件是否成功
    pub root_files_ok: bool,
    /// 根目录列文件失败原因（root_files_ok=false 时填充）
    pub root_files_error: Option<String>,
    /// 该平台当前已实现的能力清单
    pub capabilities: Vec<String>,
    /// 未实现/受限的能力（带原因说明）
    pub unsupported: Vec<CapabilityLimitation>,
}

/// 诊断样本文件项（精简版 FileInfo，仅用于展示）
#[derive(Debug, Clone, Serialize)]
pub struct DiagnoseFileItem {
    pub fid: String,
    pub file_name: String,
    pub is_dir: bool,
    pub size: i64,
}

/// 能力受限说明（如离线下载因夸克网页版下线而未实现）
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityLimitation {
    pub capability: String,
    pub reason: String,
}
