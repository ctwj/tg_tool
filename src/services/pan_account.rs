// 网盘账号服务（feature 047 US1）— CRUD + 凭据加解密 + 健康校验
// 复用现有 DbPool（双库 match）、AppError、pan::credential / pan::quark

use chrono::Utc;

use crate::errors::AppError;
use crate::models::pan_account::{
    AccountDiagnosis, CapabilityLimitation, CreatePanAccount, DiagnoseFileItem, PanAccount,
    PanAccountView, UpdatePanAccount,
};
use crate::services::pan::credential;
use crate::state::DbPool;

const SUPPORTED_PLATFORMS: &[&str] = &["quark", "uc", "baidu"];
/// 首期仅夸克有驱动实现；uc/baidu 创建后标记 disabled（驱动待 US2+ 扩展）
const DRIVER_READY_PLATFORMS: &[&str] = &["quark"];

pub async fn list_accounts(db: &DbPool) -> Result<Vec<PanAccountView>, AppError> {
    let sql = "SELECT * FROM pan_accounts ORDER BY id DESC";
    let rows = match db {
        DbPool::Sqlite(p) => sqlx::query_as::<_, PanAccount>(sql).fetch_all(p).await?,
        DbPool::Postgres(p) => sqlx::query_as::<_, PanAccount>(sql).fetch_all(p).await?,
    };
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_account_view(db: &DbPool, id: i64) -> Result<PanAccountView, AppError> {
    Ok(get_account_full(db, id).await?.into())
}

/// 解密指定账号凭据（供 transfer 等服务复用，密文不外泄）
pub async fn get_decrypted_credential(
    db: &DbPool,
    pan_key: &str,
    id: i64,
) -> Result<String, AppError> {
    let acc = get_account_full(db, id).await?;
    credential::decrypt_credential(&acc.credential_cipher, &acc.credential_nonce, pan_key)
}

async fn get_account_full(db: &DbPool, id: i64) -> Result<PanAccount, AppError> {
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query_as::<_, PanAccount>("SELECT * FROM pan_accounts WHERE id = ?")
                .bind(id)
                .fetch_optional(p)
                .await?
        }
        DbPool::Postgres(p) => {
            sqlx::query_as::<_, PanAccount>("SELECT * FROM pan_accounts WHERE id = $1")
                .bind(id)
                .fetch_optional(p)
                .await?
        }
    }
    .ok_or_else(|| AppError::NotFound(format!("网盘账号 {id} 不存在")))
}

