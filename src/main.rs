use tgTool::config::Config;
use tgTool::services::crypto;
use tgTool::services::tg_manager::TgManager;
use tgTool::state::{AppState, DbPool};
use tower_http::trace::TraceLayer;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() {
    // Load configuration
    let config = Config::load();

    // Initialize tracing (file + stdout if LOG_DIR is set, stdout only otherwise)
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if let Some(ref log_dir) = config.log_dir {
        std::fs::create_dir_all(log_dir).expect("Failed to create log directory");
        let file_appender = tracing_appender::rolling::daily(log_dir, "tg_tool.log");
        let (stdout_writer, stdout_guard) = tracing_appender::non_blocking(std::io::stdout());
        let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(stdout_writer)
            .finish()
            .with(tracing_subscriber::fmt::layer().with_writer(file_writer))
            .init();
        // Guards must outlive the subscriber — leak to keep them alive for the process lifetime
        std::mem::forget(stdout_guard);
        std::mem::forget(file_guard);
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    tracing::info!(
        "TG Forwarding Tool v{} starting...",
        env!("CARGO_PKG_VERSION")
    );

    // Initialize database
    let db_pool = init_database(&config).await;
    tracing::info!("Database initialized");

    // Run migrations
    run_migrations(&db_pool).await;
    tracing::info!("Database migrations completed");

    // Ensure root user exists with a valid bcrypt hash
    ensure_root_user(&db_pool).await;
    migrate_weak_default_password(&db_pool).await;

    // Ensure directories exist
    std::fs::create_dir_all("tg_store").expect("Failed to create tg_store directory");
    std::fs::create_dir_all("image_cache").expect("Failed to create image_cache directory");
    let image_cache_dir = std::path::PathBuf::from("image_cache");

    // Build application state
    let tg_clients =
        std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    let option_cache =
        std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    let peer_cache =
        std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    let tg_manager = std::sync::Arc::new(TgManager::new(
        config.clone(),
        db_pool.clone(),
        tg_clients.clone(),
        option_cache.clone(),
        peer_cache.clone(),
    ));
    let state = AppState::new(
        db_pool.clone(),
        config.clone(),
        tg_manager.clone(),
        image_cache_dir,
    );

    // Load options cache
    load_option_cache(&state).await;

    // Migrate legacy push config to universal config structure
    migrate_push_config(&state).await;

    // Reconnect active Telegram clients
    let reconnected = tg_manager.reconnect_on_startup().await;
    if !reconnected.is_empty() {
        tracing::info!("Reconnected {} TG clients", reconnected.len());
    }

    // Start auto-reconnector for offline clients (every 30s)
    tg_manager.spawn_auto_reconnector(30);

    // Start captcha store cleanup task (every 60s, remove entries older than 5min)
    {
        let captcha_store = state.captcha_store.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let now = std::time::Instant::now();
                captcha_store
                    .retain(|_, entry| now.duration_since(entry.created_at).as_secs() < 300);
            }
        });
    }

    // Auto-migrate: 将旧系统选项中的推送配置迁移为 push_configs 记录
    {
        let db = &state.db;
        let has_configs: bool = match db {
            tgTool::state::DbPool::Sqlite(pool) => {
                let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_configs")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                count > 0
            }
            tgTool::state::DbPool::Postgres(pool) => {
                let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_configs")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                count > 0
            }
        };

        if !has_configs {
            let cache = state.option_cache.read().await;
            let api_url = cache.get("push_api_url").cloned().unwrap_or_default();
            if !api_url.is_empty() {
                tracing::info!("Auto-migrating legacy push config to push_configs table");
                let api_token = cache.get("push_api_token").cloned();
                let target = cache.get("push_target").cloned().unwrap_or_default();
                let auth_type = cache
                    .get("push_auth_type")
                    .cloned()
                    .unwrap_or_else(|| "custom_header".to_string());
                let auth_key = cache
                    .get("push_auth_key")
                    .cloned()
                    .unwrap_or_else(|| "X-API-Token".to_string());
                let http_method = cache
                    .get("push_http_method")
                    .cloned()
                    .unwrap_or_else(|| "POST".to_string());
                let body_template = cache.get("push_body_template").cloned();
                let custom_headers = cache
                    .get("push_custom_headers")
                    .cloned()
                    .unwrap_or_else(|| "[]".to_string());
                let batch_size: i64 = cache
                    .get("push_batch_size")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1000);
                let auto_push = cache.get("push_auto_push").cloned().unwrap_or_default();
                let auto_push_bool = auto_push == "1" || auto_push.eq_ignore_ascii_case("true");
                let push_interval: i64 = cache
                    .get("push_interval")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30);
                drop(cache);

                let config_id = tgTool::services::push_config::create_config(
                    db,
                    "默认推送配置",
                    &api_url,
                    api_token.as_deref(),
                    &target,
                    &auth_type,
                    &auth_key,
                    &http_method,
                    body_template.as_deref(),
                    &custom_headers,
                    batch_size,
                    "all",
                    &[],
                    auto_push_bool,
                    push_interval,
                    true,
                )
                .await;

                match config_id {
                    Ok(id) => {
                        tracing::info!("Migrated legacy push config as push_configs id={id}");
                        // 迁移 push_histories 关联
                        match db {
                            tgTool::state::DbPool::Sqlite(pool) => {
                                let _ = sqlx::query("UPDATE push_histories SET push_config_id = ? WHERE push_config_id IS NULL")
                                    .bind(id)
                                    .execute(pool)
                                    .await;
                            }
                            tgTool::state::DbPool::Postgres(pool) => {
                                let _ = sqlx::query("UPDATE push_histories SET push_config_id = $1 WHERE push_config_id IS NULL")
                                    .bind(id)
                                    .execute(pool)
                                    .await;
                            }
                        }
                        // 迁移已推送资源状态
                        match db {
                            tgTool::state::DbPool::Sqlite(pool) => {
                                let _ = sqlx::query("INSERT OR IGNORE INTO resource_push_status (resource_id, push_config_id, status) SELECT id, ?, 'pushed' FROM extracted_resources WHERE is_pushed = 1")
                                    .bind(id)
                                    .execute(pool)
                                    .await;
                            }
                            tgTool::state::DbPool::Postgres(pool) => {
                                let _ = sqlx::query("INSERT INTO resource_push_status (resource_id, push_config_id, status) SELECT id, $1, 'pushed' FROM extracted_resources WHERE is_pushed = TRUE ON CONFLICT (resource_id, push_config_id) DO NOTHING")
                                    .bind(id)
                                    .execute(pool)
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to migrate legacy push config: {e}");
                    }
                }
            }
        }
    }

    // Check if auto extract is enabled
    {
        let cache = state.option_cache.read().await;
        let auto_extract = cache.get("push_auto_extract").cloned().unwrap_or_default();
        let extract_interval: u64 = cache
            .get("push_extract_interval")
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        drop(cache);

        if auto_extract == "1" || auto_extract.eq_ignore_ascii_case("true") {
            tracing::info!(
                "Auto extract enabled, starting extract scheduler (interval: {}min)",
                extract_interval
            );
            tgTool::services::scheduler::start_extract_scheduler(
                state.extract_scheduler.clone(),
                extract_interval,
                state.clone(),
            )
            .await;
        }
    }

    // Start push scheduler unconditionally — 调度器固定 1 分钟 tick，由 run_push_tick
    // 内部按 is_active=1 AND auto_push=1 过滤；这样运行时 toggle 任一配置都不需要
    // 重启调度循环（避免丢失 config_last_run 内存态触发 LOGIC-015 防风暴）。
    // 调度监控卡片"运行中/已暂停"语义由 /status 的 push_active_configs 字段决定。
    tracing::info!("Starting push scheduler (unconditional, fixed 1-min tick)");
    tgTool::services::scheduler::start_scheduler(
        state.scheduler.clone(),
        1,
        state.db.clone(),
        state.option_cache.clone(),
    )
    .await;

    // Start forward scheduler if image hosting is configured
    {
        let cache = state.option_cache.read().await;
        let bot_id = cache.get("ImageBotId").cloned().unwrap_or_default();
        let chat_id = cache.get("ImageGroupChatId").cloned().unwrap_or_default();
        let interval: u64 = cache
            .get("ImageForwardInterval")
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        drop(cache);

        if !bot_id.is_empty() && !chat_id.is_empty() {
            tracing::info!("图片转发调度器启动 (间隔: {interval}s)");
            tgTool::services::forward_queue::start_forward_scheduler(
                state.forward_scheduler.clone(),
                interval,
                state.clone(),
            )
            .await;
        }
    }

    // Crawler 子系统启动（feature 042）— 调度器 + 图片上传 worker + 恢复 active 任务排程
    tgTool::services::crawler::scheduler::recover_active_tasks_schedule(&state).await;
    tgTool::services::crawler::scheduler::start_scheduler(state.clone()).await;
    tgTool::services::crawler::image_uploader::start_uploader(state.clone()).await;

    // Build router
    let app = tgTool::routes::build_router(state.clone())
        .layer(tgTool::middleware::cors::cors_layer())
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("{}", format_bind_error(&addr, e)));

    // Run server with graceful shutdown
    let tg_manager_shutdown = tg_manager.clone();
    let scheduler_shutdown = state.scheduler.clone();
    let extract_scheduler_shutdown = state.extract_scheduler.clone();
    let forward_scheduler_shutdown = state.forward_scheduler.clone();

    // feature 030 SEC-008：axum with_graceful_shutdown —— 信号后 drain 在途 HTTP（非立即中断）
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    if let Err(e) = result {
        tracing::error!("Server error: {e}");
    }

    // 关闭序列（serve drain 完成后）：停调度器 + 断 Telegram
    tracing::info!("HTTP drain 完成，开始关闭后台任务...");
    tgTool::services::scheduler::stop_scheduler(scheduler_shutdown).await;
    tgTool::services::scheduler::stop_extract_scheduler(extract_scheduler_shutdown).await;
    tgTool::services::forward_queue::stop_forward_scheduler(forward_scheduler_shutdown).await;
    tracing::info!("Schedulers stopped");

    // Telegram 客户端优雅断开（timeout 兜底）
    let shutdown_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tg_manager_shutdown.graceful_shutdown(),
    )
    .await;
    if shutdown_result.is_err() {
        tracing::warn!("Graceful shutdown timed out after 10 seconds");
    }
    tracing::info!("Server shutdown complete");
}

