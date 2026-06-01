use crate::state::AppState;
use axum::{Json, extract::State};
use serde_json::{Value, json};

pub async fn system_status(State(state): State<AppState>) -> Json<Value> {
    let tg_clients = state.tg_clients.read().await;
    let total = tg_clients.len();
    let active = tg_clients.values().filter(|e| e.status == "active").count();

    Json(json!({
        "success": true,
        "data": {
            "version": env!("CARGO_PKG_VERSION"),
            "clients": {
                "total": total,
                "active": active,
            }
        }
    }))
}
