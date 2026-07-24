// 转存/上传任务编排（feature 047 US2）— 幂等创建 + 状态机执行 + 夸克转存/分享
// 异步执行：handler 用 tokio::spawn 调 run_task（首期简单 spawn；FR-019 限流/worker 池后续完善）

use chrono::Utc;

use crate::errors::AppError;
use crate::models::transfer_task::{
    self, CreateTransferTask, TransferTask, STATUS_FAILED, STATUS_PENDING, STATUS_PROCESSING,
    STATUS_SUCCEEDED,
};
use crate::services::{link_checker, link_parser, pan::quark, pan_account, share, staging};
use crate::state::DbPool;
use std::path::Path;

/// 创建任务（幂等：同源同目标返回既有任务）
pub async fn create_task(
    db: &DbPool,
    source_origin: &str,
    req: CreateTransferTask,
) -> Result<TransferTask, AppError> {
    let parsed = link_parser::parse(&req.source_url, req.extract_code.as_deref());
    if parsed.source_type == link_parser::SourceType::Unknown {
        return Err(AppError::BadRequest("无法识别的链接类型".into()));
    }
    let source_type = parsed.source_type.as_str();

    if !account_exists(db, req.target_account_id).await? {
        return Err(AppError::BadRequest(format!(
            "目标账号 {} 不存在",
            req.target_account_id
        )));
    }

    let key = transfer_task::idempotency_key(&req.source_url, req.target_account_id, source_type);
    if let Some(existing) = find_by_idem(db, &key).await? {
        return Ok(existing);
    }

    let id: i64 = match db {
        DbPool::Sqlite(p) => {
            let res = sqlx::query(
                "INSERT INTO transfer_tasks (source_url, source_type, source_platform, extract_code, target_account_id, status, source_origin, idempotency_key) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&req.source_url)
            .bind(source_type)
            .bind(parsed.platform.as_deref())
            .bind(req.extract_code.as_deref())
            .bind(req.target_account_id)
            .bind(STATUS_PENDING)
            .bind(source_origin)
            .bind(&key)
            .execute(p)
            .await?;
            res.last_insert_rowid() as i64
        }
        DbPool::Postgres(p) => {
            let (id,): (i64,) = sqlx::query_as(
                "INSERT INTO transfer_tasks (source_url, source_type, source_platform, extract_code, target_account_id, status, source_origin, idempotency_key) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
            )
            .bind(&req.source_url)
            .bind(source_type)
            .bind(parsed.platform.as_deref())
            .bind(req.extract_code.as_deref())
            .bind(req.target_account_id)
            .bind(STATUS_PENDING)
            .bind(source_origin)
            .bind(&key)
            .fetch_one(p)
            .await?;
            id
        }
    };
    get_task(db, id).await
}

pub async fn get_task(db: &DbPool, id: i64) -> Result<TransferTask, AppError> {
    match db {
        DbPool::Sqlite(p) => sqlx::query_as::<_, TransferTask>(
            "SELECT * FROM transfer_tasks WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(p)
        .await?,
        DbPool::Postgres(p) => sqlx::query_as::<_, TransferTask>(
            "SELECT * FROM transfer_tasks WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(p)
        .await?,
    }
    .ok_or_else(|| AppError::NotFound(format!("转存任务 {id} 不存在")))
}

/// 执行任务：错误统一转 failed，不向上传播（供 spawn 调用）
pub async fn run_task(
    db: &DbPool,
    pan_key: &str,
    staging_dir: &Path,
    option_cache: &crate::state::OptionCache,
    task_id: i64,
) -> Result<(), AppError> {
    if let Err(e) = run_task_inner(db, pan_key, staging_dir, option_cache, task_id).await {
        let msg = format!("{e}");
        let _ = mark_failed(db, task_id, &msg).await;
        tracing::error!("转存任务 {task_id} 失败: {msg}");
    }
    Ok(())
}

async fn run_task_inner(
    db: &DbPool,
    pan_key: &str,
    staging_dir: &Path,
    option_cache: &crate::state::OptionCache,
    task_id: i64,
) -> Result<(), AppError> {
    mark_processing(db, task_id).await?;
    let task = get_task(db, task_id).await?;
    let account = pan_account::get_account_view(db, task.target_account_id).await?;
    let cookie = pan_account::get_decrypted_credential(db, pan_key, task.target_account_id).await?;

    match (account.platform.as_str(), task.source_type.as_str()) {
        ("quark", "pan_share") => {
            // 转存前 PanCheck 预判链接有效性（FR-015，协同 spec 022）；未配置则跳过
            if let Ok(Some(checker)) = link_checker::resolve_checker(option_cache).await
                && let Ok(verdicts) = checker.check(std::slice::from_ref(&task.source_url)).await
                && let Some(v) = verdicts.first()
                && v.status == link_checker::LinkStatus::Invalid
            {
                return Err(AppError::BadRequest(format!(
                    "源链接已失效（PanCheck）: {}",
                    v.fail_reason.as_deref().unwrap_or("分享失效")
                )));
            }
            let parsed = link_parser::parse(&task.source_url, task.extract_code.as_deref());
            let pwd_id = parsed
                .pwd_id
                .ok_or_else(|| AppError::Internal("缺少 pwd_id".into()))?;
            let saved =
                quark::transfer_share(&cookie, &pwd_id, parsed.passcode.as_deref(), &account.target_dir)
                    .await?;
            if saved.is_empty() {
                return Err(AppError::Internal("转存返回空文件列表".into()));
            }
            let fid_list: Vec<String> = saved.iter().map(|f| f.fid.clone()).collect();
            let title = saved
                .iter()
                .map(|f| f.file_name.clone())
                .collect::<Vec<_>>()
                .join(",");
            let remote_id = saved.first().map(|f| f.fid.clone());
            let (share_url, share_pwd) = quark::create_share(&cookie, &fid_list, &title, None).await?;
            let share_id =
                share::create(db, account.id, &title, &share_url, share_pwd, remote_id).await?;
            mark_succeeded(db, task_id, share_id).await?;
        }
        ("quark", "direct_link") => {
            // 直链下载到中转 → 夸克分片上传 → 生成分享
            let filename = staging::extract_filename(&task.source_url, task_id);
            let path = staging::download_to_staging(&task.source_url, task_id, staging_dir).await?;
            let fid = quark::upload_file(&cookie, &path, &account.target_dir, &filename).await;
            staging::cleanup(&path).await; // 无论成败清理中转
            let fid = fid?;
            let fid_list = vec![fid.clone()];
            let (share_url, share_pwd) =
                quark::create_share(&cookie, &fid_list, &filename, None).await?;
            let share_id =
                share::create(db, account.id, &filename, &share_url, share_pwd, Some(fid)).await?;
            mark_succeeded(db, task_id, share_id).await?;
        }
        (_, "direct_link") => {
            return Err(AppError::Internal(format!(
                "平台 {} 直链上传驱动未实现",
                account.platform
            )));
        }
        (plat, _) => {
            return Err(AppError::Internal(format!("平台 {plat} 转存驱动未实现")));
        }
    }
    Ok(())
}

async fn account_exists(db: &DbPool, id: i64) -> Result<bool, AppError> {
    let (c,): (i64,) = match db {
        DbPool::Sqlite(p) => {
            sqlx::query_as("SELECT COUNT(*) FROM pan_accounts WHERE id = ?")
                .bind(id)
                .fetch_one(p)
                .await?
        }
        DbPool::Postgres(p) => {
            sqlx::query_as("SELECT COUNT(*) FROM pan_accounts WHERE id = $1")
                .bind(id)
                .fetch_one(p)
                .await?
        }
    };
    Ok(c > 0)
}

async fn find_by_idem(db: &DbPool, key: &str) -> Result<Option<TransferTask>, AppError> {
    Ok(match db {
        DbPool::Sqlite(p) => sqlx::query_as::<_, TransferTask>(
            "SELECT * FROM transfer_tasks WHERE idempotency_key = ?",
        )
        .bind(key)
        .fetch_optional(p)
        .await?,
        DbPool::Postgres(p) => sqlx::query_as::<_, TransferTask>(
            "SELECT * FROM transfer_tasks WHERE idempotency_key = $1",
        )
        .bind(key)
        .fetch_optional(p)
        .await?,
    })
}

async fn mark_processing(db: &DbPool, id: i64) -> Result<(), AppError> {
    let now = Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query(
                "UPDATE transfer_tasks SET status = ?, started_at = COALESCE(started_at, ?) WHERE id = ?",
            )
            .bind(STATUS_PROCESSING)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
        DbPool::Postgres(p) => {
            sqlx::query(
                "UPDATE transfer_tasks SET status = $1, started_at = COALESCE(started_at, $2) WHERE id = $3",
            )
            .bind(STATUS_PROCESSING)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
    }
    Ok(())
}

async fn mark_succeeded(db: &DbPool, id: i64, share_id: i64) -> Result<(), AppError> {
    let now = Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query(
                "UPDATE transfer_tasks SET status = ?, share_id = ?, completed_at = ? WHERE id = ?",
            )
            .bind(STATUS_SUCCEEDED)
            .bind(share_id)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
        DbPool::Postgres(p) => {
            sqlx::query(
                "UPDATE transfer_tasks SET status = $1, share_id = $2, completed_at = $3 WHERE id = $4",
            )
            .bind(STATUS_SUCCEEDED)
            .bind(share_id)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
    }
    Ok(())
}

async fn mark_failed(db: &DbPool, id: i64, reason: &str) -> Result<(), AppError> {
    let now = Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query(
                "UPDATE transfer_tasks SET status = ?, failure_reason = ?, completed_at = ? WHERE id = ?",
            )
            .bind(STATUS_FAILED)
            .bind(reason)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
        DbPool::Postgres(p) => {
            sqlx::query(
                "UPDATE transfer_tasks SET status = $1, failure_reason = $2, completed_at = $3 WHERE id = $4",
            )
            .bind(STATUS_FAILED)
            .bind(reason)
            .bind(now)
            .bind(id)
            .execute(p)
            .await?;
        }
    }
    Ok(())
}

fn sanitize_status(s: &str) -> Option<&'static str> {
    match s {
        "pending" => Some("pending"),
        "processing" => Some("processing"),
        "succeeded" => Some("succeeded"),
        "failed" => Some("failed"),
        _ => None,
    }
}

/// 任务列表（分页 + status/account_id 筛选）
pub async fn list_tasks(
    db: &DbPool,
    status: Option<&str>,
    account_id: Option<i64>,
    page: i64,
    page_size: i64,
) -> Result<(Vec<TransferTask>, i64), AppError> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let offset = (page - 1) * page_size;
    let mut where_sql = String::from("WHERE 1=1");
    if let Some(s) = status.and_then(sanitize_status) {
        where_sql.push_str(&format!(" AND status = '{s}'")); // 白名单，防注入
    }
    if let Some(aid) = account_id {
        where_sql.push_str(&format!(" AND target_account_id = {aid}"));
    }
    let items = match db {
        DbPool::Sqlite(p) => sqlx::query_as::<_, TransferTask>(&format!(
            "SELECT * FROM transfer_tasks {where_sql} ORDER BY id DESC LIMIT ? OFFSET ?"
        ))
        .bind(page_size)
        .bind(offset)
        .fetch_all(p)
        .await?,
        DbPool::Postgres(p) => sqlx::query_as::<_, TransferTask>(&format!(
            "SELECT * FROM transfer_tasks {where_sql} ORDER BY id DESC LIMIT $1 OFFSET $2"
        ))
        .bind(page_size)
        .bind(offset)
        .fetch_all(p)
        .await?,
    };
    let (total,): (i64,) = match db {
        DbPool::Sqlite(p) => {
            sqlx::query_as(&format!("SELECT COUNT(*) FROM transfer_tasks {where_sql}"))
                .fetch_one(p)
                .await?
        }
        DbPool::Postgres(p) => {
            sqlx::query_as(&format!("SELECT COUNT(*) FROM transfer_tasks {where_sql}"))
                .fetch_one(p)
                .await?
        }
    };
    Ok((items, total))
}

