use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;

pub async fn system_status(State(state): State<AppState>) -> impl IntoResponse {
    // 从数据库查询客户端状态（包含 Bot 类型，tg_clients 内存不含 Bot）
    // 同时用首次查询结果判断 DB 健康状态
    let mut db_ok = true;
    let (client_total, client_active): (i64, i64) = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clients")
                .fetch_one(pool)
                .await
                .unwrap_or_else(|e| {
                    db_ok = false;
                    tracing::warn!("DB health check failed: {e}");
                    0
                });
            let active: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM clients WHERE status = 'active'")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            (total, active)
        }
        crate::state::DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clients")
                .fetch_one(pool)
                .await
                .unwrap_or_else(|e| {
                    db_ok = false;
                    tracing::warn!("DB health check failed: {e}");
                    0
                });
            let active: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM clients WHERE status = 'active'")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            (total, active)
        }
    };

    // 规则和采集器统计
    let (rules_total, rules_active, collectors_total, collectors_active): (i64, i64, i64, i64) =
        match &state.db {
            crate::state::DbPool::Sqlite(pool) => {
                let rt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rules")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                let ra: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rules WHERE enabled = 1")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                let ct: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collectors")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                let ca: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM collectors WHERE enabled = 1")
                        .fetch_one(pool)
                        .await
                        .unwrap_or(0);
                (rt, ra, ct, ca)
            }
            crate::state::DbPool::Postgres(pool) => {
                let rt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rules")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                let ra: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rules WHERE enabled = true")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                let ct: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collectors")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                let ca: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM collectors WHERE enabled = true")
                        .fetch_one(pool)
                        .await
                        .unwrap_or(0);
                (rt, ra, ct, ca)
            }
        };

    let db_status = if db_ok { "ok" } else { "error" };

    // 调度器状态
    let extract_sched = state.extract_scheduler.read().await;
    let extract_interval = extract_sched.interval_minutes;
    let extract_next_run = if extract_sched.running {
        // 修正：last_run_at 在重启后为 None，回退到 started_at 作为基准
        let baseline = extract_sched.last_run_at.or(extract_sched.started_at);
        let interval_secs = extract_sched.interval_minutes * 60;
        let elapsed = baseline.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        // 取当前周期内的剩余时间（取模处理多次执行后的场景）
        let next_secs = interval_secs.saturating_sub(elapsed % interval_secs.max(1));
        Some(
            (chrono::Local::now() + chrono::Duration::seconds(next_secs as i64))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        )
    } else {
        None
    };
    drop(extract_sched);

    // 活跃自动推送配置数 — 用于前端区分"调度器循环在跑但无活跃配置"场景
    let push_active_configs: i64 = match &state.db {
        crate::state::DbPool::Sqlite(pool) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM push_configs WHERE is_active = 1 AND auto_push = 1",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0),
        crate::state::DbPool::Postgres(pool) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM push_configs WHERE is_active = TRUE AND auto_push = TRUE",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0),
    };

    let push_sched = state.scheduler.read().await;
    let push_interval = push_sched.interval_minutes;
    // T005 (US1): 系统扫描周期（秒），明确语义；前端据此展示"每 N 分钟扫描一次"
    let push_scan_interval_secs = push_sched.interval_minutes * 60;
    let push_running = push_sched.running;
    // clone Arc 引用 — 后续读 config_last_run 时短暂取锁，避免长期持锁阻塞 tick
    let config_last_run_arc = push_sched.config_last_run.clone();
    let push_next_run = if push_sched.running {
        // 修正：last_run_at 在重启后为 None，回退到 started_at 作为基准
        let baseline = push_sched.last_run_at.or(push_sched.started_at);
        let interval_secs = push_sched.interval_minutes * 60;
        let elapsed = baseline.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        let next_secs = interval_secs.saturating_sub(elapsed % interval_secs.max(1));
        Some(
            (chrono::Local::now() + chrono::Duration::seconds(next_secs as i64))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        )
    } else {
        None
    };
    drop(push_sched);

    // T005 (US1): 查询所有 active 自动推送配置（与 run_push_tick 同源），构造每配置调度视图。
    // 即使调度器未运行，只要 DB 中存在 active 配置就应展示（next_run/last_run_at 为 null），
    // 让用户从监控页能看到"有哪些 active 配置"。
    let active_push_configs: Vec<crate::models::push_config::PushConfig> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT * FROM push_configs WHERE is_active = 1 AND auto_push = 1 ORDER BY id ASC",
            )
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT * FROM push_configs WHERE is_active = TRUE AND auto_push = TRUE ORDER BY id ASC",
            )
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        }
    };
    let config_last_run_guard = config_last_run_arc.read().await;
    let push_configs_view: Vec<serde_json::Value> = active_push_configs
        .iter()
        .map(|c| {
            let interval_secs = (c.push_interval.max(0) as u64) * 60;
            // 调度器未运行 → last_run_at / next_run 都为 null（但仍展示该配置）
            let (last_run_at, next_run) = if !push_running {
                (None, None)
            } else {
                let last = config_last_run_guard.get(&c.id).copied();
                compute_last_and_next_run(last, interval_secs)
            };
            serde_json::json!({
                "id": c.id,
                "name": c.name,
                "push_interval": c.push_interval,
                "last_run_at": last_run_at,
                "next_run": next_run,
            })
        })
        .collect();
    drop(config_last_run_guard);

    let forward_sched = state.forward_scheduler.read().await;
    let forward_running = forward_sched.running;
    let forward_interval = forward_sched.interval_secs;
    drop(forward_sched);

    // Forward queue stats
    let (fwd_pending, fwd_forwarded, fwd_failed): (i64, i64, i64) = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let p: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM forward_tasks WHERE status = 'pending'")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let f: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM forward_tasks WHERE status = 'forwarded'")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let e: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM forward_tasks WHERE status = 'failed'")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            (p, f, e)
        }
        crate::state::DbPool::Postgres(pool) => {
            let p: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM forward_tasks WHERE status = 'pending'")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let f: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM forward_tasks WHERE status = 'forwarded'")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let e: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM forward_tasks WHERE status = 'failed'")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            (p, f, e)
        }
    };

    let body = Json(json!({
        "success": db_ok,
        "data": {
            "version": env!("CARGO_PKG_VERSION"),
            "db_status": db_status,
            "clients": {
                "total": client_total,
                "active": client_active,
            },
            "rules": {
                "total": rules_total,
                "active": rules_active,
            },
            "collectors": {
                "total": collectors_total,
                "active": collectors_active,
            },
            "schedulers": {
                "extract_running": extract_next_run.is_some(),
                "extract_next_run": extract_next_run,
                "extract_interval_minutes": extract_interval,
                "push_running": push_next_run.is_some(),
                "push_next_run": push_next_run,
                "push_interval_minutes": push_interval,
                "push_active_configs": push_active_configs,
                "push_scan_interval_secs": push_scan_interval_secs,
                "push_configs": push_configs_view,
                "forward_running": forward_running,
                "forward_interval_secs": forward_interval,
            },
            "forward_queue": {
                "pending": fwd_pending,
                "forwarded": fwd_forwarded,
                "failed": fwd_failed,
            }
        }
    }));

    if db_ok {
        (StatusCode::OK, body)
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, body)
    }
}