/// feature 030 SEC-008：关闭信号（ctrl_c + Unix SIGTERM）—— 触发 axum graceful drain
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        let _ = signal(SignalKind::terminate())
            .expect("install terminate handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("收到关闭信号，开始 drain 在途请求...");
}

/// 格式化 bind 错误信息（feature 030 LOGIC-008，纯函数便于 TDD）
fn format_bind_error(addr: &str, e: impl std::fmt::Display) -> String {
    format!("服务启动失败：无法监听 {addr}（{e}）— 请检查端口是否被占用或权限不足")
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bind_error_contains_addr() {
        let msg = format_bind_error("0.0.0.0:3000", "test error");
        assert!(msg.contains("0.0.0.0:3000"), "应含地址: {msg}");
    }

    #[test]
    fn test_format_bind_error_contains_reason() {
        let msg = format_bind_error("0.0.0.0:3000", "地址已被占用 (os error 98)");
        assert!(msg.contains("地址已被占用"), "应含原因: {msg}");
    }

    #[test]
    fn test_format_bind_error_contains_hint() {
        let msg = format_bind_error("0.0.0.0:3000", "err");
        assert!(
            msg.contains("端口") && msg.contains("占用"),
            "应含解决建议: {msg}"
        );
    }
}

async fn init_database(config: &Config) -> DbPool {
    let database_url = config.database_url();
    tracing::info!(
        "Connecting to database ({}-mode)...",
        if config.is_postgres() {
            "postgres"
        } else {
            "sqlite"
        }
    );

    if config.is_postgres() {
        // 用 tokio::time::timeout 包装连接，无论卡在 TCP/认证/SSL，15 秒必失败
        // （PG 未完全启动或共享内存异常时会卡住握手）
        let connect_fut = sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&database_url);
        let pool = tokio::time::timeout(std::time::Duration::from_secs(15), connect_fut)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "连接 PostgreSQL 超时（15秒无响应）\n\
                     排查：1) 确认 PG 已完全启动（重启后等待几秒）\n\
                     2) 检查 SQL_DSN 主机/端口/密码\n\
                     3) PG 共享内存错误(58P01)需重启 PG 服务\n\
                     4) 查看 PG 日志确认是否在 recovery"
                )
            })
            .unwrap_or_else(|e| panic!("Failed to connect to PostgreSQL: {e}"));
        DbPool::Postgres(pool)
    } else {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    use sqlx::Executor;
                    // busy_timeout: 并发写入时等待锁而非直接报错（5秒）
                    conn.execute(sqlx::query("PRAGMA busy_timeout=5000"))
                        .await?;
                    // WAL 模式: 写操作不再阻塞读操作
                    conn.execute(sqlx::query("PRAGMA journal_mode=WAL")).await?;
                    // synchronous=NORMAL: WAL 模式下足够安全，性能更好
                    conn.execute(sqlx::query("PRAGMA synchronous=NORMAL"))
                        .await?;
                    // WAL auto-checkpoint: 避免 WAL 文件无限增长
                    conn.execute(sqlx::query("PRAGMA wal_autocheckpoint=1000"))
                        .await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await
            .expect("Failed to connect to SQLite");

        DbPool::Sqlite(pool)
    }
}

