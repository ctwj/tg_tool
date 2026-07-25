//! 爬虫任务调度器（research.md R4）
//!
//! 仿现有 `src/services/scheduler.rs` 的 `SchedulerState` + `CancellationToken` 模式，
//! 30s tick，扫描 `status='active' AND next_run_at <= now()`，
//! 通过 `tokio::sync::Semaphore(全局并发上限)` 控制并发。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::state::{AppState, DbPool};

/// 调度器运行时状态
#[derive(Debug)]
pub struct CrawlerSchedulerState {
    pub running: bool,
    pub scan_interval_secs: u64,
    pub started_at: Option<std::time::Instant>,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
    /// 正在运行的任务 ID 集合（防同任务被并发 spawn 多份）
    pub running_tasks: Arc<Mutex<HashSet<i64>>>,
}

/// 全局共享句柄（参考现有 `SchedulerHandle` 模式）
pub type CrawlerSchedulerHandle = Arc<RwLock<CrawlerSchedulerState>>;

/// 创建调度器句柄（未启动状态）
pub fn create_scheduler() -> CrawlerSchedulerHandle {
    Arc::new(RwLock::new(CrawlerSchedulerState {
        running: false,
        scan_interval_secs: 30,
        started_at: None,
        handle: None,
        cancel: None,
        running_tasks: Arc::new(Mutex::new(HashSet::new())),
    }))
}

/// 启动调度器
///
/// - 若已在运行：直接返回
/// - 否则：spawn 一个 30s tick 的 worker，循环扫描到期任务
pub async fn start_scheduler(state: AppState) {
    let mut s = state.crawler_scheduler.write().await;
    if s.running {
        return;
    }
    let cancel = CancellationToken::new();
    s.cancel = Some(cancel.clone());
    s.running = true;
    s.started_at = Some(std::time::Instant::now());

    // 045：清理上次进程崩溃遗留的 status='running' 孤儿历史行
    cleanup_orphan_running(&state.db).await;

    let handle = tokio::spawn(run_loop(state.clone(), cancel));
    s.handle = Some(handle);
    tracing::info!("Crawler scheduler started (30s tick)");
}

/// 045：启动时清理上次进程崩溃遗留的 status='running' 孤儿历史行（标记为 failed）
async fn cleanup_orphan_running(db: &DbPool) {
    let now = chrono::Utc::now().naive_utc();
    let rows = match db {
        DbPool::Sqlite(pool) => sqlx::query(
            "UPDATE crawler_run_histories \
             SET status = 'failed', error_message = '进程未正常结束（启动清理）', finished_at = ? \
             WHERE status = 'running'",
        )
        .bind(now)
        .execute(pool)
        .await
        .map(|r| r.rows_affected())
        .unwrap_or(0),
        DbPool::Postgres(pool) => sqlx::query(
            "UPDATE crawler_run_histories \
             SET status = 'failed', error_message = '进程未正常结束（启动清理）', finished_at = $1 \
             WHERE status = 'running'",
        )
        .bind(now)
        .execute(pool)
        .await
        .map(|r| r.rows_affected())
        .unwrap_or(0),
    };
    if rows > 0 {
        tracing::info!(target: "crawler", "清理 {rows} 条孤儿 running 历史行（上次进程未正常结束）");
    }
}

async fn run_loop(state: AppState, cancel: CancellationToken) {
    let duration = Duration::from_secs(30);
    loop {
        tokio::select! {
            _ = tokio::time::sleep(duration) => {
                if let Err(e) = tick(&state).await {
                    tracing::warn!("Crawler scheduler tick error: {e}");
                }
            }
            _ = cancel.cancelled() => {
                tracing::info!("Crawler scheduler cancelled");
                break;
            }
        }
    }
}

