// Telegram authentication flow using grammers-client 0.7
// State machine: new → wait_code → wait_password → active

use crate::errors::AppError;
use crate::state::{DbPool, TgClientMap, UserInfo};

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
    client_id: &str,
    phone: &str,
    tg_clients: &TgClientMap,
    db: &DbPool,
) -> Result<AuthState, AppError> {
    let mut clients = tg_clients.write().await;
    let entry = clients
        .get_mut(client_id)
        .ok_or_else(|| AppError::NotFound("客户端不存在".into()))?;

    let client = entry
        .client
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("客户端未连接".into()))?;

    // grammers-client 0.7: request_login_code 只需 phone，api_hash 已在 Config 中
    let token = client
        .request_login_code(phone)
        .await
        .map_err(|e| AppError::Internal(format!("请求验证码失败: {e}")))?;

    entry.login_token = Some(token);
    entry.status = "wait_code".to_string();

    drop(clients);

    // Update DB status
    update_client_status(client_id, "wait_code", db).await?;

    tracing::info!("Login code requested for client {}", client_id);
    Ok(AuthState::WaitCode)
}

/// Submit verification code received via Telegram
pub async fn submit_code(
    client_id: &str,
    code: &str,
    tg_clients: &TgClientMap,
    db: &DbPool,
    tg_manager: &crate::services::tg_manager::TgManager,
) -> Result<AuthState, AppError> {
    let mut clients = tg_clients.write().await;
    let entry = clients
        .get_mut(client_id)
        .ok_or_else(|| AppError::NotFound("客户端不存在".into()))?;

    let token = entry
        .login_token
        .take()
        .ok_or_else(|| AppError::BadRequest("没有等待验证的登录请求".into()))?;

    let client = entry
        .client
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("客户端未连接".into()))?
        .clone();

    // Release write lock before potentially long operation
    drop(clients);

    match client.sign_in(&token, code).await {
        Ok(user) => {
            // Save session
            save_session(client_id, tg_clients).await?;
            update_client_status(client_id, "active", db).await?;

            // Update in-memory status, cache user info, and start update listener
            let mut clients = tg_clients.write().await;
            if let Some(e) = clients.get_mut(client_id) {
                e.status = "active".to_string();
                e.login_token = None;
                e.user_info = Some(extract_user_info(&user));
            }

            // Start update listener
            tg_manager.spawn_update_listener(client_id.to_string(), client);

            tracing::info!("Client {} authenticated successfully", client_id);
            Ok(AuthState::Ready)
        }
        Err(grammers_client::SignInError::PasswordRequired(password_token)) => {
            // Need 2FA password
            let mut clients = tg_clients.write().await;
            if let Some(e) = clients.get_mut(client_id) {
                e.password_token = Some(password_token);
                e.status = "wait_password".to_string();
            }

            update_client_status(client_id, "wait_password", db).await?;

            tracing::info!("Client {} requires 2FA password", client_id);
            Ok(AuthState::WaitPassword)
        }
        Err(grammers_client::SignInError::SignUpRequired { .. }) => Err(AppError::BadRequest(
            "该手机号需要先注册 Telegram 账号".into(),
        )),
        Err(grammers_client::SignInError::InvalidCode) => {
            // Restore login_token so user can retry
            let mut clients = tg_clients.write().await;
            if let Some(e) = clients.get_mut(client_id) {
                e.login_token = Some(token);
            }
            Err(AppError::BadRequest("验证码错误".into()))
        }
        Err(e) => Err(AppError::Internal(format!("验证码验证失败: {e}"))),
    }
}

