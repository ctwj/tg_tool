use sqlx::{Pool, Postgres, Sqlite};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type alias for the database pool (supports both SQLite and Postgres)
#[derive(Clone)]
pub enum DbPool {
    Sqlite(Pool<Sqlite>),
    Postgres(Pool<Postgres>),
}

impl DbPool {
    pub fn sqlite(&self) -> Option<&Pool<Sqlite>> {
        match self {
            DbPool::Sqlite(p) => Some(p),
            _ => None,
        }
    }
    pub fn postgres(&self) -> Option<&Pool<Postgres>> {
        match self {
            DbPool::Postgres(p) => Some(p),
            _ => None,
        }
    }
}

/// Manages Telegram client instances (keyed by client ID string)
pub type TgClientMap = Arc<RwLock<HashMap<String, TgClientEntry>>>;

/// A single Telegram client entry
pub struct TgClientEntry {
    pub status: String,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub client: Option<grammers_client::Client>,
    pub login_token: Option<grammers_client::types::LoginToken>,
    pub password_token: Option<grammers_client::types::PasswordToken>,
    pub session_path: String,
}

impl std::fmt::Debug for TgClientEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TgClientEntry")
            .field("status", &self.status)
            .field("has_handle", &self.handle.is_some())
            .field("has_client", &self.client.is_some())
            .field("has_login_token", &self.login_token.is_some())
            .field("has_password_token", &self.password_token.is_some())
            .field("session_path", &self.session_path)
            .finish()
    }
}

/// System option cache (key -> value)
pub type OptionCache = Arc<RwLock<HashMap<String, String>>>;

/// Application shared state
#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub config: crate::config::Config,
    pub tg_clients: TgClientMap,
    pub option_cache: OptionCache,
    pub tg_manager: std::sync::Arc<crate::services::tg_manager::TgManager>,
    pub scheduler: crate::services::scheduler::SchedulerHandle,
}

impl AppState {
    pub fn new(
        db: DbPool,
        config: crate::config::Config,
        tg_manager: std::sync::Arc<crate::services::tg_manager::TgManager>,
    ) -> Self {
        Self {
            db,
            config,
            tg_clients: tg_manager.clients().clone(),
            option_cache: tg_manager.option_cache().clone(),
            tg_manager,
            scheduler: crate::services::scheduler::create_scheduler(),
        }
    }

    /// 获取 TG APP ID：优先使用系统配置，回退到环境变量
    pub async fn tg_app_id(&self) -> i32 {
        let cache = self.option_cache.read().await;
        if let Some(v) = cache
            .get("tg_app_id")
            .and_then(|v| v.parse::<i32>().ok())
            && v != 0
        {
            return v;
        }
        self.config.tg_app_id
    }

    /// 获取 TG APP Hash：优先使用系统配置，回退到环境变量
    pub async fn tg_app_hash(&self) -> String {
        let cache = self.option_cache.read().await;
        if let Some(v) = cache.get("tg_app_hash")
            && !v.is_empty()
        {
            return v.clone();
        }
        self.config.tg_app_hash.clone()
    }

    /// 获取代理地址：优先使用系统配置，回退到空
    pub async fn proxy_url(&self) -> Option<String> {
        let cache = self.option_cache.read().await;
        cache
            .get("proxy_url")
            .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
    }
}
