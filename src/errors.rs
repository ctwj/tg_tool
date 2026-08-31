use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

/// 统一错误类型
#[derive(Debug)]
pub enum AppError {
    /// 400 Bad Request
    BadRequest(String),
    /// 401 Unauthorized
    Unauthorized(String),
    /// 403 Forbidden
    Forbidden(String),
    /// 404 Not Found
    NotFound(String),
    /// 500 Internal Server Error
    Internal(String),
    /// Database error
    Database(sqlx::Error),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::BadRequest(msg) => write!(f, "Bad Request: {msg}"),
            AppError::Unauthorized(msg) => write!(f, "Unauthorized: {msg}"),
            AppError::Forbidden(msg) => write!(f, "Forbidden: {msg}"),
            AppError::NotFound(msg) => write!(f, "Not Found: {msg}"),
            AppError::Internal(msg) => write!(f, "Internal Error: {msg}"),
            AppError::Database(e) => write!(f, "Database Error: {e}"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "数据库错误".to_string())
            }
        };

        let body = json!({
            "success": false,
            "message": message,
        });

        // 错误响应禁止缓存：防止 CDN（Cloudflare 等）把瞬时错误页
        // （如源站未就绪时的 404/5xx）长期缓存，修复后仍持续吐旧错误
        (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Database(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_bad_request() {
        let err = AppError::BadRequest("参数错误".to_string());
        assert_eq!(format!("{err}"), "Bad Request: 参数错误");
    }

    #[test]
    fn test_display_unauthorized() {
        let err = AppError::Unauthorized("未登录".to_string());
        assert_eq!(format!("{err}"), "Unauthorized: 未登录");
    }

    #[test]
    fn test_display_forbidden() {
        let err = AppError::Forbidden("无权限".to_string());
        assert_eq!(format!("{err}"), "Forbidden: 无权限");
    }

    #[test]
    fn test_display_not_found() {
        let err = AppError::NotFound("不存在".to_string());
        assert_eq!(format!("{err}"), "Not Found: 不存在");
    }

    #[test]
    fn test_display_internal() {
        let err = AppError::Internal("服务错误".to_string());
        assert_eq!(format!("{err}"), "Internal Error: 服务错误");
    }

    #[tokio::test]
    async fn test_into_response_status_codes() {
        // BadRequest → 400
        let resp = AppError::BadRequest("test".into()).into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Unauthorized → 401
        let resp = AppError::Unauthorized("test".into()).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Forbidden → 403
        let resp = AppError::Forbidden("test".into()).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // NotFound → 404
        let resp = AppError::NotFound("test".into()).into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Internal → 500
        let resp = AppError::Internal("test".into()).into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// 错误响应必须带 Cache-Control: no-store（防 CDN 长期缓存错误页）
    #[tokio::test]
    async fn test_error_response_no_store_header() {
        for resp in [
            AppError::BadRequest("t".into()).into_response(),
            AppError::NotFound("t".into()).into_response(),
            AppError::Internal("t".into()).into_response(),
        ] {
            assert_eq!(
                resp.headers()
                    .get(header::CACHE_CONTROL)
                    .and_then(|v| v.to_str().ok()),
                Some("no-store"),
                "错误响应缺 no-store 头"
            );
        }
    }
}