async fn run_migrations(pool: &DbPool) {
    match pool {
        DbPool::Sqlite(pool) => {
            let migration_sql = include_str!("../migrations/001_init_sqlite.sql");
            sqlx::raw_sql(migration_sql)
                .execute(pool)
                .await
                .expect("Failed to run SQLite migrations");
            // Migration 002: Add client_id to collectors (idempotent — ignore if already exists)
            let m2 = include_str!("../migrations/002_collector_client_id_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m2).execute(pool).await {
                // "duplicate column name" means already applied — safe to ignore
                if !e.to_string().contains("duplicate column") {
                    panic!("Failed to run SQLite migration 002: {e}");
                }
                tracing::debug!("SQLite migration 002 skipped (already applied)");
            }
            // Migration 003: Create extracted_resources table
            let m3 = include_str!("../migrations/003_extracted_resources_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m3).execute(pool).await {
                if !e.to_string().contains("already exists") {
                    panic!("Failed to run SQLite migration 003: {e}");
                }
                tracing::debug!("SQLite migration 003 skipped (already applied)");
            }
            // Migration 004: Add is_extracted to collector_histories
            let m4 = include_str!("../migrations/004_collector_histories_is_extracted_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m4).execute(pool).await {
                if !e.to_string().contains("duplicate column") {
                    panic!("Failed to run SQLite migration 004: {e}");
                }
                tracing::debug!("SQLite migration 004 skipped (already applied)");
            }
            // Migration 005: Add share_ids to extracted_resources
            let m5 = include_str!("../migrations/005_add_share_ids_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m5).execute(pool).await {
                if !e.to_string().contains("duplicate column") {
                    panic!("Failed to run SQLite migration 005: {e}");
                }
                tracing::debug!("SQLite migration 005 skipped (already applied)");
            }
            // Migration 006: Dedup extracted_resources + unique index on url
            let m6 = include_str!("../migrations/006_dedup_extracted_resources_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m6).execute(pool).await {
                if !e.to_string().contains("already exists") {
                    panic!("Failed to run SQLite migration 006: {e}");
                }
                tracing::debug!("SQLite migration 006 skipped (already applied)");
            }
            // Migration 007: no-op for SQLite
            let m7 = include_str!("../migrations/007_int4_to_int8_sqlite.sql");
            sqlx::raw_sql(m7).execute(pool).await.ok();
            // Migration 008: Image mappings + forward tasks
            let m8 = include_str!("../migrations/008_image_tables_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m8).execute(pool).await {
                if !e.to_string().contains("already exists") {
                    panic!("Failed to run SQLite migration 008: {e}");
                }
                tracing::debug!("SQLite migration 008 skipped (already applied)");
            }
            // Migration 009: Create extract_histories table
            let m9 = include_str!("../migrations/009_extract_histories_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m9).execute(pool).await {
                if !e.to_string().contains("already exists") {
                    panic!("Failed to run SQLite migration 009: {e}");
                }
                tracing::debug!("SQLite migration 009 skipped (already applied)");
            }
            // Migration 010: Add filter + forward_client_id columns to rules
            let m10 = include_str!("../migrations/010_rule_filter_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m10).execute(pool).await {
                if !e.to_string().contains("duplicate column") {
                    panic!("Failed to run SQLite migration 010: {e}");
                }
                tracing::debug!("SQLite migration 010 skipped (already applied)");
            }
            // Migration 011: Add source_client_id to rules
            let m11 = include_str!("../migrations/011_rule_source_client_sqlite.sql");
            if let Err(e) = sqlx::raw_sql(m11).execute(pool).await {
                if !e.to_string().contains("duplicate column") {
                    panic!("Failed to run SQLite migration 011: {e}");
                }
                tracing::debug!("SQLite migration 011 skipped (already applied)");
            }
            // Migration 012: push_configs + push_config_collectors + resource_push_status
            {
                let m12_tables = "\
                    CREATE TABLE IF NOT EXISTS push_configs ( \
                        id INTEGER PRIMARY KEY AUTOINCREMENT, \
                        name TEXT NOT NULL, \
                        api_url TEXT NOT NULL DEFAULT '', \
                        api_token TEXT, \
                        target TEXT NOT NULL DEFAULT '', \
                        auth_type TEXT NOT NULL DEFAULT 'custom_header', \
                        auth_key TEXT NOT NULL DEFAULT 'X-API-Token', \
                        http_method TEXT NOT NULL DEFAULT 'POST', \
                        body_template TEXT, \
                        custom_headers TEXT NOT NULL DEFAULT '[]', \
                        batch_size INTEGER NOT NULL DEFAULT 1000, \
                        data_source_type TEXT NOT NULL DEFAULT 'all', \
                        auto_push INTEGER NOT NULL DEFAULT 0, \
                        push_interval INTEGER NOT NULL DEFAULT 30, \
                        is_active INTEGER NOT NULL DEFAULT 1, \
                        created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, \
                        updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP \
                    ); \
                    CREATE TABLE IF NOT EXISTS push_config_collectors ( \
                        push_config_id INTEGER NOT NULL REFERENCES push_configs(id) ON DELETE CASCADE, \
                        collector_id INTEGER NOT NULL REFERENCES collectors(id) ON DELETE CASCADE, \
                        PRIMARY KEY (push_config_id, collector_id) \
                    ); \
                    CREATE TABLE IF NOT EXISTS resource_push_status ( \
                        id INTEGER PRIMARY KEY AUTOINCREMENT, \
                        resource_id INTEGER NOT NULL REFERENCES extracted_resources(id) ON DELETE CASCADE, \
                        push_config_id INTEGER NOT NULL REFERENCES push_configs(id) ON DELETE CASCADE, \
                        status TEXT NOT NULL DEFAULT 'pending', \
                        created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, \
                        updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, \
                        UNIQUE(resource_id, push_config_id) \
                    ); \
                    CREATE INDEX IF NOT EXISTS idx_resource_push_status_config ON resource_push_status(push_config_id); \
                    CREATE INDEX IF NOT EXISTS idx_resource_push_status_status ON resource_push_status(status); \
                ";
                sqlx::raw_sql(m12_tables)
                    .execute(pool)
                    .await
                    .expect("Failed to run SQLite migration 012 (tables)");

                // ALTER TABLE ADD COLUMN — 幂等：先检查列是否存在
                let has_col: bool = sqlx::query_scalar(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('push_histories') WHERE name = 'push_config_id'"
                )
                .fetch_one(pool)
                .await
                .unwrap_or(false);
                if !has_col {
                    sqlx::query("ALTER TABLE push_histories ADD COLUMN push_config_id INTEGER REFERENCES push_configs(id)")
                        .execute(pool)
                        .await
                        .expect("Failed to add push_config_id to push_histories");
                    tracing::info!("SQLite migration 012: added push_config_id to push_histories");
                } else {
                    tracing::debug!(
                        "SQLite migration 012: push_config_id already exists in push_histories"
                    );
                }
            }

            // Migration 013: link_check_results + push_skip_records + push_histories skip columns
            {
                let m13 = include_str!("../migrations/013_resource_link_check_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m13).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("duplicate column") && !msg.contains("already exists") {
                        panic!("Failed to run SQLite migration 013: {e}");
                    }
                    tracing::debug!("SQLite migration 013 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 013 applied");
                }
            }

            // Migration 014: push_configs 加 link_check_before_push 开关
            {
                let m14 = include_str!("../migrations/014_push_config_link_check_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m14).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("duplicate column") && !msg.contains("already exists") {
                        panic!("Failed to run SQLite migration 014: {e}");
                    }
                    tracing::debug!("SQLite migration 014 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 014 applied");
                }
            }

            // Migration 015: forward_tasks 加 image_message_id 字段 + awaiting_bot 部分索引
            {
                let m15 = include_str!("../migrations/015_forward_task_message_id_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m15).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("duplicate column") && !msg.contains("already exists") {
                        panic!("Failed to run SQLite migration 015: {e}");
                    }
                    tracing::debug!("SQLite migration 015 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 015 applied");
                }
            }
            // Migration 016: users 加 must_change_password（feature 027 SEC-002）
            {
                let m16 = include_str!("../migrations/016_users_must_change_password_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m16).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("duplicate column") && !msg.contains("already exists") {
                        panic!("Failed to run SQLite migration 016: {e}");
                    }
                    tracing::debug!("SQLite migration 016 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 016 applied");
                }
            }
            // Migration 017: clients 加 name/username（客户端列表显示账号名）
            {
                let m17 = include_str!("../migrations/017_client_name_username_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m17).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("duplicate column") && !msg.contains("already exists") {
                        panic!("Failed to run SQLite migration 017: {e}");
                    }
                    tracing::debug!("SQLite migration 017 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 017 applied");
                }
            }
            // Migration 018: forward_tasks 加 (remote_id, id DESC) 索引（修复资源分页慢）
            {
                let m18 = include_str!("../migrations/018_forward_tasks_remote_id_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m18).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("already exists") {
                        panic!("Failed to run SQLite migration 018: {e}");
                    }
                    tracing::debug!("SQLite migration 018 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 018 applied");
                }
            }
            // Migration 019: push_histories.data_count INT4->BIGINT（SQLite no-op，保持序号一致）
            {
                let m19 =
                    include_str!("../migrations/019_push_histories_data_count_bigint_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m19).execute(pool).await {
                    tracing::warn!("SQLite migration 019 skipped: {e}");
                } else {
                    tracing::debug!("SQLite migration 019 applied (no-op)");
                }
            }
            // Migration 020: crawler_tasks（feature 042-web-crawler-collector）
            {
                let m20 = include_str!("../migrations/020_crawler_tasks_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m20).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run SQLite migration 020: {e}");
                    }
                    tracing::debug!("SQLite migration 020 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 020 applied (crawler_tasks)");
                }
            }
            // Migration 021: crawler_articles
            {
                let m21 = include_str!("../migrations/021_crawler_articles_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m21).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run SQLite migration 021: {e}");
                    }
                    tracing::debug!("SQLite migration 021 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 021 applied (crawler_articles)");
                }
            }
            // Migration 022: crawler_article_links
            {
                let m22 = include_str!("../migrations/022_crawler_article_links_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m22).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run SQLite migration 022: {e}");
                    }
                    tracing::debug!("SQLite migration 022 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 022 applied (crawler_article_links)");
                }
            }
            // Migration 023: crawler_article_images
            {
                let m23 = include_str!("../migrations/023_crawler_article_images_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m23).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run SQLite migration 023: {e}");
                    }
                    tracing::debug!("SQLite migration 023 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 023 applied (crawler_article_images)");
                }
            }
            // Migration 024: crawler_run_histories
            {
                let m24 = include_str!("../migrations/024_crawler_run_histories_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m24).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run SQLite migration 024: {e}");
                    }
                    tracing::debug!("SQLite migration 024 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 024 applied (crawler_run_histories)");
                }
            }
            // Migration 025: crawler_tasks 自动翻页字段
            {
                let m25 = include_str!("../migrations/025_crawler_tasks_pagination_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m25).execute(pool).await {
                    // ALTER TABLE ADD COLUMN 失败时，错误信息含 "duplicate column name"
                    if !e.to_string().contains("duplicate column name") {
                        panic!("Failed to run SQLite migration 025: {e}");
                    }
                    tracing::debug!("SQLite migration 025 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 025 applied (crawler_tasks pagination)");
                }
            }
            // Migration 026: 删除 crawler_tasks.selectors 列（043 取代 042 抓取路径）
            {
                let m26 = include_str!("../migrations/026_crawler_drop_selectors_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m26).execute(pool).await {
                    // SQLite DROP COLUMN：列不存在时错误信息含 "no such column"
                    let msg = e.to_string();
                    if msg.contains("no such column") || msg.contains("already") {
                        tracing::debug!("SQLite migration 026 skipped (selectors column already dropped)");
                    } else {
                        panic!("Failed to run SQLite migration 026: {e}");
                    }
                } else {
                    tracing::info!("SQLite migration 026 applied (crawler_tasks.selectors dropped)");
                }
            }
            // Migration 027: 预置字段库表 + 种子数据（043）
            {
                let m27 = include_str!("../migrations/027_crawler_field_library_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m27).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run SQLite migration 027: {e}");
                    }
                    tracing::debug!("SQLite migration 027 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 027 applied (crawler_field_library)");
                }
            }
            // Migration 028: 任务字段树节点表（043）
            {
                let m28 = include_str!("../migrations/028_crawler_task_field_nodes_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m28).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run SQLite migration 028: {e}");
                    }
                    tracing::debug!("SQLite migration 028 skipped (already applied)");
                } else {
                    tracing::info!("SQLite migration 028 applied (crawler_task_field_nodes)");
                }
            }
            // Migration 029: 文章扩展字段值表 + extra_fields_json（043）
            {
                let m29 = include_str!("../migrations/029_crawler_article_extras_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m29).execute(pool).await {
                    let msg = e.to_string();
                    if msg.contains("already exists") || msg.contains("duplicate column") {
                        tracing::debug!("SQLite migration 029 skipped (already applied)");
                    } else {
                        panic!("Failed to run SQLite migration 029: {e}");
                    }
                } else {
                    tracing::info!("SQLite migration 029 applied (crawler_article_field_values + extra_fields_json)");
                }
            }
            // Migration 030: crawler_tasks.max_pagination_depth（043 US5 分页）
            {
                let m30 = include_str!("../migrations/030_crawler_tasks_pagination_depth_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m30).execute(pool).await {
                    let msg = e.to_string();
                    if msg.contains("duplicate column") || msg.contains("already exists") {
                        tracing::debug!("SQLite migration 030 skipped (already applied)");
                    } else {
                        panic!("Failed to run SQLite migration 030: {e}");
                    }
                } else {
                    tracing::info!("SQLite migration 030 applied (crawler_tasks.max_pagination_depth)");
                }
            }
            // Migration 031: crawler_field_library resource 类补 download_url/resource_name + sort_order 重排
            {
                let m31 = include_str!("../migrations/031_crawler_field_library_resource_sort_sqlite.sql");
                if let Err(e) = sqlx::raw_sql(m31).execute(pool).await {
                    tracing::warn!("SQLite migration 031 (field_library resource sort) failed: {e}");
                } else {
                    tracing::info!("SQLite migration 031 applied (crawler_field_library resource sort)");
                }
            }
            // 043：种子化 crawler_field_library（若表为空则用应用层 BUILTIN_PRESETS 补种）
            {
                if let Err(e) = tgTool::services::crawler::preset_library::seed_if_empty_sqlite(pool).await {
                    tracing::warn!("SQLite crawler_field_library seed failed: {e}");
                }
            }
        }
        DbPool::Postgres(pool) => {
            let migration_sql = include_str!("../migrations/001_init_postgres.sql");
            sqlx::raw_sql(migration_sql)
                .execute(pool)
                .await
                .expect("Failed to run PostgreSQL migrations");
            // Migration 002: Add client_id to collectors (idempotent — ignore if already exists)
            let m2 = include_str!("../migrations/002_collector_client_id_postgres.sql");
            if let Err(e) = sqlx::raw_sql(m2).execute(pool).await {
                if !e.to_string().contains("already exists") && !e.to_string().contains("duplicate")
                {
                    panic!("Failed to run PostgreSQL migration 002: {e}");
                }
                tracing::debug!("PostgreSQL migration 002 skipped (already applied)");
            }
            // Migration 003: Create extracted_resources table
            let m3 = include_str!("../migrations/003_extracted_resources_postgres.sql");
            if let Err(e) = sqlx::raw_sql(m3).execute(pool).await {
                if !e.to_string().contains("already exists") {
                    panic!("Failed to run PostgreSQL migration 003: {e}");
                }
                tracing::debug!("PostgreSQL migration 003 skipped (already applied)");
            }
            // Migration 004: Add is_extracted to collector_histories
            let m4 =
                include_str!("../migrations/004_collector_histories_is_extracted_postgres.sql");
            if let Err(e) = sqlx::raw_sql(m4).execute(pool).await {
                if !e.to_string().contains("already exists") && !e.to_string().contains("duplicate")
                {
                    panic!("Failed to run PostgreSQL migration 004: {e}");
                }
                tracing::debug!("PostgreSQL migration 004 skipped (already applied)");
            }
            // Migration 005: Add share_ids to extracted_resources
            let m5 = include_str!("../migrations/005_add_share_ids_postgres.sql");
            if let Err(e) = sqlx::raw_sql(m5).execute(pool).await {
                if !e.to_string().contains("already exists") && !e.to_string().contains("duplicate")
                {
                    panic!("Failed to run PostgreSQL migration 005: {e}");
                }
                tracing::debug!("PostgreSQL migration 005 skipped (already applied)");
            }
            // Migration 006: Dedup extracted_resources + unique index on url
            let m6 = include_str!("../migrations/006_dedup_extracted_resources_postgres.sql");
            if let Err(e) = sqlx::raw_sql(m6).execute(pool).await {
                if !e.to_string().contains("already exists") {
                    panic!("Failed to run PostgreSQL migration 006: {e}");
                }
                tracing::debug!("PostgreSQL migration 006 skipped (already applied)");
            }
            // Migration 007: Convert INT4 id/fk columns to INT8 (BIGINT)
            {
                let m7 = include_str!("../migrations/007_int4_to_int8_postgres.sql");
                match sqlx::raw_sql(m7).execute(pool).await {
                    Ok(_) => tracing::info!("PostgreSQL migration 007 applied (INT4 -> INT8)"),
                    Err(e) => {
                        // Already INT8 or other safe error
                        tracing::info!("PostgreSQL migration 007 skipped: {e}");
                    }
                }
            }
            // Migration 008: Image mappings + forward tasks
            {
                let m8 = include_str!("../migrations/008_image_tables_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m8).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 008: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 008 skipped (already applied)");
                }
            }
            // Migration 009: Create extract_histories table
            {
                let m9 = include_str!("../migrations/009_extract_histories_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m9).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 009: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 009 skipped (already applied)");
                }
            }
            // Migration 010: Add filter + forward_client_id columns to rules
            {
                let m10 = include_str!("../migrations/010_rule_filter_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m10).execute(pool).await {
                    if !e.to_string().contains("already exists")
                        && !e.to_string().contains("duplicate")
                    {
                        panic!("Failed to run PostgreSQL migration 010: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 010 skipped (already applied)");
                }
            }
            // Migration 011: Add source_client_id to rules
            {
                let m11 = include_str!("../migrations/011_rule_source_client_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m11).execute(pool).await {
                    if !e.to_string().contains("already exists")
                        && !e.to_string().contains("duplicate")
                    {
                        panic!("Failed to run PostgreSQL migration 011: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 011 skipped (already applied)");
                }
            }
            // Migration 012: push_configs + push_config_collectors + resource_push_status
            {
                let m12 = include_str!("../migrations/012_push_configs_postgres.sql");
                // CREATE TABLE IF NOT EXISTS 部分是幂等的，ALTER TABLE ADD COLUMN 需要检查
                // 先执行整个文件，如果 ALTER TABLE 报列已存在则忽略
                if let Err(e) = sqlx::raw_sql(m12).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("already exists") && !msg.contains("duplicate") {
                        panic!("Failed to run PostgreSQL migration 012: {e}");
                    }
                    // 如果只是 ALTER TABLE 列已存在，尝试单独补建表
                    tracing::debug!(
                        "PostgreSQL migration 012: partial skip, ensuring tables exist"
                    );
                    let m12_tables = "\
                        CREATE TABLE IF NOT EXISTS push_configs ( \
                            id BIGSERIAL PRIMARY KEY, \
                            name TEXT NOT NULL, \
                            api_url TEXT NOT NULL DEFAULT '', \
                            api_token TEXT, \
                            target TEXT NOT NULL DEFAULT '', \
                            auth_type TEXT NOT NULL DEFAULT 'custom_header', \
                            auth_key TEXT NOT NULL DEFAULT 'X-API-Token', \
                            http_method TEXT NOT NULL DEFAULT 'POST', \
                            body_template TEXT, \
                            custom_headers TEXT NOT NULL DEFAULT '[]', \
                            batch_size BIGINT NOT NULL DEFAULT 1000, \
                            data_source_type TEXT NOT NULL DEFAULT 'all', \
                            auto_push BOOLEAN NOT NULL DEFAULT false, \
                            push_interval BIGINT NOT NULL DEFAULT 30, \
                            is_active BOOLEAN NOT NULL DEFAULT true, \
                            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, \
                            updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP \
                        ); \
                        CREATE TABLE IF NOT EXISTS push_config_collectors ( \
                            push_config_id BIGINT NOT NULL REFERENCES push_configs(id) ON DELETE CASCADE, \
                            collector_id BIGINT NOT NULL REFERENCES collectors(id) ON DELETE CASCADE, \
                            PRIMARY KEY (push_config_id, collector_id) \
                        ); \
                        CREATE TABLE IF NOT EXISTS resource_push_status ( \
                            id BIGSERIAL PRIMARY KEY, \
                            resource_id BIGINT NOT NULL REFERENCES extracted_resources(id) ON DELETE CASCADE, \
                            push_config_id BIGINT NOT NULL REFERENCES push_configs(id) ON DELETE CASCADE, \
                            status TEXT NOT NULL DEFAULT 'pending', \
                            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, \
                            updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, \
                            UNIQUE(resource_id, push_config_id) \
                        ); \
                        CREATE INDEX IF NOT EXISTS idx_resource_push_status_config ON resource_push_status(push_config_id); \
                        CREATE INDEX IF NOT EXISTS idx_resource_push_status_status ON resource_push_status(status); \
                    ";
                    sqlx::raw_sql(m12_tables)
                        .execute(pool)
                        .await
                        .expect("Failed to ensure PostgreSQL migration 012 tables");

                    // ALTER TABLE — 检查列是否存在
                    let has_col: bool = sqlx::query_scalar(
                        "SELECT COUNT(*) > 0 FROM information_schema.columns WHERE table_name = 'push_histories' AND column_name = 'push_config_id'"
                    )
                    .fetch_one(pool)
                    .await
                    .unwrap_or(false);
                    if !has_col {
                        sqlx::query("ALTER TABLE push_histories ADD COLUMN push_config_id BIGINT REFERENCES push_configs(id)")
                            .execute(pool)
                            .await
                            .expect("Failed to add push_config_id to push_histories");
                        tracing::info!(
                            "PostgreSQL migration 012: added push_config_id to push_histories"
                        );
                    }
                }
            }

            // Migration 013: link_check_results + push_skip_records + push_histories skip columns
            {
                let m13 = include_str!("../migrations/013_resource_link_check_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m13).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("already exists") && !msg.contains("duplicate") {
                        panic!("Failed to run PostgreSQL migration 013: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 013 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 013 applied");
                }
            }

            // Migration 014: push_configs 加 link_check_before_push 开关
            {
                let m14 = include_str!("../migrations/014_push_config_link_check_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m14).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("already exists") && !msg.contains("duplicate") {
                        panic!("Failed to run PostgreSQL migration 014: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 014 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 014 applied");
                }
            }

            // Migration 015: forward_tasks 加 image_message_id 字段 + awaiting_bot 部分索引
            {
                let m15 = include_str!("../migrations/015_forward_task_message_id_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m15).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("already exists") && !msg.contains("duplicate") {
                        panic!("Failed to run PostgreSQL migration 015: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 015 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 015 applied");
                }
            }
            // Migration 016: users 加 must_change_password（feature 027 SEC-002）
            {
                let m16 = include_str!("../migrations/016_users_must_change_password_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m16).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("already exists") && !msg.contains("duplicate") {
                        panic!("Failed to run PostgreSQL migration 016: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 016 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 016 applied");
                }
            }
            // Migration 017: clients 加 name/username（客户端列表显示账号名）
            {
                let m17 = include_str!("../migrations/017_client_name_username_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m17).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("already exists") && !msg.contains("duplicate") {
                        panic!("Failed to run PostgreSQL migration 017: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 017 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 017 applied");
                }
            }
            // Migration 018: forward_tasks 加 (remote_id, id DESC) 索引（修复资源分页慢）
            {
                let m18 = include_str!("../migrations/018_forward_tasks_remote_id_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m18).execute(pool).await {
                    let msg = e.to_string();
                    if !msg.contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 018: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 018 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 018 applied");
                }
            }
            // Migration 019: push_histories.data_count INT4 -> BIGINT
            // 修复存量 PG 库（在 001 改用 BIGINT 之前创建）data_count 仍为 INT4 导致推送历史 500 错误
            {
                let m19 =
                    include_str!("../migrations/019_push_histories_data_count_bigint_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m19).execute(pool).await {
                    tracing::warn!("PostgreSQL migration 019 skipped: {e}");
                } else {
                    tracing::info!("PostgreSQL migration 019 applied (data_count -> BIGINT)");
                }
            }
            // Migration 020: crawler_tasks（feature 042-web-crawler-collector）
            {
                let m20 = include_str!("../migrations/020_crawler_tasks_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m20).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 020: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 020 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 020 applied (crawler_tasks)");
                }
            }
            // Migration 021: crawler_articles
            {
                let m21 = include_str!("../migrations/021_crawler_articles_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m21).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 021: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 021 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 021 applied (crawler_articles)");
                }
            }
            // Migration 022: crawler_article_links
            {
                let m22 = include_str!("../migrations/022_crawler_article_links_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m22).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 022: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 022 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 022 applied (crawler_article_links)");
                }
            }
            // Migration 023: crawler_article_images
            {
                let m23 = include_str!("../migrations/023_crawler_article_images_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m23).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 023: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 023 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 023 applied (crawler_article_images)");
                }
            }
            // Migration 024: crawler_run_histories
            {
                let m24 = include_str!("../migrations/024_crawler_run_histories_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m24).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 024: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 024 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 024 applied (crawler_run_histories)");
                }
            }
            // Migration 025: crawler_tasks 自动翻页字段
            {
                let m25 = include_str!("../migrations/025_crawler_tasks_pagination_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m25).execute(pool).await {
                    // ADD COLUMN IF NOT EXISTS 在 PG 里通常不会失败；若失败按已应用处理
                    if !e.to_string().contains("already exists") && !e.to_string().contains("already applied") {
                        tracing::warn!("PostgreSQL migration 025 skipped: {e}");
                    } else {
                        tracing::debug!("PostgreSQL migration 025 skipped (already applied)");
                    }
                } else {
                    tracing::info!("PostgreSQL migration 025 applied (crawler_tasks pagination)");
                }
            }
            // Migration 026: 删除 crawler_tasks.selectors 列（043 取代 042 抓取路径）
            {
                let m26 = include_str!("../migrations/026_crawler_drop_selectors_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m26).execute(pool).await {
                    let msg = e.to_string();
                    // PG: DROP COLUMN IF NOT EXISTS 会报 "column does not exist"（实际 IF EXISTS 不报错；
                    // 此兜底防 odd schema drift）
                    if msg.contains("does not exist") || msg.contains("already") {
                        tracing::debug!("PostgreSQL migration 026 skipped (selectors already dropped)");
                    } else {
                        panic!("Failed to run PostgreSQL migration 026: {e}");
                    }
                } else {
                    tracing::info!("PostgreSQL migration 026 applied (crawler_tasks.selectors dropped)");
                }
            }
            // Migration 027: 预置字段库表 + 种子数据（043）
            {
                let m27 = include_str!("../migrations/027_crawler_field_library_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m27).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 027: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 027 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 027 applied (crawler_field_library)");
                }
            }
            // Migration 028: 任务字段树节点表（043）
            {
                let m28 = include_str!("../migrations/028_crawler_task_field_nodes_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m28).execute(pool).await {
                    if !e.to_string().contains("already exists") {
                        panic!("Failed to run PostgreSQL migration 028: {e}");
                    }
                    tracing::debug!("PostgreSQL migration 028 skipped (already applied)");
                } else {
                    tracing::info!("PostgreSQL migration 028 applied (crawler_task_field_nodes)");
                }
            }
            // Migration 029: 文章扩展字段值表 + extra_fields_json（043）
            {
                let m29 = include_str!("../migrations/029_crawler_article_extras_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m29).execute(pool).await {
                    let msg = e.to_string();
                    if msg.contains("already exists") || msg.contains("already") {
                        tracing::debug!("PostgreSQL migration 029 skipped (already applied)");
                    } else {
                        panic!("Failed to run PostgreSQL migration 029: {e}");
                    }
                } else {
                    tracing::info!("PostgreSQL migration 029 applied (crawler_article_field_values + extra_fields_json)");
                }
            }
            // Migration 030: crawler_tasks.max_pagination_depth（043 US5 分页）
            {
                let m30 = include_str!("../migrations/030_crawler_tasks_pagination_depth_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m30).execute(pool).await {
                    let msg = e.to_string();
                    if msg.contains("already exists") || msg.contains("already") {
                        tracing::debug!("PostgreSQL migration 030 skipped (already applied)");
                    } else {
                        panic!("Failed to run PostgreSQL migration 030: {e}");
                    }
                } else {
                    tracing::info!("PostgreSQL migration 030 applied (crawler_tasks.max_pagination_depth)");
                }
            }
            // Migration 031: crawler_field_library resource 类补 download_url/resource_name + sort_order 重排
            {
                let m31 = include_str!("../migrations/031_crawler_field_library_resource_sort_postgres.sql");
                if let Err(e) = sqlx::raw_sql(m31).execute(pool).await {
                    tracing::warn!("PostgreSQL migration 031 (field_library resource sort) failed: {e}");
                } else {
                    tracing::info!("PostgreSQL migration 031 applied (crawler_field_library resource sort)");
                }
            }
            // 043：种子化 crawler_field_library
            {
                if let Err(e) = tgTool::services::crawler::preset_library::seed_if_empty_postgres(pool).await {
                    tracing::warn!("PostgreSQL crawler_field_library seed failed: {e}");
                }
            }
        }
    }
}

