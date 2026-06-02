// Push scheduling service
// Delegates to resource service for push logic

use crate::errors::AppError;
use crate::state::DbPool;

/// Trigger a push batch — 委托给 resource::push_resources
pub async fn trigger_push(
    api_url: &str,
    api_token: &str,
    target: &str,
    batch_size: i64,
    db: &DbPool,
    option_cache: &crate::state::OptionCache,
) -> Result<serde_json::Value, AppError> {
    crate::services::resource::push_resources(
        api_url, api_token, target, batch_size, db, option_cache,
    )
    .await
}

/// Get push statistics
pub async fn get_stats(db: &DbPool) -> Result<serde_json::Value, AppError> {
    let (total, success, failed): (i64, i64, i64) = match db {
        crate::state::DbPool::Sqlite(pool) => {
            let total: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM push_histories")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let success: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM push_histories WHERE status = ?")
                    .bind("success")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let failed: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM push_histories WHERE status = ?")
                    .bind("failed")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            (total, success, failed)
        }
        crate::state::DbPool::Postgres(pool) => {
            let total: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM push_histories")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let success: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM push_histories WHERE status = $1")
                    .bind("success")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            let failed: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM push_histories WHERE status = $1")
                    .bind("failed")
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            (total, success, failed)
        }
    };
    Ok(serde_json::json!({ "total": total, "success": success, "failed": failed }))
}