/// T006 (US1): 给定某推送配置的"上次推送 Instant"与"推送间隔秒数"，计算其
/// 本地时间字符串形式的 `last_run_at` 与 `next_run`。
///
/// 语义（与 contracts/status-api.md §3.2 对齐）：
/// - `last` 为 `None`（`config_last_run` 未初始化，全新/重启后未触发首次）→ `(None, None)`
/// - `last` 存在但已过期（`elapsed >= interval_secs`）→ `(last_local, now_local)`，
///   前端显示"即将执行"（下次 tick 立即触发）
/// - `last` 存在且未到期 → `(last_local, last + interval 的本地时间)`
///
/// `interval_secs` 为 0 时按 1 处理（防御，避免除零与负数剩余）。
fn compute_last_and_next_run(
    last: Option<std::time::Instant>,
    interval_secs: u64,
) -> (Option<String>, Option<String>) {
    let interval_secs = interval_secs.max(1) as i64;
    let last = match last {
        None => return (None, None),
        Some(t) => t,
    };
    let elapsed = last.elapsed().as_secs() as i64;
    let now = chrono::Local::now();
    let last_at = (now - chrono::Duration::seconds(elapsed))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let next_at = if elapsed >= interval_secs {
        // 已过期：下次 tick 立即触发，next_run = now（前端显示"即将执行"）
        now.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        let remaining = interval_secs - elapsed;
        (now + chrono::Duration::seconds(remaining))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    };
    (Some(last_at), Some(next_at))
}

#[cfg(test)]
mod tests {
    use super::compute_last_and_next_run;

    /// T006: 无 last_run（config_last_run 未初始化）→ (None, None)
    #[test]
    fn test_compute_last_and_next_run_no_last() {
        let (last, next) = compute_last_and_next_run(None, 60);
        assert!(last.is_none());
        assert!(next.is_none());
    }

    /// T006: 已过期（elapsed > interval）→ next_run = now，last_run_at 为反推的本地时间
    #[test]
    fn test_compute_last_and_next_run_expired() {
        // 1 小时前的 Instant（interval = 60 秒，必然已过期）
        let last = std::time::Instant::now() - std::time::Duration::from_secs(3600);
        let (last_at, next_at) = compute_last_and_next_run(Some(last), 60);
        assert!(
            last_at.is_some(),
            "last_run_at should be Some when last is Some"
        );
        assert!(
            next_at.is_some(),
            "next_run should be Some when last is Some"
        );
        // 已过期时 next_run ≈ now；这里仅校验格式（避免与本地时间硬比较）
        assert!(
            next_at.unwrap().contains(' '),
            "next_run should be formatted YYYY-MM-DD HH:MM:SS"
        );
    }

    /// T006: 未到期 → next_run 为 last + interval（剩余 > 0）
    #[test]
    fn test_compute_last_and_next_run_within_interval() {
        // 10 秒前的 Instant，interval = 60 秒 → 剩余 50 秒
        let last = std::time::Instant::now() - std::time::Duration::from_secs(10);
        let (last_at, next_at) = compute_last_and_next_run(Some(last), 60);
        assert!(last_at.is_some());
        assert!(next_at.is_some());
    }

    /// T006: interval_secs = 0 防御（按 1 处理，不 panic）
    #[test]
    fn test_compute_last_and_next_run_zero_interval_safe() {
        let last = std::time::Instant::now() - std::time::Duration::from_secs(10);
        // 不应 panic；按 interval=1 处理
        let (last_at, next_at) = compute_last_and_next_run(Some(last), 0);
        assert!(last_at.is_some());
        assert!(next_at.is_some());
    }
}