/// Submit 2FA password
pub async fn submit_password(
    client_id: &str,
    password: &str,
    tg_clients: &TgClientMap,
    db: &DbPool,
    tg_manager: &crate::services::tg_manager::TgManager,
) -> Result<AuthState, AppError> {
    let mut clients = tg_clients.write().await;
    let entry = clients
        .get_mut(client_id)
        .ok_or_else(|| AppError::NotFound("客户端不存在".into()))?;

    let password_token = entry
        .password_token
        .take()
        .ok_or_else(|| AppError::BadRequest("没有等待验证的密码请求".into()))?;

    let client = entry
        .client
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("客户端未连接".into()))?
        .clone();

    drop(clients);

    match client
        .check_password(password_token.clone(), password.as_bytes())
        .await
    {
        Ok(user) => {
            save_session(client_id, tg_clients).await?;
            update_client_status(client_id, "active", db).await?;

            let mut clients = tg_clients.write().await;
            if let Some(e) = clients.get_mut(client_id) {
                e.status = "active".to_string();
                e.password_token = None;
                e.user_info = Some(extract_user_info(&user));
            }

            tg_manager.spawn_update_listener(client_id.to_string(), client);

            tracing::info!("Client {} 2FA authenticated successfully", client_id);
            Ok(AuthState::Ready)
        }
        Err(grammers_client::SignInError::InvalidPassword) => {
            // Restore password_token so user can retry
            let mut clients = tg_clients.write().await;
            if let Some(e) = clients.get_mut(client_id) {
                e.password_token = Some(password_token);
            }
            Err(AppError::BadRequest("密码错误".into()))
        }
        Err(e) => Err(AppError::Internal(format!("密码验证失败: {e}"))),
    }
}

/// Bot token authentication — 使用 Bot Token 直接登录
pub async fn bot_sign_in(
    client_id: &str,
    token: &str,
    tg_clients: &TgClientMap,
    db: &DbPool,
    tg_manager: &crate::services::tg_manager::TgManager,
) -> Result<AuthState, AppError> {
    let mut clients = tg_clients.write().await;
    let entry = clients
        .get_mut(client_id)
        .ok_or_else(|| AppError::NotFound("客户端不存在".into()))?;

    let client = entry
        .client
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("客户端未连接".into()))?
        .clone();

    drop(clients);

    match client.bot_sign_in(token).await {
        Ok(user) => {
            save_session(client_id, tg_clients).await?;
            update_client_status(client_id, "active", db).await?;

            let mut clients = tg_clients.write().await;
            if let Some(e) = clients.get_mut(client_id) {
                e.status = "active".to_string();
                e.user_info = Some(extract_user_info(&user));
            }

            tg_manager.spawn_update_listener(client_id.to_string(), client);

            tracing::info!("Bot client {} authenticated successfully", client_id);
            Ok(AuthState::Ready)
        }
        Err(e) => Err(AppError::Internal(format!("Bot 认证失败: {e}"))),
    }
}

/// Extract UserInfo from a grammers User object
fn extract_user_info(user: &grammers_client::types::User) -> UserInfo {
    UserInfo {
        user_id: user.id(),
        username: user.username().map(|s| s.to_string()),
        first_name: Some(user.first_name().to_string()),
        last_name: user.last_name().map(|s| s.to_string()),
        is_bot: user.is_bot(),
    }
}

/// Save session file after successful authentication
async fn save_session(client_id: &str, tg_clients: &TgClientMap) -> Result<(), AppError> {
    let clients = tg_clients.read().await;
    if let Some(entry) = clients.get(client_id)
        && let Some(client) = &entry.client
    {
        let path = &entry.session_path;
        client
            .session()
            .save_to_file(path)
            .map_err(|e| AppError::Internal(format!("保存 session 文件失败: {e}")))?;
        tracing::info!("Session saved for client {} to {}", client_id, path);
    }
    Ok(())
}