/// 重试：failed → pending，retry_count++，清空 failure_reason/时间
pub async fn retry_task(db: &DbPool, id: i64) -> Result<TransferTask, AppError> {
    let task = get_task(db, id).await?;
    if task.status != STATUS_FAILED {
        return Err(AppError::BadRequest("仅 failed 任务可重试".into()));
    }
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query(
                "UPDATE transfer_tasks SET status = 'pending', failure_reason = NULL, started_at = NULL, completed_at = NULL, retry_count = retry_count + 1 WHERE id = ?",
            )
            .bind(id)
            .execute(p)
            .await?;
        }
        DbPool::Postgres(p) => {
            sqlx::query(
                "UPDATE transfer_tasks SET status = 'pending', failure_reason = NULL, started_at = NULL, completed_at = NULL, retry_count = retry_count + 1 WHERE id = $1",
            )
            .bind(id)
            .execute(p)
            .await?;
        }
    };
    get_task(db, id).await
}

/// 清理过期 succeeded/failed 任务（share_records 保留，分享不随清理失效）
pub async fn cleanup_expired(db: &DbPool, retention_days: i64) -> Result<i64, AppError> {
    let days = retention_days.max(1);
    let deleted = match db {
        DbPool::Sqlite(p) => sqlx::query(
            "DELETE FROM transfer_tasks WHERE status IN ('succeeded','failed') AND created_at < datetime('now', ? || ' days')",
        )
        .bind(-days)
        .execute(p)
        .await?
        .rows_affected(),
        DbPool::Postgres(p) => sqlx::query(
            "DELETE FROM transfer_tasks WHERE status IN ('succeeded','failed') AND created_at < NOW() - ($1 || ' days')::interval",
        )
        .bind(days)
        .execute(p)
        .await?
        .rows_affected(),
    };
    Ok(deleted as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use sqlx::sqlite::SqlitePoolOptions;
    use crate::models::transfer_task::{
        CreateTransferTask, SOURCE_ORIGIN_MANUAL, STATUS_FAILED, STATUS_PENDING,
    };
    use crate::state::DbPool;

    fn pan_key() -> String {
        base64::engine::general_purpose::STANDARD.encode([0x42u8; 32])
    }

    fn empty_cache() -> crate::state::OptionCache {
        std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()))
    }

    async fn setup() -> DbPool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let m = include_str!("../../migrations/040_pan_management_sqlite.sql");
        sqlx::raw_sql(m).execute(&pool).await.unwrap();
        DbPool::Sqlite(pool)
    }

    async fn make_uc_account(db: &DbPool) -> i64 {
        let v = crate::services::pan_account::create_account(
            db,
            &pan_key(),
            crate::models::pan_account::CreatePanAccount {
                platform: "uc".into(),
                display_name: "UC".into(),
                credential: "cookie".into(),
                target_dir: "0".into(),
            },
        )
        .await
        .unwrap();
        v.id
    }

    #[tokio::test]
    async fn test_create_task_pan_share_pending() {
        let db = setup().await;
        let acc_id = make_uc_account(&db).await;
        let req = CreateTransferTask {
            source_url: "https://pan.quark.cn/s/abcdef?pwd=xxxx".into(),
            extract_code: None,
            target_account_id: acc_id,
        };
        let t = create_task(&db, SOURCE_ORIGIN_MANUAL, req).await.unwrap();
        assert_eq!(t.status, STATUS_PENDING);
        assert_eq!(t.source_type, "pan_share");
        assert_eq!(t.source_platform.as_deref(), Some("quark"));
    }

    #[tokio::test]
    async fn test_create_task_idempotent() {
        let db = setup().await;
        let acc_id = make_uc_account(&db).await;
        let mk = || CreateTransferTask {
            source_url: "https://pan.quark.cn/s/shareid?pwd=pp".into(),
            extract_code: None,
            target_account_id: acc_id,
        };
        let t1 = create_task(&db, SOURCE_ORIGIN_MANUAL, mk()).await.unwrap();
        let t2 = create_task(&db, SOURCE_ORIGIN_MANUAL, mk()).await.unwrap();
        assert_eq!(t1.id, t2.id, "同源同目标应幂等返回同一任务");
    }

    #[tokio::test]
    async fn test_create_task_rejects_unknown_link() {
        let db = setup().await;
        let acc_id = make_uc_account(&db).await;
        let req = CreateTransferTask {
            source_url: "not-a-link".into(),
            extract_code: None,
            target_account_id: acc_id,
        };
        assert!(create_task(&db, SOURCE_ORIGIN_MANUAL, req).await.is_err());
    }

    #[tokio::test]
    async fn test_run_task_unsupported_platform_marks_failed_no_network() {
        // uc 目标账号驱动未实现 → run_task 标记 failed，不触发夸克网络
        let db = setup().await;
        let acc_id = make_uc_account(&db).await;
        let t = create_task(
            &db,
            SOURCE_ORIGIN_MANUAL,
            CreateTransferTask {
                source_url: "https://pan.quark.cn/s/abcdef".into(),
                extract_code: None,
                target_account_id: acc_id,
            },
        )
        .await
        .unwrap();
        run_task(
            &db,
            &pan_key(),
            std::path::Path::new("/tmp/tgtool-staging-test"),
            &empty_cache(),
            t.id,
        )
        .await
        .unwrap();
        let after = get_task(&db, t.id).await.unwrap();
        assert_eq!(after.status, STATUS_FAILED);
        assert!(after.failure_reason.as_deref().unwrap_or("").contains("uc"));
    }

    #[tokio::test]
    async fn test_list_tasks_filter_and_pagination() {
        let db = setup().await;
        let acc = make_uc_account(&db).await;
        for s in ["aaa", "bbb", "ccc"] {
            create_task(
                &db,
                SOURCE_ORIGIN_MANUAL,
                CreateTransferTask {
                    source_url: format!("https://pan.quark.cn/s/{s}"),
                    extract_code: None,
                    target_account_id: acc,
                },
            )
            .await
            .unwrap();
        }
        // run 最新一个 → uc failed
        let (items, _) = list_tasks(&db, None, None, 1, 10).await.unwrap();
        run_task(&db, &pan_key(), std::path::Path::new("/tmp/x"), &empty_cache(), items[0].id).await.unwrap();

        let (_, total) = list_tasks(&db, None, None, 1, 10).await.unwrap();
        assert_eq!(total, 3);
        let (_, t_failed) = list_tasks(&db, Some("failed"), None, 1, 10).await.unwrap();
        assert_eq!(t_failed, 1);
        let (_, t_pending) = list_tasks(&db, Some("pending"), None, 1, 10).await.unwrap();
        assert_eq!(t_pending, 2);
    }

    #[tokio::test]
    async fn test_retry_failed_resets_to_pending() {
        let db = setup().await;
        let acc = make_uc_account(&db).await;
        let t = create_task(
            &db,
            SOURCE_ORIGIN_MANUAL,
            CreateTransferTask {
                source_url: "https://pan.quark.cn/s/retrytest".into(),
                extract_code: None,
                target_account_id: acc,
            },
        )
        .await
        .unwrap();
        run_task(&db, &pan_key(), std::path::Path::new("/tmp/x"), &empty_cache(), t.id).await.unwrap(); // uc → failed
        assert_eq!(get_task(&db, t.id).await.unwrap().status, STATUS_FAILED);
        let retried = retry_task(&db, t.id).await.unwrap();
        assert_eq!(retried.status, STATUS_PENDING);
        assert_eq!(retried.retry_count, 1);
        assert!(retried.failure_reason.is_none());
    }

    #[tokio::test]
    async fn test_retry_non_failed_rejected() {
        let db = setup().await;
        let acc = make_uc_account(&db).await;
        let t = create_task(
            &db,
            SOURCE_ORIGIN_MANUAL,
            CreateTransferTask {
                source_url: "https://pan.quark.cn/s/notfailed".into(),
                extract_code: None,
                target_account_id: acc,
            },
        )
        .await
        .unwrap(); // pending
        assert!(retry_task(&db, t.id).await.is_err());
    }

    #[tokio::test]
    async fn test_cleanup_expired_deletes_old_keeps_share() {
        let db = setup().await;
        let acc = make_uc_account(&db).await;
        let pool = match &db {
            DbPool::Sqlite(p) => p,
            _ => unreachable!(),
        };
        // 过期 succeeded
        sqlx::query(
            "INSERT INTO transfer_tasks (source_url, source_type, target_account_id, status, source_origin, idempotency_key, created_at) VALUES ('old','pan_share',?, 'succeeded','manual','k1', datetime('now','-100 days'))",
        )
        .bind(acc)
        .execute(pool)
        .await
        .unwrap();
        // 新 pending
        sqlx::query(
            "INSERT INTO transfer_tasks (source_url, source_type, target_account_id, status, source_origin, idempotency_key) VALUES ('new','pan_share',?, 'pending','manual','k2')",
        )
        .bind(acc)
        .execute(pool)
        .await
        .unwrap();

        let deleted = cleanup_expired(&db, 90).await.unwrap();
        assert_eq!(deleted, 1);
        let (_, total) = list_tasks(&db, None, None, 1, 10).await.unwrap();
        assert_eq!(total, 1); // 剩新 pending
    }
}