async fn load_option_cache(state: &AppState) {
    let options = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, (String, Option<String>)>("SELECT key, value FROM options")
                .fetch_all(pool)
                .await
                .unwrap_or_default()
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, (String, Option<String>)>("SELECT key, value FROM options")
                .fetch_all(pool)
                .await
                .unwrap_or_default()
        }
    };

    let mut cache = state.option_cache.write().await;
    for (key, value) in options {
        cache.insert(key, value.unwrap_or_default());
    }
    tracing::info!("Loaded {} options into cache", cache.len());
}

/// Ensure the default root user exists with a valid bcrypt hash.
/// This avoids hardcoding a bcrypt hash in the migration SQL,
/// which would break across bcrypt library version upgrades.
async fn ensure_root_user(pool: &DbPool) {
    // feature 027 SEC-002：root 不再用固定 123456，首启动生成随机强口令 + must_change_password=1
    let random_pw = crypto::generate_random_password();
    let hash = crypto::hash_password(&random_pw).expect("Failed to hash root password");
    let created = match pool {
        DbPool::Sqlite(pool) => {
            sqlx::query("INSERT OR IGNORE INTO users (username, password, role, status, must_change_password) VALUES ('root', ?, 100, 1, 1)")
                .bind(&hash)
                .execute(pool)
                .await
                .expect("Failed to ensure root user")
                .rows_affected()
                > 0
        }
        DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO users (username, password, role, status, must_change_password) VALUES ('root', $1, 100, 1, TRUE) ON CONFLICT (username) DO NOTHING")
                .bind(&hash)
                .execute(pool)
                .await
                .expect("Failed to ensure root user")
                .rows_affected()
                > 0
        }
    };

    if created {
        // 首启动打印一次性初始随机口令（敏感·首次登录后请立即改密）
        tracing::warn!(
            "已创建默认 root 用户，初始随机口令（敏感·首次登录后请立即改密）: {}",
            random_pw
        );
    }
}

