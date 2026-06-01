use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::errors::AppError;
use crate::models::user::User;
use crate::state::AppState;

/// Extract user from Authorization header or session cookie
pub async fn extract_current_user(state: &AppState, req: &Request) -> Result<User, AppError> {
    // Try Authorization header first
    if let Some(auth_header) = req.headers().get("Authorization") {
        let auth_str = auth_header
            .to_str()
            .map_err(|_| AppError::Unauthorized("Invalid auth header".into()))?;

        if let Some(token) = auth_str.strip_prefix("Bearer ") {
            return validate_token(state, token).await;
        }
    }

    // Try session cookie
    if let Some(cookie) = req.headers().get("cookie") {
        let cookie_str = cookie
            .to_str()
            .map_err(|_| AppError::Unauthorized("Invalid cookie".into()))?;
        if let Some(token) = extract_session_token(cookie_str) {
            return validate_token(state, &token).await;
        }
    }

    Err(AppError::Unauthorized("未登录".into()))
}

fn extract_session_token(cookie_str: &str) -> Option<String> {
    for part in cookie_str.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("session_token=") {
            return Some(val.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_session_token_found() {
        let cookies = "theme=dark; session_token=abc123xyz; lang=zh";
        let result = extract_session_token(cookies);
        assert_eq!(result, Some("abc123xyz".to_string()));
    }

    #[test]
    fn test_extract_session_token_first() {
        let cookies = "session_token=mytoken";
        let result = extract_session_token(cookies);
        assert_eq!(result, Some("mytoken".to_string()));
    }

    #[test]
    fn test_extract_session_token_not_found() {
        let cookies = "theme=dark; lang=zh";
        let result = extract_session_token(cookies);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_session_token_empty() {
        let result = extract_session_token("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_session_token_with_spaces() {
        let cookies = "  session_token = spaced  ; other=val";
        // This should NOT match because strip_prefix is exact
        let result = extract_session_token(cookies);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_session_token_trailing() {
        let cookies = "a=1; session_token=tok123";
        let result = extract_session_token(cookies);
        assert_eq!(result, Some("tok123".to_string()));
    }
}

async fn validate_token(state: &AppState, token: &str) -> Result<User, AppError> {
    use crate::services::crypto;

    let claims = crypto::verify_token_with_secret(token, &state.config.session_secret)?;

    let user = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ? AND status = 1")
                .bind(claims.sub)
                .fetch_optional(pool)
                .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 AND status = 1")
                .bind(claims.sub)
                .fetch_optional(pool)
                .await?
        }
    };

    user.ok_or_else(|| AppError::Unauthorized("用户不存在或已禁用".into()))
}

/// User auth middleware (role >= 1)
/// Gets AppState from request extensions (set via Extension layer)
pub async fn user_auth_middleware(req: Request, next: Next) -> Response {
    let state = req
        .extensions()
        .get::<AppState>()
        .cloned()
        .expect("AppState extension not configured");

    match extract_current_user(&state, &req).await {
        Ok(user) => {
            let mut req = req;
            req.extensions_mut().insert(user);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}
