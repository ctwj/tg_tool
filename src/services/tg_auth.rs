// Telegram authentication flow using grammers-client
// This module handles the state machine for TG authentication:
// new → wait_phone → wait_code → wait_password → ready

use crate::errors::AppError;

/// Authentication state for a Telegram client
#[derive(Debug, Clone, PartialEq)]
pub enum AuthState {
    /// Initial state, no auth attempted
    Unauthenticated,
    /// Waiting for login code after phone number submitted
    WaitCode,
    /// Waiting for 2FA password
    WaitPassword,
    /// Successfully authenticated
    Ready,
}

/// Submit phone number to request login code
pub async fn request_login_code(
    _client_id: &str,
    _phone: &str,
    _app_id: i32,
    _app_hash: &str,
) -> Result<AuthState, AppError> {
    // TODO: Implement with grammers-client
    // 1. Create grammers client with session file
    // 2. Call client.request_login_code(phone, app_id, app_hash)
    // 3. Return WaitCode state
    tracing::info!("Requesting login code for client {}", _client_id);
    Ok(AuthState::WaitCode)
}

/// Submit verification code received via Telegram
pub async fn submit_code(
    _client_id: &str,
    _code: &str,
) -> Result<AuthState, AppError> {
    // TODO: Implement with grammers-client
    // 1. Get client from tg_manager
    // 2. Call client.sign_in(token, code)
    // 3. If password needed → return WaitPassword
    // 4. If success → return Ready
    tracing::info!("Submitting code for client {}", _client_id);
    Ok(AuthState::Ready)
}

/// Submit 2FA password
pub async fn submit_password(
    _client_id: &str,
    _password: &str,
) -> Result<AuthState, AppError> {
    // TODO: Implement with grammers-client
    // 1. Get client from tg_manager
    // 2. Call client.check_password(password)
    // 3. Return Ready on success
    tracing::info!("Submitting password for client {}", _client_id);
    Ok(AuthState::Ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_state_equality() {
        assert_eq!(AuthState::Unauthenticated, AuthState::Unauthenticated);
        assert_eq!(AuthState::WaitCode, AuthState::WaitCode);
        assert_eq!(AuthState::WaitPassword, AuthState::WaitPassword);
        assert_eq!(AuthState::Ready, AuthState::Ready);
    }

    #[test]
    fn test_auth_state_inequality() {
        assert_ne!(AuthState::Unauthenticated, AuthState::Ready);
        assert_ne!(AuthState::WaitCode, AuthState::WaitPassword);
    }

    #[test]
    fn test_auth_state_debug() {
        let state = AuthState::WaitCode;
        let debug = format!("{state:?}");
        assert!(debug.contains("WaitCode"));
    }

    #[tokio::test]
    async fn test_request_login_code_returns_wait_code() {
        let result = request_login_code("test-client", "+1234567890", 12345, "hash").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), AuthState::WaitCode);
    }

    #[tokio::test]
    async fn test_submit_code_returns_ready() {
        let result = submit_code("test-client", "12345").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), AuthState::Ready);
    }

    #[tokio::test]
    async fn test_submit_password_returns_ready() {
        let result = submit_password("test-client", "mypassword").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), AuthState::Ready);
    }
}