/// feature 027 SEC-002：存量迁移——检测仍为弱口令 123456 的账号，标记 must_change_password
/// （不删除、不改 hash，仅标记；下次登录强制改密。兼容既有部署。）
async fn migrate_weak_default_password(pool: &DbPool) {
    let users: Vec<(i64, String)> = match pool {
        DbPool::Sqlite(pool) => sqlx::query_as("SELECT id, password FROM users")
            .fetch_all(pool)
            .await
            .unwrap_or_default(),
        DbPool::Postgres(pool) => sqlx::query_as("SELECT id, password FROM users")
            .fetch_all(pool)
            .await
            .unwrap_or_default(),
    };
    for (id, hash) in users {
        if crypto::verify_password("123456", &hash).unwrap_or(false) {
            match pool {
                DbPool::Sqlite(p) => {
                    let _ = sqlx::query("UPDATE users SET must_change_password = 1 WHERE id = ?")
                        .bind(id)
                        .execute(p)
                        .await;
                }
                DbPool::Postgres(p) => {
                    let _ =
                        sqlx::query("UPDATE users SET must_change_password = TRUE WHERE id = $1")
                            .bind(id)
                            .execute(p)
                            .await;
                }
            };
            tracing::warn!("用户 {} 检测到弱口令 123456，已标记必须改密", id);
        }
    }
}

