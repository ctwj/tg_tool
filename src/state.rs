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
#[derive(Debug)]
pub struct TgClientEntry {
    pub status: String,
    pub handle: Option<tokio::task::JoinHandle<()>>,
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
}

impl AppState {
    pub fn new(db: DbPool, config: crate::config::Config) -> Self {
        Self {
            db,
            config,
            tg_clients: Arc::new(RwLock::new(HashMap::new())),
            option_cache: Arc::new(RwLock::new(HashMap::new())),
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
