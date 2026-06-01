use tgTool::config::Config;
use tgTool::services::crypto;
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

    tracing::info!("TG Forwarding Tool v{} starting...", env!("CARGO_PKG_VERSION"));

    // Initialize database
    let db_pool = init_database(&config).await;
    tracing::info!("Database initialized");

    // Run migrations
    run_migrations(&db_pool).await;
    tracing::info!("Database migrations completed");

    // Ensure root user exists with a valid bcrypt hash
    ensure_root_user(&db_pool).await;

    // Build application state
    let state = AppState::new(db_pool, config.clone());

    // Load options cache
    load_option_cache(&state).await;

    // Build router
    let app = tgTool::routes::build_router(state.clone())
        .layer(tgTool::middleware::cors::cors_layer())
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
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
            .connect(&database_url)
            .await
            .expect("Failed to connect to SQLite");
        DbPool::Sqlite(pool)
    }
}

async fn run_migrations(pool: &DbPool) {
    // Read and execute the migration SQL
    let migration_sql = include_str!("../migrations/001_init.sql");

    match pool {
        DbPool::Sqlite(pool) => {
            sqlx::raw_sql(migration_sql)
                .execute(pool)
                .await
                .expect("Failed to run SQLite migrations");
        }
        DbPool::Postgres(pool) => {
            sqlx::raw_sql(migration_sql)
                .execute(pool)
                .await
                .expect("Failed to run PostgreSQL migrations");
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
            let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = 'root'")
                .fetch_optional(pool)
                .await
                .unwrap_or(None);
            row.is_some()
        }
        DbPool::Postgres(pool) => {
            let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = 'root'")
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
