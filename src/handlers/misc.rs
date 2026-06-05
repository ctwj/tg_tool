use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;

pub async fn system_status(State(state): State<AppState>) -> impl IntoResponse {
    let tg_clients = state.tg_clients.read().await;
    let total = tg_clients.len();
    let active = tg_clients.values().filter(|e| e.status == "active").count();
    drop(tg_clients);

    // Database health check
    let db_ok = check_db_health(&state.db).await;
    let db_status = if db_ok { "ok" } else { "error" };

    let body = Json(json!({
        "success": db_ok,
        "data": {
            "version": env!("CARGO_PKG_VERSION"),
            "db_status": db_status,
            "clients": {
                "total": total,
                "active": active,
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
