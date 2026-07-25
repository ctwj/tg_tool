use dashmap::DashMap;
use sqlx::{Pool, Postgres, Sqlite};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Telegram 用户信息，认证成功后缓存
#[derive(Debug, Clone, serde::Serialize)]
pub struct UserInfo {
    pub user_id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub is_bot: bool,
}

/// 对话目标解析缓存 (chat_id -> (PackedChat, cached_at))
pub type PeerCache = Arc<RwLock<HashMap<i64, (grammers_client::types::PackedChat, Instant)>>>;

/// 验证码条目
#[derive(Debug, Clone)]
pub struct CaptchaEntry {
    pub answer: String,
    pub created_at: Instant,
}

/// 验证码存储 (captcha_key -> CaptchaEntry)
pub type CaptchaStore = Arc<DashMap<String, CaptchaEntry>>;

/// 登录失败追踪 (IP -> (fail_count, window_start))
pub type LoginAttempts = Arc<DashMap<String, (u32, Instant)>>;

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
    pub user_info: Option<UserInfo>,
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
            .field("has_user_info", &self.user_info.is_some())
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
    pub extract_scheduler: crate::services::scheduler::ExtractSchedulerHandle,
    pub forward_scheduler: crate::services::forward_queue::ForwardSchedulerHandle,
    /// 爬虫调度器（feature 042-web-crawler-collector）
    pub crawler_scheduler: crate::services::crawler::scheduler::CrawlerSchedulerHandle,
    /// 爬虫图片上传 worker（feature 042 T023）
    pub crawler_image_uploader: crate::services::crawler::image_uploader::ImageUploaderHandle,
    pub peer_cache: PeerCache,
    pub rate_limiter: crate::middleware::rate_limit::RateLimiter,
    pub captcha_store: CaptchaStore,
    pub login_attempts: LoginAttempts,
    /// 图片缓存目录路径
    pub image_cache_dir: PathBuf,
    /// 正在下载的图片 ID 标记（防并发重复下载）
    pub inflight_downloads: Arc<DashMap<String, Instant>>,
}

impl AppState {
    pub fn new(
        db: DbPool,
        config: crate::config::Config,
        tg_manager: std::sync::Arc<crate::services::tg_manager::TgManager>,
        image_cache_dir: PathBuf,
    ) -> Self {
        let rate_limiter = crate::middleware::rate_limit::RateLimiter::new(
            config.rate_limit_max,
            config.rate_limit_window_secs,
        );
        Self {
            db,
            config,
            tg_clients: tg_manager.clients().clone(),
            option_cache: tg_manager.option_cache().clone(),
            tg_manager,
            scheduler: crate::services::scheduler::create_scheduler(),
            extract_scheduler: crate::services::scheduler::create_extract_scheduler(),
            forward_scheduler: crate::services::forward_queue::create_forward_scheduler(),
            crawler_scheduler: crate::services::crawler::create_scheduler(),
            crawler_image_uploader: crate::services::crawler::image_uploader::create_uploader(),
            peer_cache: Arc::new(RwLock::new(HashMap::new())),
            rate_limiter,
            captcha_store: Arc::new(DashMap::new()),
            login_attempts: Arc::new(DashMap::new()),
            image_cache_dir,
            inflight_downloads: Arc::new(DashMap::new()),
        }
    }

    /// 获取 TG APP ID：优先使用系统配置，回退到环境变量
    pub async fn tg_app_id(&self) -> i32 {
        let cache = self.option_cache.read().await;
        if let Some(v) = cache.get("tg_app_id").and_then(|v| v.parse::<i32>().ok())
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

    /// 获取 HTTP 代理地址：用于 AI 提取等 HTTP API 请求
    pub async fn http_proxy_url(&self) -> Option<String> {
        let cache = self.option_cache.read().await;
        cache
            .get("http_proxy_url")
            .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
    }

    /// 获取 PanCheck 服务 Host（如 http://pancheck:6080）；空=未配置（不启用链接检测）
    pub async fn pancheck_host(&self) -> Option<String> {
        let cache = self.option_cache.read().await;
        cache
            .get("pancheck_host")
            .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
    }

    /// 获取链接检测并发数（默认 5，范围 1-20）
    pub async fn link_check_concurrency(&self) -> usize {
        let cache = self.option_cache.read().await;
        cache
            .get("link_check_concurrency")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(5)
            .clamp(1, 20)
    }

    /// 获取链接检测缓存 TTL（小时，默认 24，最小 1）
    pub async fn link_check_cache_ttl_hours(&self) -> i64 {
        let cache = self.option_cache.read().await;
        cache
            .get("link_check_cache_ttl_hours")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(24)
            .max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_info_full_serialization() {
        let info = UserInfo {
            user_id: 123456789,
            username: Some("test_user".to_string()),
            first_name: Some("John".to_string()),
            last_name: Some("Doe".to_string()),
            is_bot: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"user_id\":123456789"));
        assert!(json.contains("\"username\":\"test_user\""));
        assert!(json.contains("\"first_name\":\"John\""));
        assert!(json.contains("\"last_name\":\"Doe\""));
        assert!(json.contains("\"is_bot\":false"));
    }

    #[test]
    fn test_user_info_null_username() {
        let info = UserInfo {
            user_id: 998877,
            username: None,
            first_name: Some("Bot".to_string()),
            last_name: None,
            is_bot: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"user_id\":998877"));
        assert!(json.contains("\"username\":null"));
        assert!(json.contains("\"is_bot\":true"));
    }

    #[test]
    fn test_user_info_bot_serialization() {
        let info = UserInfo {
            user_id: 111222333,
            username: Some("my_bot".to_string()),
            first_name: None,
            last_name: None,
            is_bot: true,
        };
        let val: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert_eq!(val["is_bot"], true);
        assert_eq!(val["username"], "my_bot");
    }
}