/// 单次 tick：扫描到期任务，启动一次性 worker 推进抓取
///
/// 设计：
/// - 用 Semaphore 控制全局并发（默认 3，可配）
/// - 任务级并发通过 task_concurrency 字段控制（在 engine 内部 spawn）
/// - 抢不到 permit 的任务下次 tick 再试
async fn tick(state: &AppState) -> Result<(), String> {
    let due = fetch_due_tasks(&state.db).await?;
    if due.is_empty() {
        return Ok(());
    }

    let global_concurrency = global_concurrency(state).await;
    let sem = Arc::new(tokio::sync::Semaphore::new(global_concurrency as usize));

    let task_ids: Vec<(i64, String)> = due.iter().map(|t| (t.id, t.name.clone())).collect();
    tracing::info!(
        "Crawler tick: {} due tasks (global_concurrency={})",
        task_ids.len(),
        global_concurrency
    );

    // 运行中任务集合（防同任务被多个 tick 并发 spawn）
    let running_set = {
        let s = state.crawler_scheduler.read().await;
        s.running_tasks.clone()
    };

    for task in due {
        // 检查并原子标记 — 同任务已 spawn 则跳过
        {
            let mut guard = running_set.lock().await;
            if !guard.insert(task.id) {
                tracing::info!(
                    "Task {} ({}) still running, skip this tick",
                    task.id,
                    task.name
                );
                continue;
            }
        }
        // 045：DB 持久防重 — 手动 /run 触发的 running 行（内存 running_tasks 未记录）也跳过
        if crate::services::crawler::engine::is_task_running(&state.db, task.id).await {
            tracing::info!(
                "Task {} ({}) has a running history row (manual trigger or orphan), skip this tick",
                task.id,
                task.name
            );
            // 回退刚才的内存标记（这次不跑，等下次 tick）
            let mut guard = running_set.lock().await;
            guard.remove(&task.id);
            continue;
        }
        let sem = sem.clone();
        let state = state.clone();
        let task_id = task.id;
        let task_name = task.name.clone();
        let running_set_clone = running_set.clone();
        tokio::spawn(async move {
            // 用 async 块包装：确保任何退出路径（Ok/Err）都执行 finally 移除标记
            let inner = async {
                // 抢占全局并发 permit（无超时等待 — 下次 tick 自动补漏）
                let permit = match sem.acquire_owned().await {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("Task {task_id} ({task_name}) acquire permit failed: {e}");
                        return;
                    }
                };
                // 执行抓取
                let result = crate::services::crawler::engine::run_task(task_id, &state).await;
                match result {
                    Ok(summary) => {
                        tracing::info!(
                            "Task {task_id} ({task_name}) done: status={} crawled={} new={} failed={}",
                            summary.status,
                            summary.crawled_count,
                            summary.new_count,
                            summary.failed_count
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Task {task_id} ({task_name}) engine error: {e}");
                    }
                }
                drop(permit);
            };
            inner.await;
            // finally: 无论正常/异常都从运行集合移除（panic 时此处不执行，
            // 但 tokio task panic 罕见，且下次重启 recover 会重新调度）
            running_set_clone.lock().await.remove(&task_id);
        });
    }
    Ok(())
}

/// 任务调度用最小投影
#[derive(Debug, Clone, sqlx::FromRow)]
struct DueTask {
    id: i64,
    name: String,
}

async fn fetch_due_tasks(db: &DbPool) -> Result<Vec<DueTask>, String> {
    let now = chrono::Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(pool) => {
            let rows = sqlx::query_as::<_, DueTask>(
                "SELECT id, name FROM crawler_tasks \
                 WHERE enabled = 1 AND status = 'active' \
                 AND (next_run_at IS NULL OR next_run_at <= ?) \
                 ORDER BY next_run_at ASC NULLS FIRST",
            )
            .bind(now)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(rows)
        }
        DbPool::Postgres(pool) => {
            let rows = sqlx::query_as::<_, DueTask>(
                "SELECT id, name FROM crawler_tasks \
                 WHERE enabled = TRUE AND status = 'active' \
                 AND (next_run_at IS NULL OR next_run_at <= $1) \
                 ORDER BY next_run_at ASC NULLS FIRST",
            )
            .bind(now)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(rows)
        }
    }
}

async fn global_concurrency(state: &AppState) -> i64 {
    let cache = state.option_cache.read().await;
    cache
        .get("crawler_global_concurrency")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(3)
        .max(1)
}

/// 启动期：扫描所有 active 任务，重算 next_run_at = now() + interval_minutes
///
/// 防止重启后旧 next_run_at 已过导致立刻刷抓。
/// **只在 next_run_at 已过期或为 NULL 时才推进**——刚跑完任务的 next_run_at 在未来，
/// 不应被本函数覆盖（避免抹掉 finalize_run 写入的正确调度时间）。
pub async fn recover_active_tasks_schedule(state: &AppState) {
    let now = chrono::Utc::now().naive_utc();
    let updated = match &state.db {
        DbPool::Sqlite(pool) => {
            // SQLite datetime('now', '+N minutes') 返回 UTC ISO 字符串，与 NaiveDateTime 兼容
            sqlx::query(
                "UPDATE crawler_tasks \
                 SET next_run_at = datetime('now', '+' || interval_minutes || ' minutes') \
                 WHERE status = 'active' AND enabled = 1 \
                   AND (next_run_at IS NULL OR next_run_at <= ?)",
            )
            .bind(now)
            .execute(pool)
            .await
            .map(|r| r.rows_affected())
            .unwrap_or(0)
        }
        DbPool::Postgres(pool) => {
            // CURRENT_TIMESTAMP 返回 timestamptz，但 SQLite 列是 timestamp without tz，
            // Postgres 此处列也是 timestamp without tz —— 用 NOW() AT TIME ZONE 'UTC' 取 UTC naive
            sqlx::query(
                "UPDATE crawler_tasks \
                 SET next_run_at = (NOW() AT TIME ZONE 'UTC') + (interval_minutes || ' minutes')::INTERVAL \
                 WHERE status = 'active' AND enabled = TRUE \
                   AND (next_run_at IS NULL OR next_run_at <= $1)",
            )
            .bind(now)
            .execute(pool)
            .await
            .map(|r| r.rows_affected())
            .unwrap_or(0)
        }
    };
    if updated > 0 {
        tracing::info!(
            "Recovered {updated} active crawler tasks schedule (advanced next_run_at to now + interval)"
        );
    }
}
