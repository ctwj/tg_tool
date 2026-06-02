// Telegram client lifecycle management
// Manages multiple grammers-client instances using tokio tasks

use crate::config::Config;
use crate::errors::AppError;
use crate::state::{DbPool, OptionCache, PeerCache, TgClientEntry, TgClientMap};
use grammers_client::{Client, Config as GrammersConfig, InitParams};
use grammers_session::Session;
use std::path::Path;

/// Manager for Telegram client instances
pub struct TgManager {
    config: Config,
    db: DbPool,
    clients: TgClientMap,
    option_cache: OptionCache,
    peer_cache: PeerCache,
}

impl TgManager {
    pub fn new(
        config: Config,
        db: DbPool,
        clients: TgClientMap,
        option_cache: OptionCache,
        peer_cache: PeerCache,
    ) -> Self {
        Self {
            config,
            db,
            clients,
            option_cache,
            peer_cache,
        }
    }

    /// Get app_id from option cache or config
    async fn app_id(&self) -> i32 {
        let cache = self.option_cache.read().await;
        if let Some(v) = cache.get("tg_app_id").and_then(|v| v.parse::<i32>().ok())
            && v != 0 {
                return v;
            }
        self.config.tg_app_id
    }

    /// Get app_hash from option cache or config
    async fn app_hash(&self) -> String {
        let cache = self.option_cache.read().await;
        if let Some(v) = cache.get("tg_app_hash")
            && !v.is_empty() {
                return v.clone();
            }
        self.config.tg_app_hash.clone()
    }

    /// Get proxy URL from option cache
    async fn proxy_url(&self) -> Option<String> {
        let cache = self.option_cache.read().await;
        cache
            .get("proxy_url")
            .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
    }

    /// Ensure tg_store directory exists
    fn ensure_tg_store_dir() -> std::io::Result<()> {
        std::fs::create_dir_all("tg_store")
    }

    /// Start a Telegram client by ID
    pub async fn start_client(&self, client_id: &str) -> Result<String, AppError> {
        // Prevent duplicate start
        {
            let clients = self.clients.read().await;
            if let Some(entry) = clients.get(client_id)
                && entry.status == "active" {
                    return Ok("active".to_string());
                }
        }

        Self::ensure_tg_store_dir()
            .map_err(|e| AppError::Internal(format!("创建 tg_store 目录失败: {e}")))?;

        let session_path = format!("tg_store/{client_id}.session");
        let session = Session::load_file_or_create(&session_path)
            .map_err(|e| AppError::Internal(format!("加载 session 失败: {e}")))?;

        let app_id = self.app_id().await;
        let app_hash = self.app_hash().await;
        let _proxy_url = self.proxy_url().await;

        let mut params = InitParams {
            catch_up: true,
            ..Default::default()
        };

        // proxy feature 在 Cargo.toml 中始终启用
        params.proxy_url = _proxy_url;

        let client = Client::connect(GrammersConfig {
            session,
            api_id: app_id,
            api_hash: app_hash,
            params,
        })
        .await
        .map_err(|e| AppError::Internal(format!("连接 Telegram 失败: {e}")))?;

        // Check if already authorized
        let is_auth = client
            .is_authorized()
            .await
            .map_err(|e| AppError::Internal(format!("检查认证状态失败: {e}")))?;

        let status = if is_auth {
            // Already logged in, start update listener
            self.spawn_update_listener(client_id.to_string(), client.clone());
            "active".to_string()
        } else {
            "wait_code".to_string()
        };

        // Update in-memory state
        let mut clients = self.clients.write().await;
        clients.insert(
            client_id.to_string(),
            TgClientEntry {
                status: status.clone(),
                handle: None,
                client: Some(client),
                login_token: None,
                password_token: None,
                session_path,
                user_info: None,
            },
        );

        tracing::info!("Started TG client {} → status={}", client_id, status);
        Ok(status)
    }

