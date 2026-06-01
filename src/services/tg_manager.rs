// Telegram client lifecycle management
// Manages multiple grammers-client instances using tokio tasks

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::errors::AppError;
use crate::config::Config;
use crate::state::TgClientEntry;

/// Manager for Telegram client instances
pub struct TgManager {
    config: Config,
    clients: Arc<RwLock<HashMap<String, TgClientEntry>>>,
}

impl TgManager {
    pub fn new(config: Config, clients: Arc<RwLock<HashMap<String, TgClientEntry>>>) -> Self {
        Self { config, clients }
    }

    /// Start a Telegram client by ID
    pub async fn start_client(&self, client_id: &str) -> Result<(), AppError> {
        // TODO: Implement with grammers-client
        // 1. Load session file for client_id
        // 2. Create grammers Client::connect()
        // 3. Spawn tokio task to handle updates
        // 4. Update status to "active"

        let mut clients = self.clients.write().await;
        clients.insert(
            client_id.to_string(),
            TgClientEntry {
                status: "active".to_string(),
                handle: None,
            },
        );

        tracing::info!("Started TG client {}", client_id);
        Ok(())
    }

    /// Stop a Telegram client by ID
    pub async fn stop_client(&self, client_id: &str) -> Result<(), AppError> {
        let mut clients = self.clients.write().await;
        if let Some(entry) = clients.get_mut(client_id) {
            if let Some(handle) = entry.handle.take() {
                handle.abort();
            }
            entry.status = "offline".to_string();
        }
        tracing::info!("Stopped TG client {}", client_id);
        Ok(())
    }

    /// Remove a client entirely
    pub async fn remove_client(&self, client_id: &str) -> Result<(), AppError> {
        self.stop_client(client_id).await?;
        let mut clients = self.clients.write().await;
        clients.remove(client_id);
        Ok(())
    }

    /// Get client status
    pub async fn get_status(&self, client_id: &str) -> String {
        let clients = self.clients.read().await;
        clients.get(client_id).map(|e| e.status.clone()).unwrap_or_else(|| "new".to_string())
    }

    /// Get the TG config
    pub fn config(&self) -> &Config {
        &self.config
    }
}
