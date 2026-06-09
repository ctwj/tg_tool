// 提取历史业务逻辑 — 写入/查询提取批次执行记录

use crate::errors::AppError;
use crate::models::extract_history::ExtractHistory;
use crate::state::DbPool;
use serde::Serialize;

/// 提取历史列表结果
#[derive(Debug, Serialize)]
pub struct ExtractHistoryListResult {
    pub list: Vec<ExtractHistory>,
    pub pagination: ExtractHistoryPagination,
}

#[derive(Debug, Serialize)]
pub struct ExtractHistoryPagination {
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

/// 提取历史统计
#[derive(Debug, Serialize)]
pub struct ExtractHistoryStats {
    pub total: i64,
    pub success: i64,
    pub failed: i64,
    pub last_extracted: i64,
}

/// 写入一条提取历史记录
pub async fn insert(
    db: &DbPool,
    status: &str,
    total_scanned: i64,
    extracted: i64,
    skipped: i64,
    errors: i64,
    message: Option<&str>,
) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO extract_histories \
                 (status, total_scanned, extracted, skipped, errors, message) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(status)
            .bind(total_scanned)
            .bind(extracted)
            .bind(skipped)
            .bind(errors)
            .bind(message)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO extract_histories \
                 (status, total_scanned, extracted, skipped, errors, message) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(status)
            .bind(total_scanned)
            .bind(extracted)
            .bind(skipped)
            .bind(errors)
            .bind(message)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 分页查询提取历史（按时间倒序）
pub async fn list(
    db: &DbPool,
    page: i64,
    page_size: i64,
) -> Result<ExtractHistoryListResult, AppError> {
    let offset = (page - 1).max(0) * page_size;

    let (list, total): (Vec<ExtractHistory>, i64) = match db {
        DbPool::Sqlite(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extract_histories")
                .fetch_one(pool)
                .await?;
            let list = sqlx::query_as(
                "SELECT id, status, total_scanned, extracted, skipped, errors, message, executed_at \
                 FROM extract_histories ORDER BY executed_at DESC LIMIT ? OFFSET ?",
            )
            .bind(page_size)
            .bind(offset)
            .fetch_all(pool)
            .await?;
            (list, total)
        }
        DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extract_histories")
                .fetch_one(pool)
                .await?;
            let list = sqlx::query_as(
                "SELECT id, status, total_scanned, extracted, skipped, errors, message, executed_at \
                 FROM extract_histories ORDER BY executed_at DESC LIMIT $1 OFFSET $2",
            )
            .bind(page_size)
            .bind(offset)
            .fetch_all(pool)
            .await?;
            (list, total)
        }
    };

    Ok(ExtractHistoryListResult {
        list,
        pagination: ExtractHistoryPagination {
            page,
            page_size,
            total,
        },
    })
}

/// 提取历史统计
pub async fn stats(db: &DbPool) -> Result<ExtractHistoryStats, AppError> {
    let (total, success, failed, last_extracted): (i64, i64, i64, i64) = match db {
        DbPool::Sqlite(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extract_histories")
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            let success: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM extract_histories WHERE status = 'success'",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            let failed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM extract_histories WHERE status = 'failed'",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            let last_extracted: i64 = sqlx::query_scalar(
                "SELECT COALESCE((SELECT extracted FROM extract_histories WHERE status = 'success' ORDER BY executed_at DESC LIMIT 1), 0)",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            (total, success, failed, last_extracted)
        }
        DbPool::Postgres(pool) => {
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM extract_histories")
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            let success: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM extract_histories WHERE status = 'success'",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            let failed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM extract_histories WHERE status = 'failed'",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            let last_extracted: i64 = sqlx::query_scalar(
                "SELECT COALESCE((SELECT extracted FROM extract_histories WHERE status = 'success' ORDER BY executed_at DESC LIMIT 1), 0)",
            )
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            (total, success, failed, last_extracted)
        }
    };

    Ok(ExtractHistoryStats {
        total,
        success,
        failed,
        last_extracted,
    })
}
