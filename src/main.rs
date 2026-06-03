use tgTool::config::Config;
use tgTool::services::crypto;
use tgTool::services::tg_manager::TgManager;
use tgTool::state::{AppState, DbPool};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    // Load configuration
    let config = Config::load();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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
    let tg_clients = std::sync::Arc::new(tokio::sync::RwLock::new(
        std::collections::HashMap::new(),
    ));
    let option_cache = std::sync::Arc::new(tokio::sync::RwLock::new(
        std::collections::HashMap::new(),
    ));
    let peer_cache = std::sync::Arc::new(tokio::sync::RwLock::new(
        std::collections::HashMap::new(),
    ));
    let tg_manager = std::sync::Arc::new(TgManager::new(
        config.clone(),
        db_pool.clone(),
        tg_clients.clone(),
        option_cache.clone(),
        peer_cache.clone(),
    ));
    let state = AppState::new(db_pool.clone(), config.clone(), tg_manager.clone(), image_cache_dir);

    // Load options cache
    load_option_cache(&state).await;

    // Reconnect active Telegram clients
    let reconnected = tg_manager.reconnect_on_startup().await;
    if !reconnected.is_empty() {
        tracing::info!("Reconnected {} TG clients", reconnected.len());
    }

    // Start auto-reconnector for offline clients (every 30s)
    tg_manager.spawn_auto_reconnector(30);

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
            tracing::info!("Auto extract enabled, starting extract scheduler (interval: {}min)", extract_interval);
            tgTool::services::scheduler::start_extract_scheduler(
                state.extract_scheduler.clone(),
                extract_interval,
                state.db.clone(),
                state.option_cache.clone(),
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
            .after_connect(|conn, _meta| Box::pin(async move {
                use sqlx::Executor;
                // busy_timeout: 并发写入时等待锁而非直接报错（5秒）
                conn.execute(sqlx::query("PRAGMA busy_timeout=5000")).await?;
                // WAL 模式: 写操作不再阻塞读操作
                conn.execute(sqlx::query("PRAGMA journal_mode=WAL")).await?;
                // synchronous=NORMAL: WAL 模式下足够安全，性能更好
                conn.execute(sqlx::query("PRAGMA synchronous=NORMAL")).await?;
                // WAL auto-checkpoint: 避免 WAL 文件无限增长
                conn.execute(sqlx::query("PRAGMA wal_autocheckpoint=1000")).await?;
                Ok(())
            }))
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
                if !e.to_string().contains("already exists") && !e.to_string().contains("duplicate") {
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
            let m4 = include_str!("../migrations/004_collector_histories_is_extracted_postgres.sql");
            if let Err(e) = sqlx::raw_sql(m4).execute(pool).await {
                if !e.to_string().contains("already exists") && !e.to_string().contains("duplicate") {
                    panic!("Failed to run PostgreSQL migration 004: {e}");
                }
                tracing::debug!("PostgreSQL migration 004 skipped (already applied)");
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
    // Check if root user exists
    let exists: bool = match pool {
        DbPool::Sqlite(pool) => {
            let row: Option<(i64,)> =
                sqlx::query_as("SELECT id FROM users WHERE username = 'root'")
                    .fetch_optional(pool)
                    .await
                    .unwrap_or(None);
            row.is_some()
        }
        DbPool::Postgres(pool) => {
            let row: Option<(i64,)> =
                sqlx::query_as("SELECT id FROM users WHERE username = 'root'")
                    .fetch_optional(pool)
                    .await
                    .unwrap_or(None);
            row.is_some()
        }
    };

    if !exists {
        let hash = crypto::hash_password("123456").expect("Failed to hash root password");
        match pool {
            DbPool::Sqlite(pool) => {
                sqlx::query("INSERT INTO users (username, password, role, status) VALUES ('root', ?, 100, 1)")
                    .bind(&hash)
                    .execute(pool)
                    .await
                    .expect("Failed to create root user");
            }
            DbPool::Postgres(pool) => {
                sqlx::query("INSERT INTO users (username, password, role, status) VALUES ('root', $1, 100, 1)")
                    .bind(&hash)
                    .execute(pool)
                    .await
                    .expect("Failed to create root user");
            }
        }
        tracing::info!("Created default root user");
    }
}
