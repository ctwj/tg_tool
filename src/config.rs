use clap::Parser;
use std::path::PathBuf;

/// Telegram 消息转发工具
#[derive(Parser, Debug, Clone)]
#[command(name = "tgTool", about = "Telegram 消息转发工具", version)]
pub struct Config {
    /// 服务端口
    #[arg(long, default_value = "3000", env = "PORT")]
    pub port: u16,

    /// 日志目录
    #[arg(long, env = "LOG_DIR")]
    pub log_dir: Option<PathBuf>,

    /// 日志级别
    #[arg(long, default_value = "info", env = "RUST_LOG")]
    pub rust_log: String,

    /// Telegram 客户端存储路径
    #[arg(long, default_value = "./tg_store", env = "TG_STORE")]
    pub tg_store: PathBuf,

    /// Telegram APP ID
    #[arg(long, default_value = "0", env = "TG_APP_ID")]
    pub tg_app_id: i32,

    /// Telegram APP Hash
    #[arg(long, default_value = "", env = "TG_APP_HASH")]
    pub tg_app_hash: String,

    /// 数据库连接串（留空使用 SQLite）
    #[arg(long, default_value = "", env = "SQL_DSN")]
    pub sql_dsn: String,

    /// Redis 连接串（留空使用内存存储）
    #[arg(long, default_value = "", env = "REDIS_CONN_STRING")]
    pub redis_conn_string: String,

    /// Session 密钥（启动期强制校验：非空 / 非默认值 / ≥32 字符；feature 027 SEC-001）
    #[arg(long, default_value = "", env = "SESSION_SECRET")]
    pub session_secret: String,

    /// 单 IP 速率限制（请求数/窗口期）
    #[arg(long, default_value = "100", env = "RATE_LIMIT_MAX")]
    pub rate_limit_max: usize,

    /// 速率限制窗口（秒）
    #[arg(long, default_value = "60", env = "RATE_LIMIT_WINDOW")]
    pub rate_limit_window_secs: u64,
}

impl Config {
    pub fn load() -> Self {
        // Load .env file (ignore error if not found)
        let _ = dotenvy::dotenv();

        let config = Config::parse();

        // feature 027 SEC-001：启动期强制校验 SESSION_SECRET（拒绝空 / 默认值 / 过短）
        // 生产环境 fail-fast；测试环境仅提示（测试不模拟生产 secret，
        // 校验逻辑由 validate_session_secret 单测覆盖，见 SC-001）
        if let Err(msg) = crate::services::crypto::validate_session_secret(&config.session_secret) {
            if cfg!(not(test)) {
                panic!("启动失败 — {msg}");
            } else {
                eprintln!("[test] SESSION_SECRET 校验跳过: {msg}");
            }
        }

        // Ensure tg_store directory exists
        if !config.tg_store.exists() {
            std::fs::create_dir_all(&config.tg_store).expect("Failed to create tg_store directory");
        }

        config
    }

    /// Get the database URL. Falls back to SQLite if SQL_DSN is empty.
    pub fn database_url(&self) -> String {
        if self.sql_dsn.is_empty() {
            "sqlite:./data.db?mode=rwc".to_string()
        } else {
            self.sql_dsn.clone()
        }
    }

    /// Check if using PostgreSQL
    pub fn is_postgres(&self) -> bool {
        self.sql_dsn.starts_with("postgres")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(sql_dsn: &str) -> Config {
        Config {
            port: 3000,
            log_dir: None,
            rust_log: "info".to_string(),
            tg_store: PathBuf::from("./tg_store"),
            tg_app_id: 12345,
            tg_app_hash: "testhash".to_string(),
            sql_dsn: sql_dsn.to_string(),
            redis_conn_string: String::new(),
            session_secret: "test-secret-0123456789abcdef0123456789".to_string(),
            rate_limit_max: 100,
            rate_limit_window_secs: 60,
        }
    }

    #[test]
    fn test_database_url_default_sqlite() {
        let config = make_config("");
        let url = config.database_url();
        assert_eq!(url, "sqlite:./data.db?mode=rwc");
    }

    #[test]
    fn test_database_url_postgres_dsn() {
        let config = make_config("postgres://user:pass@localhost/db");
        let url = config.database_url();
        assert_eq!(url, "postgres://user:pass@localhost/db");
    }

    #[test]
    fn test_database_url_sqlite_dsn() {
        let config = make_config("sqlite:./custom.db?mode=rwc");
        let url = config.database_url();
        assert_eq!(url, "sqlite:./custom.db?mode=rwc");
    }

    #[test]
    fn test_is_postgres_true() {
        let config = make_config("postgres://user:pass@host/db");
        assert!(config.is_postgres());
    }

    #[test]
    fn test_is_postgres_false_empty() {
        let config = make_config("");
        assert!(!config.is_postgres());
    }

    #[test]
    fn test_is_postgres_false_sqlite() {
        let config = make_config("sqlite:./test.db");
        assert!(!config.is_postgres());
    }

    #[test]
    fn test_is_postgres_postgresql_prefix() {
        let config = make_config("postgresql://host/db");
        assert!(config.is_postgres()); // "postgresql" starts with "postgres"
    }

    #[test]
    fn test_rate_limit_defaults() {
        let config = make_config("");
        assert_eq!(config.rate_limit_max, 100);
        assert_eq!(config.rate_limit_window_secs, 60);
    }
}
