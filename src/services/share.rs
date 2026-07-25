// 分享记录服务（feature 047 US2）

use crate::errors::AppError;
use crate::models::share_record::ShareRecord;
use crate::state::DbPool;

pub async fn create(
    db: &DbPool,
    account_id: i64,
    file_name: &str,
    share_url: &str,
    extract_code: Option<String>,
    remote_file_id: Option<String>,
) -> Result<i64, AppError> {
    match db {
        DbPool::Sqlite(p) => {
            let res = sqlx::query(
                "INSERT INTO share_records (account_id, file_name, share_url, extract_code, remote_file_id) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(account_id)
            .bind(file_name)
            .bind(share_url)
            .bind(extract_code)
            .bind(remote_file_id)
            .execute(p)
            .await?;
            Ok(res.last_insert_rowid() as i64)
        }
        DbPool::Postgres(p) => {
            let (id,): (i64,) = sqlx::query_as(
                "INSERT INTO share_records (account_id, file_name, share_url, extract_code, remote_file_id) VALUES ($1, $2, $3, $4, $5) RETURNING id",
            )
            .bind(account_id)
            .bind(file_name)
            .bind(share_url)
            .bind(extract_code)
            .bind(remote_file_id)
            .fetch_one(p)
            .await?;
            Ok(id)
        }
    }
}

pub async fn get(db: &DbPool, id: i64) -> Result<ShareRecord, AppError> {
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query_as::<_, ShareRecord>("SELECT * FROM share_records WHERE id = ?")
                .bind(id)
                .fetch_optional(p)
                .await?
        }
        DbPool::Postgres(p) => {
            sqlx::query_as::<_, ShareRecord>("SELECT * FROM share_records WHERE id = $1")
                .bind(id)
                .fetch_optional(p)
                .await?
        }
    }
    .ok_or_else(|| AppError::NotFound(format!("分享记录 {id} 不存在")))
}