    /// Stop a Telegram client by ID
    pub async fn stop_client(&self, client_id: &str) -> Result<(), AppError> {
        let mut clients = self.clients.write().await;
        if let Some(entry) = clients.get_mut(client_id) {
            if let Some(handle) = entry.handle.take() {
                handle.abort();
            }
            // grammers-client 0.7 没有 disconnect() 方法，drop 即可断开
            entry.client = None;
            entry.login_token = None;
            entry.password_token = None;
            entry.status = "offline".to_string();
        }
        tracing::info!("Stopped TG client {}", client_id);
        Ok(())
    }

    /// Remove a client entirely
    pub async fn remove_client(&self, client_id: &str) -> Result<(), AppError> {
        self.stop_client(client_id).await?;
        let mut clients = self.clients.write().await;
        if let Some(entry) = clients.remove(client_id) {
            // Clean up session file
            let path = &entry.session_path;
            if Path::new(path).exists()
                && let Err(e) = std::fs::remove_file(path) {
                    tracing::warn!("Failed to remove session file {}: {e}", path);
                }
        }
        Ok(())
    }

    /// Get a grammers Client reference
    pub async fn get_client(&self, client_id: &str) -> Result<Client, AppError> {
        let clients = self.clients.read().await;
        clients
            .get(client_id)
            .and_then(|e| e.client.clone())
            .ok_or_else(|| AppError::NotFound("客户端未连接".into()))
    }

    /// Get current client status
    pub async fn get_status(&self, client_id: &str) -> String {
        let clients = self.clients.read().await;
        clients
            .get(client_id)
            .map(|e| e.status.clone())
            .unwrap_or_else(|| "new".to_string())
    }