pub async fn create_account(
    db: &DbPool,
    pan_key: &str,
    req: CreatePanAccount,
) -> Result<PanAccountView, AppError> {
    credential::validate_pan_key(pan_key)?;
    validate_platform(&req.platform)?;
    if req.credential.trim().is_empty() {
        return Err(AppError::BadRequest("凭据不能为空".into()));
    }
    if req.target_dir.trim().is_empty() {
        return Err(AppError::BadRequest("目标目录不能为空".into()));
    }

    let driver_ready = DRIVER_READY_PLATFORMS.contains(&req.platform.as_str());
    // 驱动未实现的平台先 disabled，避免被误用
    let initial_status = if driver_ready { "active" } else { "disabled" };
    let (cipher, nonce) = credential::encrypt_credential(&req.credential, pan_key)?;

    let id: i64 = match db {
        DbPool::Sqlite(p) => {
            let res = sqlx::query(
                "INSERT INTO pan_accounts (platform, display_name, credential_cipher, credential_nonce, status, target_dir) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&req.platform)
            .bind(&req.display_name)
            .bind(&cipher)
            .bind(&nonce)
            .bind(initial_status)
            .bind(&req.target_dir)
            .execute(p)
            .await?;
            res.last_insert_rowid() as i64
        }
        DbPool::Postgres(p) => {
            let (id,): (i64,) = sqlx::query_as(
                "INSERT INTO pan_accounts (platform, display_name, credential_cipher, credential_nonce, status, target_dir) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
            )
            .bind(&req.platform)
            .bind(&req.display_name)
            .bind(&cipher)
            .bind(&nonce)
            .bind(initial_status)
            .bind(&req.target_dir)
            .fetch_one(p)
            .await?;
            id
        }
    };

    if !driver_ready {
        tracing::info!(
            "网盘账号 {id}({}) 创建成功，但该平台驱动暂未实现，标记 disabled",
            req.platform
        );
    } else if let Err(e) = refresh_health(db, pan_key, id).await {
        // 创建即健康校验失败（网络/解析）：不阻断创建，记录告警，保持 active 待后续校验
        tracing::warn!("网盘账号 {id} 创建后健康校验失败（不阻断创建）: {e}");
    }

    get_account_view(db, id).await
}

pub async fn update_account(
    db: &DbPool,
    pan_key: &str,
    id: i64,
    req: UpdatePanAccount,
) -> Result<PanAccountView, AppError> {
    let acc = get_account_full(db, id).await?;
    let display_name = req.display_name.unwrap_or(acc.display_name);
    let target_dir = req.target_dir.unwrap_or(acc.target_dir);
    let has_new_credential = req.credential.is_some();
    let (cipher, nonce) = if let Some(cred) = req.credential {
        credential::validate_pan_key(pan_key)?;
        if cred.trim().is_empty() {
            return Err(AppError::BadRequest("凭据不能为空".into()));
        }
        credential::encrypt_credential(&cred, pan_key)?
    } else {
        (acc.credential_cipher, acc.credential_nonce)
    };
    let now = Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query(
                "UPDATE pan_accounts SET display_name = ?, credential_cipher = ?, credential_nonce = ?, target_dir = ?, updated_at = ? WHERE id = ?",
            )
            .bind(&display_name)
            .bind(&cipher)
            .bind(&nonce)
            .bind(&target_dir)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
        DbPool::Postgres(p) => {
            sqlx::query(
                "UPDATE pan_accounts SET display_name = $1, credential_cipher = $2, credential_nonce = $3, target_dir = $4, updated_at = $5 WHERE id = $6",
            )
            .bind(&display_name)
            .bind(&cipher)
            .bind(&nonce)
            .bind(&target_dir)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
    };

    // 若更新了凭据且平台驱动就绪，重新健康校验
    if has_new_credential
        && DRIVER_READY_PLATFORMS.contains(&acc.platform.as_str())
        && let Err(e) = refresh_health(db, pan_key, id).await
    {
        tracing::warn!("网盘账号 {id} 更新凭据后健康校验失败: {e}");
    }
    get_account_view(db, id).await
}

pub async fn delete_account(db: &DbPool, id: i64) -> Result<(), AppError> {
    let affected = match db {
        DbPool::Sqlite(p) => sqlx::query("DELETE FROM pan_accounts WHERE id = ?")
            .bind(id)
            .execute(p)
            .await?
            .rows_affected(),
        DbPool::Postgres(p) => sqlx::query("DELETE FROM pan_accounts WHERE id = $1")
            .bind(id)
            .execute(p)
            .await?
            .rows_affected(),
    };
    if affected == 0 {
        return Err(AppError::NotFound(format!("网盘账号 {id} 不存在")));
    }
    Ok(())
}

/// 健康校验：解密凭据 → 调驱动校验 → 回写 status/capacity/last_checked_at。
/// 凭据失效是正常业务结果（status=expired），返回 view；网络/解析错误才报 AppError。
pub async fn check_account(
    db: &DbPool,
    pan_key: &str,
    id: i64,
) -> Result<PanAccountView, AppError> {
    let acc = get_account_full(db, id).await?;
    if !DRIVER_READY_PLATFORMS.contains(&acc.platform.as_str()) {
        return Err(AppError::BadRequest(format!(
            "平台 {} 的驱动暂未实现，无法健康校验",
            acc.platform
        )));
    }
    refresh_health(db, pan_key, id).await?;
    get_account_view(db, id).await
}

/// 综合诊断：cookie 有效性 + 容量 + 根目录样本 + 能力清单。
/// 比 check_account 更详细，但不写库（status/capacity 由 check_account 负责）。
/// 各步骤独立降级：health_check 失败仍返回诊断结构；list_files 失败仅置 root_files_ok=false。
pub async fn diagnose_account(
    db: &DbPool,
    pan_key: &str,
    id: i64,
) -> Result<AccountDiagnosis, AppError> {
    let acc = get_account_full(db, id).await?;
    let platform = acc.platform.clone();
    let cookie =
        credential::decrypt_credential(&acc.credential_cipher, &acc.credential_nonce, pan_key)?;

    // 步骤 1：health_check（cookie 有效性 + 容量）
    let (valid, message, capacity_bytes, used_capacity_bytes) = match platform.as_str() {
        "quark" => match crate::services::pan::quark::health_check(&cookie).await {
            Ok(h) => (h.valid, h.message, h.capacity_bytes, h.used_capacity_bytes),
            Err(e) => (false, Some(format!("健康校验异常: {e}")), None, None),
        },
        other => {
            return Err(AppError::BadRequest(format!(
                "平台 {other} 的驱动暂未实现，无法诊断"
            )));
        }
    };

    // 步骤 2：list_files 根目录（best-effort，失败不影响整体诊断）
    let (root_sample, root_total, root_ok, root_err) = if valid {
        match crate::services::pan::quark::list_files(&cookie, "0", 1, 10).await {
            Ok((items, total)) => (
                items
                    .into_iter()
                    .map(|f| DiagnoseFileItem {
                        fid: f.fid,
                        file_name: f.file_name,
                        is_dir: f.is_dir,
                        size: f.size,
                    })
                    .collect::<Vec<_>>(),
                total,
                true,
                None,
            ),
            Err(e) => (Vec::new(), 0, false, Some(format!("列根目录失败: {e}"))),
        }
    } else {
        (Vec::new(), 0, false, Some("Cookie 失效，跳过".into()))
    };

    Ok(AccountDiagnosis {
        account_id: id,
        platform: platform.clone(),
        valid,
        message,
        capacity_bytes,
        used_capacity_bytes,
        root_files_sample: root_sample,
        root_files_total: root_total,
        root_files_ok: root_ok,
        root_files_error: root_err,
        capabilities: platform_capabilities(&platform),
        unsupported: platform_unsupported(&platform),
    })
}

/// 各平台已实现能力清单（与 quark.rs 顶部注释一致）
fn platform_capabilities(platform: &str) -> Vec<String> {
    match platform {
        "quark" => vec![
            "health_check".into(),
            "transfer_share".into(),
            "create_share".into(),
            "upload_file".into(),
            "list_files".into(),
            "check_share_validity".into(),
            "check_instant_transfer".into(),
        ],
        _ => Vec::new(),
    }
}

/// 各平台受限/未实现能力（带原因，用于 UI 展示）
fn platform_unsupported(platform: &str) -> Vec<CapabilityLimitation> {
    match platform {
        "quark" => vec![CapabilityLimitation {
            capability: "offline_download".into(),
            reason: "夸克网页版已下线原生磁力链离线下载；社区逆向端点需 kps/sign/vcode 动态签名易失效。如需，建议走 aria2 委托模式（OpenList 路线）。".into(),
        }],
        "uc" => vec![CapabilityLimitation {
            capability: "all".into(),
            reason: "UC 网盘驱动尚未实现（占位）。API 与夸克同源（drive-pc.uc.cn），未来可快速接入。".into(),
        }],
        "baidu" => vec![CapabilityLimitation {
            capability: "all".into(),
            reason: "百度网盘驱动尚未实现（占位）。需适配百度 OAuth + bdstoken 双重校验。".into(),
        }],
        _ => Vec::new(),
    }
}

async fn refresh_health(db: &DbPool, pan_key: &str, id: i64) -> Result<(), AppError> {
    let acc = get_account_full(db, id).await?;
    let plain =
        credential::decrypt_credential(&acc.credential_cipher, &acc.credential_nonce, pan_key)?;
    let health = match acc.platform.as_str() {
        "quark" => crate::services::pan::quark::health_check(&plain).await?,
        other => return Err(AppError::Internal(format!("平台 {other} 驱动未实现"))),
    };
    let new_status = if health.valid { "active" } else { "expired" };
    let now = Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query(
                "UPDATE pan_accounts SET status = ?, capacity_bytes = ?, used_capacity_bytes = ?, last_checked_at = ?, updated_at = ? WHERE id = ?",
            )
            .bind(new_status)
            .bind(health.capacity_bytes)
            .bind(health.used_capacity_bytes)
            .bind(now)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
        DbPool::Postgres(p) => {
            sqlx::query(
                "UPDATE pan_accounts SET status = $1, capacity_bytes = $2, used_capacity_bytes = $3, last_checked_at = $4, updated_at = $5 WHERE id = $6",
            )
            .bind(new_status)
            .bind(health.capacity_bytes)
            .bind(health.used_capacity_bytes)
            .bind(now)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
    };
    if !health.valid {
        tracing::warn!("网盘账号 {id} 健康校验未通过: {:?}", health.message);
    }
    Ok(())
}

fn validate_platform(platform: &str) -> Result<(), AppError> {
    if !SUPPORTED_PLATFORMS.contains(&platform) {
        return Err(AppError::BadRequest(format!(
            "不支持的平台 {platform}，当前支持: quark/uc/baidu"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_platform_quark_ok() {
        assert!(validate_platform("quark").is_ok());
    }

    #[test]
    fn test_validate_platform_baidu_ok() {
        assert!(validate_platform("baidu").is_ok());
    }

    #[test]
    fn test_validate_platform_unknown_rejected() {
        assert!(validate_platform("onedrive").is_err());
    }

    #[test]
    fn test_driver_ready_only_quark() {
        assert!(DRIVER_READY_PLATFORMS.contains(&"quark"));
        assert!(!DRIVER_READY_PLATFORMS.contains(&"uc"));
        assert!(!DRIVER_READY_PLATFORMS.contains(&"baidu"));
    }

    #[test]
    fn platform_capabilities_quark_has_all_seven() {
        let caps = platform_capabilities("quark");
        assert!(caps.contains(&"health_check".to_string()));
        assert!(caps.contains(&"transfer_share".to_string()));
        assert!(caps.contains(&"create_share".to_string()));
        assert!(caps.contains(&"upload_file".to_string()));
        assert!(caps.contains(&"list_files".to_string()));
        assert!(caps.contains(&"check_share_validity".to_string()));
        assert!(caps.contains(&"check_instant_transfer".to_string()));
        // 离线下载不在已实现清单中（独立通过 unsupported 暴露）
        assert!(!caps.contains(&"offline_download".to_string()));
        assert_eq!(caps.len(), 7);
    }

    #[test]
    fn platform_capabilities_uc_baidu_empty() {
        // UC/百度驱动未实现 → 能力清单为空，由 unsupported.all 统一说明
        assert!(platform_capabilities("uc").is_empty());
        assert!(platform_capabilities("baidu").is_empty());
    }

    #[test]
    fn platform_unsupported_quark_lists_offline_download() {
        let unsup = platform_unsupported("quark");
        assert_eq!(unsup.len(), 1);
        assert_eq!(unsup[0].capability, "offline_download");
        // 原因包含关键提示词
        assert!(unsup[0].reason.contains("网页版"), "应说明网页版下线原因");
    }

    #[test]
    fn platform_unsupported_uc_baidu_mark_all_unimplemented() {
        // UC/百度整平台未实现 → unsupported[0].capability = "all"
        assert_eq!(
            platform_unsupported("uc")
                .first()
                .map(|c| c.capability.as_str()),
            Some("all")
        );
        assert_eq!(
            platform_unsupported("baidu")
                .first()
                .map(|c| c.capability.as_str()),
            Some("all")
        );
    }
}
