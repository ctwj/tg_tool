// 开放 API 鉴权中间件（feature 047 US4）— X-API-Key 校验 + 配额消费

use axum::{
    Json,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::errors::AppError;
use crate::state::AppState;

pub async fn api_key_middleware(req: Request, next: Next) -> Response {
    let state = match req.extensions().get::<AppState>().cloned() {
        Some(s) => s,
        None => {
            return AppError::Internal("服务器配置错误：AppState 缺失".into()).into_response();
        }
    };
    let key = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let Some(key) = key else {
        return AppError::Unauthorized("缺少 X-API-Key 请求头".into()).into_response();
    };

    let api_key = match crate::services::api_key::validate(&state.db, &key).await {
        Ok(k) => k,
        Err(e) => return e.into_response(), // 401 无效/已吊销
    };

    match crate::services::api_key::consume_quota(&state.db, api_key.id).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"success": false, "message": "API 调用配额已用尽"})),
            )
                .into_response();
        }
        Err(e) => return e.into_response(),
    }

    let mut req = req;
    req.extensions_mut().insert(api_key);
    next.run(req).await
}
