// API Key 服务（feature 047 US4）— 签发/校验/配额/吊销/轮换
// 明文仅创建/轮换时返回一次；落库仅 SHA-256 hash + 前缀

use sha2::{Digest, Sha256};

use crate::errors::AppError;
use crate::models::api_key::{ApiKey, ApiKeyView, CreateApiKey};
use crate::state::DbPool;

const STATUS_ENABLED: &str = "enabled";
const STATUS_DISABLED: &str = "disabled";

/// 生成明文 Key：pk_<32 hex>（uuid v4 CSPRNG）
pub fn generate_plaintext() -> String {
    format!("pk_{}", uuid::Uuid::new_v4().simple())
}

/// SHA-256 hex
pub fn hash_key(plaintext: &str) -> String {
    let mut h = Sha256::new();
    h.update(plaintext.as_bytes());
    format!("{:x}", h.finalize())
}

/// 创建 Key，返回（脱敏视图，明文——仅此一次）
pub async fn create(db: &DbPool, req: CreateApiKey) -> Result<(ApiKeyView, String), AppError> {
    if req.system_name.trim().is_empty() {
        return Err(AppError::BadRequest("system_name 不能为空".into()));
    }
    let plaintext = generate_plaintext();
    let hash = hash_key(&plaintext);
    let prefix: String = plaintext.chars().take(8).collect();
    let quota = req.quota_limit.unwrap_or(0).max(0);
    let qps = req.rate_limit_qps.filter(|&q| q > 0);

    let id: i64 = match db {
        DbPool::Sqlite(p) => {
            let res = sqlx::query(
                "INSERT INTO api_keys (system_name, key_hash, key_prefix, status, quota_limit, quota_used, rate_limit_qps) VALUES (?, ?, ?, ?, ?, 0, ?)",
            )
            .bind(&req.system_name)
            .bind(&hash)
            .bind(&prefix)
            .bind(STATUS_ENABLED)
            .bind(quota)
            .bind(qps)
            .execute(p)
            .await?;
            res.last_insert_rowid() as i64
        }
        DbPool::Postgres(p) => {
            let (id,): (i64,) = sqlx::query_as(
                "INSERT INTO api_keys (system_name, key_hash, key_prefix, status, quota_limit, quota_used, rate_limit_qps) VALUES ($1, $2, $3, $4, $5, 0, $6) RETURNING id",
            )
            .bind(&req.system_name)
            .bind(&hash)
            .bind(&prefix)
            .bind(STATUS_ENABLED)
            .bind(quota)
            .bind(qps)
            .fetch_one(p)
            .await?;
            id
        }
    };
    let view = get_view(db, id).await?;
    Ok((view, plaintext))
}

/// 校验明文 Key，返回 enabled 的 ApiKey（未找到/吊销 → Unauthorized）
pub async fn validate(db: &DbPool, plaintext: &str) -> Result<ApiKey, AppError> {
    let hash = hash_key(plaintext);
    let key = match db {
        DbPool::Sqlite(p) => {
            sqlx::query_as::<_, ApiKey>(
                "SELECT * FROM api_keys WHERE key_hash = ? AND status = 'enabled'",
            )
            .bind(&hash)
            .fetch_optional(p)
            .await?
        }
        DbPool::Postgres(p) => {
            sqlx::query_as::<_, ApiKey>(
                "SELECT * FROM api_keys WHERE key_hash = $1 AND status = 'enabled'",
            )
            .bind(&hash)
            .fetch_optional(p)
            .await?
        }
    };
    key.ok_or_else(|| AppError::Unauthorized("无效或已吊销的 API Key".into()))
}

/// 消费一次配额：超限返回 false（调用方应返回 429）
pub async fn consume_quota(db: &DbPool, id: i64) -> Result<bool, AppError> {
    let key = get_full(db, id).await?;
    if key.quota_limit > 0 && key.quota_used >= key.quota_limit {
        return Ok(false);
    }
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query("UPDATE api_keys SET quota_used = quota_used + 1 WHERE id = ?")
                .bind(id)
                .execute(p)
                .await?;
        }
        DbPool::Postgres(p) => {
            sqlx::query("UPDATE api_keys SET quota_used = quota_used + 1 WHERE id = $1")
                .bind(id)
                .execute(p)
                .await?;
        }
    }
    Ok(true)
}

pub async fn revoke(db: &DbPool, id: i64) -> Result<ApiKeyView, AppError> {
    let now = chrono::Utc::now().naive_utc();
    let affected = match db {
        DbPool::Sqlite(p) => sqlx::query(
            "UPDATE api_keys SET status = ?, revoked_at = ? WHERE id = ? AND status = 'enabled'",
        )
        .bind(STATUS_DISABLED)
        .bind(now)
        .bind(id)
        .execute(p)
        .await?
        .rows_affected(),
        DbPool::Postgres(p) => sqlx::query(
            "UPDATE api_keys SET status = $1, revoked_at = $2 WHERE id = $3 AND status = 'enabled'",
        )
        .bind(STATUS_DISABLED)
        .bind(now)
        .bind(id)
        .execute(p)
        .await?
        .rows_affected(),
    };
    if affected == 0 {
        return Err(AppError::NotFound(format!("API Key {id} 不存在或已吊销")));
    }
    get_view(db, id).await
}