    /// Spawn update listener for a connected client
    pub fn spawn_update_listener(
        &self,
        client_id: String,
        client: Client,
    ) {
        let clients = self.clients.clone();
        let db = self.db.clone();
        let peer_cache = self.peer_cache.clone();
        let client_id_for_listener = client_id.clone();
        let client_id_for_handle = client_id.clone();

        let handle = tokio::spawn(async move {
            tracing::info!("Update listener started for client {}", client_id_for_listener);
            let mut consecutive_errors: u32 = 0;
            loop {
                // next_update with timeout — if proxy dies, this won't hang forever
                let update_result = tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    client.next_update(),
                ).await;

                match update_result {
                    Ok(Ok(update)) => {
                        consecutive_errors = 0;
                        match &update {
                            grammers_client::Update::NewMessage(msg) if !msg.outgoing() => {
                                let _ = crate::services::message_handler::handle_new_message(
                                    &client_id_for_listener,
                                    msg,
                                    &db,
                                    &clients,
                                    &peer_cache,
                                )
                                .await;
                            }
                            _ => {}
                        }
                    }
                    Ok(Err(e)) => {
                        consecutive_errors += 1;
                        tracing::warn!(
                            "Update listener error for client {} (attempt {}/5): {e}",
                            client_id_for_listener,
                            consecutive_errors,
                        );
                        if consecutive_errors >= 5 {
                            tracing::error!(
                                "Update listener giving up for client {} after 5 consecutive errors",
                                client_id_for_listener,
                            );
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                    Err(_) => {
                        // Timeout — next_update hung for 60s, verify connection with ping
                        tracing::warn!(
                            "Update listener timeout for client {}, checking connection...",
                            client_id_for_listener,
                        );
                        let ping_result = tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            client.invoke(&grammers_client::grammers_tl_types::functions::Ping { ping_id: 0 }),
                        ).await;

                        match ping_result {
                            Ok(Ok(_)) => {
                                // Ping succeeded, connection is alive, reset and continue
                                tracing::info!("Ping OK for client {}, continuing", client_id_for_listener);
                                consecutive_errors = 0;
                                continue;
                            }
                            _ => {
                                // Ping failed or timed out — connection is dead
                                consecutive_errors += 1;
                                tracing::warn!(
                                    "Connection check failed for client {} (attempt {}/3)",
                                    client_id_for_listener,
                                    consecutive_errors,
                                );
                                if consecutive_errors >= 3 {
                                    tracing::error!(
                                        "Connection lost for client {}, marking offline",
                                        client_id_for_listener,
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // Mark client as offline on disconnect
            let mut c = clients.write().await;
            if let Some(entry) = c.get_mut(&client_id_for_listener) {
                entry.status = "offline".to_string();
                entry.handle = None;
            }
            drop(c);

            // Sync offline status to database
            match &db {
                DbPool::Sqlite(pool) => {
                    if let Err(e) = sqlx::query("UPDATE clients SET status = 'offline', updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                        .bind(&client_id_for_listener)
                        .execute(pool)
                        .await
                    {
                        tracing::warn!("Failed to update client status in DB: {e}");
                    }
                }
                DbPool::Postgres(pool) => {
                    if let Err(e) = sqlx::query("UPDATE clients SET status = 'offline', updated_at = NOW() WHERE id = $1")
                        .bind(&client_id_for_listener)
                        .execute(pool)
                        .await
                    {
                        tracing::warn!("Failed to update client status in DB: {e}");
                    }
                }
            }

            tracing::info!("Update listener stopped for client {}", client_id_for_listener);
        });

        // Store the handle in a separate spawn to avoid blocking
        let clients_clone = self.clients.clone();
        tokio::spawn(async move {
            let mut c = clients_clone.write().await;
            if let Some(entry) = c.get_mut(&client_id_for_handle) {
                entry.handle = Some(handle);
            }
        });
    }

    /// Reconnect all active clients on server startup
    pub async fn reconnect_on_startup(&self) -> Vec<String> {
        let client_ids: Vec<String> = match &self.db {
            DbPool::Sqlite(pool) => {
                let rows: Vec<String> = sqlx::query_scalar(
                    "SELECT id FROM clients WHERE status = 'active'",
                )
                .fetch_all(pool)
                .await
                .unwrap_or_default();
                rows
            }
            DbPool::Postgres(pool) => {
                let rows: Vec<String> = sqlx::query_scalar(
                    "SELECT id FROM clients WHERE status = 'active'",
                )
                .fetch_all(pool)
                .await
                .unwrap_or_default();
                rows
            }
        };

        let mut reconnected = Vec::new();
        for id in client_ids {
            match self.start_client(&id).await {
                Ok(status) => {
                    tracing::info!("Reconnected client {} → {}", id, status);
                    reconnected.push(id);
                }
                Err(e) => {
                    tracing::warn!("Failed to reconnect client {}: {e}", id);
                }
            }
        }
        reconnected
    }

    /// Get the TG config
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get reference to tg_clients map
    pub fn clients(&self) -> &TgClientMap {
        &self.clients
    }

    /// Get reference to db pool
    pub fn db(&self) -> &DbPool {
        &self.db
    }

    /// Get reference to option cache
    pub fn option_cache(&self) -> &OptionCache {
        &self.option_cache
    }

    /// Gracefully shutdown all clients
    pub async fn graceful_shutdown(&self) {
        let mut clients = self.clients.write().await;
        let ids: Vec<String> = clients.keys().cloned().collect();
        for id in &ids {
            if let Some(entry) = clients.get_mut(id) {
                if let Some(handle) = entry.handle.take() {
                    handle.abort();
                }
                entry.client = None;
                entry.login_token = None;
                entry.password_token = None;
                entry.status = "offline".to_string();
                tracing::info!("Graceful shutdown: client {} disconnected", id);
            }
        }
        tracing::info!("All {} clients shut down gracefully", ids.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    async fn create_test_manager() -> TgManager {
        let config = Config::load();
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let option_cache = Arc::new(RwLock::new(HashMap::new()));
        let db = DbPool::Sqlite(
            sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .expect("Failed to create test DB"),
        );
        TgManager::new(config, db, clients, option_cache, Arc::new(RwLock::new(HashMap::new())))
    }

    #[tokio::test]
    async fn test_get_status_nonexistent() {
        let mgr = create_test_manager().await;
        let status = mgr.get_status("nonexistent").await;
        assert_eq!(status, "new");
    }

    #[tokio::test]
    async fn test_stop_client_nonexistent() {
        let mgr = create_test_manager().await;
        let result = mgr.stop_client("nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_remove_client_nonexistent() {
        let mgr = create_test_manager().await;
        let result = mgr.remove_client("nonexistent").await;
        assert!(result.is_ok());
    }
}