/// Update client status in database
async fn update_client_status(client_id: &str, status: &str, db: &DbPool) -> Result<(), AppError> {
    match db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE clients SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(status)
            .bind(client_id)
            .execute(pool)
            .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE clients SET status = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            )
            .bind(status)
            .bind(client_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::TgClientEntry;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

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

    #[test]
    fn test_auth_state_clone() {
        let state = AuthState::WaitPassword;
        let cloned = state.clone();
        assert_eq!(state, cloned);
    }

    // T014: request_login_code 错误路径测试
    #[tokio::test]
    async fn test_request_login_code_client_not_found() {
        let tg_clients: TgClientMap = Arc::new(RwLock::new(HashMap::new()));
        let db = DbPool::Sqlite(
            sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap(),
        );
        let result = request_login_code("nonexistent", "+123456", &tg_clients, &db).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            AppError::NotFound(msg) => assert!(msg.contains("客户端不存在")),
            _ => panic!("Expected NotFound error, got: {err}"),
        }
    }

    #[tokio::test]
    async fn test_request_login_code_client_not_connected() {
        let tg_clients: TgClientMap = Arc::new(RwLock::new(HashMap::new()));
        tg_clients.write().await.insert(
            "test_client".to_string(),
            TgClientEntry {
                status: "new".to_string(),
                handle: None,
                client: None, // 没有连接的 Client
                login_token: None,
                password_token: None,
                session_path: "tg_store/test.session".to_string(),
                user_info: None,
            },
        );
        let db = DbPool::Sqlite(
            sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap(),
        );
        let result = request_login_code("test_client", "+123456", &tg_clients, &db).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::BadRequest(msg) => assert!(msg.contains("客户端未连接")),
            _ => panic!("Expected BadRequest error"),
        }
    }

    // T015: submit_code 错误路径测试
    #[tokio::test]
    async fn test_submit_code_client_not_found() {
        let tg_clients: TgClientMap = Arc::new(RwLock::new(HashMap::new()));
        let db = DbPool::Sqlite(
            sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap(),
        );
        let config = crate::config::Config::load();
        let mgr = Arc::new(crate::services::tg_manager::TgManager::new(
            config,
            db.clone(),
            tg_clients.clone(),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
        ));
        let result = submit_code("nonexistent", "12345", &tg_clients, &db, &mgr).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::NotFound(msg) => assert!(msg.contains("客户端不存在")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_submit_code_no_login_token() {
        let tg_clients: TgClientMap = Arc::new(RwLock::new(HashMap::new()));
        tg_clients.write().await.insert(
            "test_client".to_string(),
            TgClientEntry {
                status: "wait_code".to_string(),
                handle: None,
                client: None,
                login_token: None, // 没有 login_token
                password_token: None,
                session_path: "tg_store/test.session".to_string(),
                user_info: None,
            },
        );
        let db = DbPool::Sqlite(
            sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap(),
        );
        let config = crate::config::Config::load();
        let mgr = Arc::new(crate::services::tg_manager::TgManager::new(
            config,
            db.clone(),
            tg_clients.clone(),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
        ));
        let result = submit_code("test_client", "12345", &tg_clients, &db, &mgr).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::BadRequest(msg) => assert!(msg.contains("没有等待验证的登录请求")),
            _ => panic!("Expected BadRequest error"),
        }
    }

    // T016: submit_password 错误路径测试
    #[tokio::test]
    async fn test_submit_password_no_password_token() {
        let tg_clients: TgClientMap = Arc::new(RwLock::new(HashMap::new()));
        tg_clients.write().await.insert(
            "test_client".to_string(),
            TgClientEntry {
                status: "wait_password".to_string(),
                handle: None,
                client: None,
                login_token: None,
                password_token: None, // 没有 password_token
                session_path: "tg_store/test.session".to_string(),
                user_info: None,
            },
        );
        let db = DbPool::Sqlite(
            sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap(),
        );
        let config = crate::config::Config::load();
        let mgr = Arc::new(crate::services::tg_manager::TgManager::new(
            config,
            db.clone(),
            tg_clients.clone(),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
        ));
        let result = submit_password("test_client", "mypass", &tg_clients, &db, &mgr).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::BadRequest(msg) => assert!(msg.contains("没有等待验证的密码请求")),
            _ => panic!("Expected BadRequest error"),
        }
    }

    // T024: bot_sign_in 错误路径测试
    #[tokio::test]
    async fn test_bot_sign_in_client_not_found() {
        let tg_clients: TgClientMap = Arc::new(RwLock::new(HashMap::new()));
        let db = DbPool::Sqlite(
            sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap(),
        );
        let config = crate::config::Config::load();
        let mgr = Arc::new(crate::services::tg_manager::TgManager::new(
            config,
            db.clone(),
            tg_clients.clone(),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
        ));
        let result = bot_sign_in("nonexistent", "bot:token", &tg_clients, &db, &mgr).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::NotFound(msg) => assert!(msg.contains("客户端不存在")),
            _ => panic!("Expected NotFound error"),
        }
    }
}