/// 轮换：旧 Key 吊销，签发新 Key，返回（新视图，新明文）
pub async fn rotate(db: &DbPool, id: i64) -> Result<(ApiKeyView, String), AppError> {
    let now = chrono::Utc::now().naive_utc();
    let old = get_full(db, id).await?;
    if old.status != STATUS_ENABLED {
        return Err(AppError::BadRequest("仅 enabled 的 Key 可轮换".into()));
    }
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query("UPDATE api_keys SET status = ?, rotated_at = ? WHERE id = ?")
                .bind(STATUS_DISABLED)
                .bind(now)
                .bind(id)
                .execute(p)
                .await?;
        }
        DbPool::Postgres(p) => {
            sqlx::query("UPDATE api_keys SET status = $1, rotated_at = $2 WHERE id = $3")
                .bind(STATUS_DISABLED)
                .bind(now)
                .bind(id)
                .execute(p)
                .await?;
        }
    }
    create(
        db,
        CreateApiKey {
            system_name: old.system_name,
            quota_limit: Some(old.quota_limit),
            rate_limit_qps: old.rate_limit_qps,
        },
    )
    .await
}

pub async fn list(db: &DbPool) -> Result<Vec<ApiKeyView>, AppError> {
    let rows = match db {
        DbPool::Sqlite(p) => {
            sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys ORDER BY id DESC")
                .fetch_all(p)
                .await?
        }
        DbPool::Postgres(p) => {
            sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys ORDER BY id DESC")
                .fetch_all(p)
                .await?
        }
    };
    Ok(rows.into_iter().map(Into::into).collect())
}

async fn get_full(db: &DbPool, id: i64) -> Result<ApiKey, AppError> {
    match db {
        DbPool::Sqlite(p) => {
            sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys WHERE id = ?")
                .bind(id)
                .fetch_optional(p)
                .await?
        }
        DbPool::Postgres(p) => {
            sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys WHERE id = $1")
                .bind(id)
                .fetch_optional(p)
                .await?
        }
    }
    .ok_or_else(|| AppError::NotFound(format!("API Key {id} 不存在")))
}

async fn get_view(db: &DbPool, id: i64) -> Result<ApiKeyView, AppError> {
    Ok(get_full(db, id).await?.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup() -> DbPool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let m = include_str!("../../migrations/040_pan_management_sqlite.sql");
        sqlx::raw_sql(m).execute(&pool).await.unwrap();
        DbPool::Sqlite(pool)
    }

    #[tokio::test]
    async fn test_create_returns_plaintext_and_view_no_hash_leak() {
        let db = setup().await;
        let (view, plaintext) = create(
            &db,
            CreateApiKey {
                system_name: "sys-a".into(),
                quota_limit: Some(10),
                rate_limit_qps: None,
            },
        )
        .await
        .unwrap();
        assert!(plaintext.starts_with("pk_"));
        assert_eq!(view.system_name, "sys-a");
        assert_eq!(view.status, "enabled");
        assert_eq!(view.quota_limit, 10);
        // 视图脱敏：序列化不含 key_hash 与明文
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("key_hash"));
        assert!(!json.contains(&plaintext));
    }

    #[tokio::test]
    async fn test_validate_correct_plaintext_ok() {
        let db = setup().await;
        let (_, plaintext) = create(
            &db,
            CreateApiKey {
                system_name: "s".into(),
                quota_limit: None,
                rate_limit_qps: None,
            },
        )
        .await
        .unwrap();
        let k = validate(&db, &plaintext).await.unwrap();
        assert_eq!(k.status, "enabled");
    }

    #[tokio::test]
    async fn test_validate_wrong_plaintext_unauthorized() {
        let db = setup().await;
        assert!(validate(&db, "pk_wrong").await.is_err());
    }

    #[tokio::test]
    async fn test_consume_quota_then_exceeds() {
        let db = setup().await;
        let (view, _) = create(
            &db,
            CreateApiKey {
                system_name: "s".into(),
                quota_limit: Some(2),
                rate_limit_qps: None,
            },
        )
        .await
        .unwrap();
        assert!(consume_quota(&db, view.id).await.unwrap());
        assert!(consume_quota(&db, view.id).await.unwrap());
        assert!(!consume_quota(&db, view.id).await.unwrap()); // 第 3 次超限
    }

    #[tokio::test]
    async fn test_consume_quota_unlimited_never_exceeds() {
        let db = setup().await;
        let (view, _) = create(
            &db,
            CreateApiKey {
                system_name: "s".into(),
                quota_limit: Some(0),
                rate_limit_qps: None,
            },
        )
        .await
        .unwrap();
        for _ in 0..100 {
            assert!(consume_quota(&db, view.id).await.unwrap());
        }
    }

    #[tokio::test]
    async fn test_revoke_blocks_validate() {
        let db = setup().await;
        let (view, plaintext) = create(
            &db,
            CreateApiKey {
                system_name: "s".into(),
                quota_limit: None,
                rate_limit_qps: None,
            },
        )
        .await
        .unwrap();
        revoke(&db, view.id).await.unwrap();
        assert!(validate(&db, &plaintext).await.is_err()); // 吊销后失效
    }

    #[tokio::test]
    async fn test_rotate_invalidates_old() {
        let db = setup().await;
        let (view, old_plain) = create(
            &db,
            CreateApiKey {
                system_name: "s".into(),
                quota_limit: None,
                rate_limit_qps: None,
            },
        )
        .await
        .unwrap();
        let (new_view, new_plain) = rotate(&db, view.id).await.unwrap();
        assert_ne!(old_plain, new_plain);
        assert!(validate(&db, &old_plain).await.is_err()); // 旧 Key 失效
        assert!(validate(&db, &new_plain).await.is_ok()); // 新 Key 可用
        assert_eq!(new_view.system_name, "s");
    }
}
