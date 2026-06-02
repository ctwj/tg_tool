// Push scheduling and analysis service
// Extract resources from collector histories, analyze content, push to external API

use crate::errors::AppError;
use crate::state::DbPool;

/// Trigger a push batch
pub async fn trigger_push(
    api_url: &str,
    api_token: &str,
    target: &str,
    batch_size: i64,
    db: &DbPool,
    option_cache: &crate::state::OptionCache,
) -> Result<serde_json::Value, AppError> {
    let batch_id = format!("batch_{}_{}", target, chrono::Utc::now().timestamp());

    // 1. Get unpushed collector histories
    let histories: Vec<(i64, i64, i64, Option<String>)> = match db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT id, channel_id, message_id, raw_data FROM collector_histories WHERE is_auto_push = 0 LIMIT ?",
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT id, channel_id, message_id, raw_data FROM collector_histories WHERE is_auto_push = false LIMIT $1",
            )
            .bind(batch_size)
            .fetch_all(pool)
            .await?
        }
    };

    if histories.is_empty() {
        // Record empty push
        record_push_history(&batch_id, target, "success", 0, "没有需要推送的数据", None, db).await?;
        return Ok(serde_json::json!({
            "status": "success",
            "message": "没有需要推送的数据",
            "count": 0
        }));
    }

    // 2. Analyze messages into structured data
    let mut resources = Vec::new();
    for (_id, channel_id, message_id, raw_data) in &histories {
        if let Some(raw) = raw_data {
            let analyzed = analyze_message(raw, *channel_id, *message_id, option_cache).await;
            resources.push(analyzed);
        }
    }
    resources.retain(|r| !r["title"].as_str().unwrap_or("").is_empty());

    // 3. Push to external API
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Internal(format!("创建 HTTP 客户端失败: {e}")))?;

    let payload = serde_json::json!({ "resources": resources });

    let resp = client
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("X-API-Token", api_token)
        .json(&payload)
        .send()
        .await;

    match resp {
        Ok(response) => {
            let status_code = response.status();
            if status_code.is_success() {
                // Mark as pushed
                for (_id, channel_id, message_id, _) in &histories {
                    mark_as_pushed(*channel_id, *message_id, db).await?;
                }
                record_push_history(
                    &batch_id, target, "success",
                    histories.len() as i64,
                    "推送成功", None, db,
                ).await?;
                Ok(serde_json::json!({
                    "status": "success",
                    "processed_count": histories.len(),
                    "batch_id": batch_id
                }))
            } else {
                let body = response.text().await.unwrap_or_default();
                record_push_history(
                    &batch_id, target, "failed",
                    histories.len() as i64,
                    &format!("API返回错误: {}", status_code),
                    Some(&body), db,
                ).await?;
                Err(AppError::Internal(format!(
                    "推送API返回错误: status={}, body={}",
                    status_code, body
                )))
            }
        }
        Err(e) => {
            record_push_history(
                &batch_id, target, "failed",
                0,
                "推送请求失败",
                Some(&e.to_string()), db,
            ).await?;
            Err(AppError::Internal(format!("推送请求失败: {e}")))
        }
    }
}

/// Analyze a raw message into structured data for push
async fn analyze_message(
    raw: &str,
    _channel_id: i64,
    message_id: i64,
    option_cache: &crate::state::OptionCache,
) -> serde_json::Value {
    // Parse the raw message to extract text
    let msg: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
    let text = extract_text_from_message(&msg);

    // Extract links using regex
    let url_regex = match regex::Regex::new(r#"https?://[^\s<>"]+"#) {
        Ok(re) => re,
        Err(_) => return serde_json::json!({}),
    };
    let links: Vec<&str> = url_regex.find_iter(&text).map(|m| m.as_str()).collect();

    // Extract title (first non-empty line, max 50 chars)
    let title = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| {
            let t = l.trim();
            let chars: Vec<char> = t.chars().collect();
            if chars.len() > 50 {
                format!("{}...", chars[..50].iter().collect::<String>())
            } else {
                t.to_string()
            }
        })
        .unwrap_or_else(|| format!("消息_{}", message_id));

    // Build image URL from remote_id + domain
    let img = {
        let cache = option_cache.read().await;
        let remote_id = msg
            .get("remote_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !remote_id.is_empty() {
            let domain = cache.get("TelegramImageDomain").map(|s| s.trim_end_matches('/')).unwrap_or("");
            if !domain.is_empty() {
                format!("{}/{}", domain, remote_id)
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    };

    serde_json::json!({
        "title": title,
        "url": links,
        "description": "",
        "category": "",
        "tags": "",
        "img": img,
        "source": "tg",
        "extra": ""
    })
}

/// Extract text from a serialized grammers Message JSON
fn extract_text_from_message(msg: &serde_json::Value) -> String {
    // The text field in grammers message serialization
    msg.get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

async fn mark_as_pushed(channel_id: i64, message_id: i64, db: &DbPool) -> Result<(), AppError> {
    match db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE collector_histories SET is_auto_push = 1 WHERE channel_id = ? AND message_id = ?",
            )
            .bind(channel_id)
            .bind(message_id)
            .execute(pool)
            .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE collector_histories SET is_auto_push = true WHERE channel_id = $1 AND message_id = $2",
            )
            .bind(channel_id)
            .bind(message_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn record_push_history(
    batch_id: &str,
    target: &str,
    status: &str,
    data_count: i64,
    message: &str,
    error_msg: Option<&str>,
    db: &DbPool,
) -> Result<(), AppError> {
    match db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO push_histories (batch_id, target, status, data_count, message, error_msg) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(batch_id)
            .bind(target)
            .bind(status)
            .bind(data_count)
            .bind(message)
            .bind(error_msg)
            .execute(pool)
            .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO push_histories (batch_id, target, status, data_count, message, error_msg) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(batch_id)
            .bind(target)
            .bind(status)
            .bind(data_count)
            .bind(message)
            .bind(error_msg)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
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
