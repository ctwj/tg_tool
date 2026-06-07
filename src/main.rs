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
        let (stdout_writer, _stdout_guard) = tracing_appender::non_blocking(std::io::stdout());
        let (file_writer, _file_guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(stdout_writer)
            .finish()
            .with(tracing_subscriber::fmt::layer().with_writer(file_writer))
            .init();
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

    // Build router
    let app = tgTool::routes::build_router(state.clone())
        .layer(tgTool::middleware::cors::cors_layer())
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    // Run server with graceful shutdown
    let tg_manager_shutdown = tg_manager.clone();
    let scheduler_shutdown = state.scheduler.clone();
    let extract_scheduler_shutdown = state.extract_scheduler.clone();
    let forward_scheduler_shutdown = state.forward_scheduler.clone();

    tokio::select! {
        result = axum::serve(listener, app) => {
            if let Err(e) = result {
                tracing::error!("Server error: {e}");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received shutdown signal, starting graceful shutdown...");

            // Stop scheduler
            tgTool::services::scheduler::stop_scheduler(scheduler_shutdown).await;
            tgTool::services::scheduler::stop_extract_scheduler(extract_scheduler_shutdown).await;
            tgTool::services::forward_queue::stop_forward_scheduler(forward_scheduler_shutdown).await;
            tracing::info!("Schedulers stopped");

            // Graceful shutdown with timeout
            let shutdown_result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                tg_manager_shutdown.graceful_shutdown(),
            ).await;

            if shutdown_result.is_err() {
                tracing::warn!("Graceful shutdown timed out after 10 seconds");
            }
            tracing::info!("Server shutdown complete");
        }
    }
}

async fn init_database(config: &Config) -> DbPool {
    let database_url = config.database_url();

    if config.is_postgres() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .connect(&database_url)
            .await
            .expect("Failed to connect to PostgreSQL");
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
    let hash = crypto::hash_password("123456").expect("Failed to hash root password");
    let created = match pool {
        DbPool::Sqlite(pool) => {
            sqlx::query("INSERT OR IGNORE INTO users (username, password, role, status) VALUES ('root', ?, 100, 1)")
                .bind(&hash)
                .execute(pool)
                .await
                .expect("Failed to ensure root user")
                .rows_affected()
                > 0
        }
        DbPool::Postgres(pool) => {
            sqlx::query("INSERT INTO users (username, password, role, status) VALUES ('root', $1, 100, 1) ON CONFLICT (username) DO NOTHING")
                .bind(&hash)
                .execute(pool)
                .await
                .expect("Failed to ensure root user")
                .rows_affected()
                > 0
        }
    };

    if created {
        tracing::info!("Created default root user");
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
