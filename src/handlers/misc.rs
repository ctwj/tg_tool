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

    let push_sched = state.scheduler.read().await;
    let push_interval = push_sched.interval_minutes;
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
