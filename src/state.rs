use std::collections::HashMap;
use std::sync::Arc;
use sqlx::{Pool, Sqlite, Postgres};
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
}
