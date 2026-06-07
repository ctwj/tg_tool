use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;

pub async fn system_status(State(state): State<AppState>) -> impl IntoResponse {
    // 从数据库查询客户端状态（包含 Bot 类型，tg_clients 内存不含 Bot）
    let (client_total, client_active): (i64, i64) = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clients")
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clients WHERE status = 'active'")
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            (total, active)
        }
        crate::state::DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clients")
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM clients WHERE status = 'active'")
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
                let ca: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collectors WHERE enabled = 1")
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
                let ca: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collectors WHERE enabled = true")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                (rt, ra, ct, ca)
            }
        };

    // Database health check
    let db_ok = check_db_health(&state.db).await;
    let db_status = if db_ok { "ok" } else { "error" };

    // 调度器状态
    let extract_sched = state.extract_scheduler.read().await;
    let extract_next_run = if extract_sched.running {
        let elapsed = extract_sched.last_run_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        let next_secs = (extract_sched.interval_minutes * 60).saturating_sub(elapsed);
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
    let push_next_run = if push_sched.running {
        let elapsed = push_sched.last_run_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        let next_secs = (push_sched.interval_minutes * 60).saturating_sub(elapsed);
        Some(
            (chrono::Local::now() + chrono::Duration::seconds(next_secs as i64))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        )
    } else {
        None
    };
    drop(push_sched);

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
                "push_running": push_next_run.is_some(),
                "push_next_run": push_next_run,
            }
        }
    }));

    if db_ok {
        (StatusCode::OK, body)
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, body)
    }
}

/// Ping database with SELECT 1
async fn check_db_health(db: &crate::state::DbPool) -> bool {
    match db {
        crate::state::DbPool::Sqlite(pool) => sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(pool)
            .await
            .is_ok(),
        crate::state::DbPool::Postgres(pool) => sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(pool)
            .await
            .is_ok(),
    }
}
