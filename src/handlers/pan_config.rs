// 网盘功能配置端点（feature 047 Polish T046）— 读写 option_cache，热生效
// 路由前缀 /api/pan/config（admin 层）

use crate::errors::AppError;
use crate::state::AppState;
use axum::{Json, extract::State};
use serde::Deserialize;
use serde_json::{Value, json};

const PAN_CONFIG_KEYS: &[&str] = &[
    "pan_task_retention_days",
    "pan_global_concurrency",
    "pan_per_account_qps",
    "pan_http_timeout_secs_metadata",
    "pan_http_timeout_secs_upload",
];

/// GET /api/pan/config
pub async fn get(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let cache = state.option_cache.read().await;
    let cfg: serde_json::Map<String, Value> = PAN_CONFIG_KEYS
        .iter()
        .map(|k| {
            (
                k.to_string(),
                Value::String(cache.get(*k).cloned().unwrap_or_default()),
            )
        })
        .collect();
    Ok(Json(json!({ "success": true, "data": cfg })))
}

#[derive(Deserialize)]
pub struct UpdateConfig {
    #[serde(default)]
    pub pan_task_retention_days: Option<String>,
    #[serde(default)]
    pub pan_global_concurrency: Option<String>,
    #[serde(default)]
    pub pan_per_account_qps: Option<String>,
    #[serde(default)]
    pub pan_http_timeout_secs_metadata: Option<String>,
    #[serde(default)]
    pub pan_http_timeout_secs_upload: Option<String>,
}

/// PUT /api/pan/config（option_cache 热生效，无需重启）
pub async fn update(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfig>,
) -> Result<Json<Value>, AppError> {
    let mut cache = state.option_cache.write().await;
    let updates = [
        ("pan_task_retention_days", req.pan_task_retention_days),
        ("pan_global_concurrency", req.pan_global_concurrency),
        ("pan_per_account_qps", req.pan_per_account_qps),
        (
            "pan_http_timeout_secs_metadata",
            req.pan_http_timeout_secs_metadata,
        ),
        (
            "pan_http_timeout_secs_upload",
            req.pan_http_timeout_secs_upload,
        ),
    ];
    for (k, v) in updates {
        if let Some(val) = v {
            if val.trim().is_empty() {
                cache.remove(k);
            } else {
                cache.insert(k.to_string(), val);
            }
        }
    }
    Ok(Json(
        json!({ "success": true, "message": "配置已更新（option_cache 热生效）" }),
    ))
}