/// Migrate legacy push config (push_api_token with X-API-Token) to universal config structure.
/// Only runs when push_api_token exists but push_auth_type does not — ensures backward compatibility.
async fn migrate_push_config(state: &AppState) {
    let cache = state.option_cache.read().await;
    let has_api_token = cache.contains_key("push_api_token")
        && !cache
            .get("push_api_token")
            .unwrap_or(&String::new())
            .is_empty();
    let has_auth_type = cache.contains_key("push_auth_type");
    drop(cache);

    if has_api_token && !has_auth_type {
        tracing::info!("Migrating legacy push config to universal config structure...");

        let defaults = [
            ("push_auth_type", "custom_header"),
            ("push_auth_key", "X-API-Token"),
            ("push_http_method", "POST"),
            ("push_body_template", "{\"resources\": {{resources}}}"),
            ("push_custom_headers", "[]"),
        ];

        let mut cache = state.option_cache.write().await;
        for (key, value) in &defaults {
            match &state.db {
                DbPool::Sqlite(pool) => {
                    sqlx::query("INSERT OR REPLACE INTO options (key, value) VALUES (?, ?)")
                        .bind(key)
                        .bind(value)
                        .execute(pool)
                        .await
                        .expect("Failed to migrate push config");
                }
                DbPool::Postgres(pool) => {
                    sqlx::query(
                        "INSERT INTO options (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = $2",
                    )
                    .bind(key)
                    .bind(value)
                    .execute(pool)
                    .await
                    .expect("Failed to migrate push config");
                }
            }
            cache.insert(key.to_string(), value.to_string());
        }
        drop(cache);

        tracing::info!("Push config migration completed (5 defaults written)");
    }
}
