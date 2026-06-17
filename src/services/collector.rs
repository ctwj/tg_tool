// Collection service
// Full collection (batch fetch history) and real-time collection

use crate::errors::AppError;
use crate::state::DbPool;
use chrono::NaiveDateTime;

/// Trigger full history collection for a collector
///
/// Fetches messages in pages (Telegram returns max 100 per API call),
/// writes to DB in batches of `BATCH_SIZE` to avoid memory spikes.
pub async fn full_collect(
    collector_id: i64,
    client_id: &str,
    channel_id: i64,
    limit: i64,
    tg_clients: &crate::state::TgClientMap,
    db: &DbPool,
) -> Result<usize, AppError> {
    const BATCH_SIZE: usize = 500;

    let clients = tg_clients.read().await;
    let client = clients
        .get(client_id)
        .and_then(|e| e.client.clone())
        .ok_or_else(|| AppError::NotFound("客户端未连接".into()))?;
    drop(clients);

    // Resolve the channel peer by searching dialogs
    let mut dialogs = client.iter_dialogs();
    let mut target_packed = None;
    while let Some(dialog) = dialogs
        .next()
        .await
        .map_err(|e| AppError::Internal(format!("搜索频道失败: {e}")))?
    {
        if dialog.chat().id() == channel_id {
            target_packed = Some(dialog.chat().pack());
            break;
        }
    }

    let packed =
        target_packed.ok_or_else(|| AppError::NotFound(format!("未找到频道: {channel_id}")))?;

    let mut messages = client.iter_messages(packed).limit(limit as usize);
    let mut collected = 0usize;
    let mut batch: Vec<(i64, NaiveDateTime, String, Option<String>)> =
        Vec::with_capacity(BATCH_SIZE);
    let mut fetched = 0usize;
    let mut since_delay = 0usize; // 距离上次延迟后取了多少条

    while let Some(msg) = messages
        .next()
        .await
        .map_err(|e| AppError::Internal(format!("获取消息失败 (已取 {fetched} 条): {e}")))?
    {
        fetched += 1;
        since_delay += 1;

        let message_id = msg.id() as i64;
        let raw_data = serialize_message_for_collection(&msg);
        let post_time = msg.date().naive_utc();
        let remote_id = extract_photo_id(&msg);

        batch.push((message_id, post_time, raw_data, remote_id));

        // 每 BATCH_SIZE 条写一次库，释放内存
        if batch.len() >= BATCH_SIZE {
            collected += write_batch(collector_id, channel_id, &batch, db).await;
            tracing::info!("Progress: fetched {fetched}, collected {collected}");
            batch.clear();
        }

        // 每 100 条消息（≈ 1 次 Telegram API 调用）后等待 1.5 秒
        // Telegram 限制约 30 次/分钟，1.5 秒间隔 ≈ 40 次/分钟，留有余量
        if since_delay >= 100 {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            since_delay = 0;
        }
    }

    // 写入剩余数据
    if !batch.is_empty() {
        collected += write_batch(collector_id, channel_id, &batch, db).await;
    }

    tracing::info!(
        "Collected {} new messages for collector {} (fetched {} total)",
        collected,
        collector_id,
        fetched
    );
    Ok(collected)
}

/// Write a batch of messages to the database in a single transaction
async fn write_batch(
    collector_id: i64,
    channel_id: i64,
    batch: &[(i64, NaiveDateTime, String, Option<String>)],
    db: &DbPool,
) -> usize {
    if batch.is_empty() {
        return 0;
    }
    let mut inserted = 0usize;
    match db {
        crate::state::DbPool::Sqlite(pool) => {
            if let Ok(mut tx) = pool.begin().await {
                for (message_id, post_time, raw_data, remote_id) in batch {
                    if let Ok(r) = sqlx::query(
                        "INSERT OR IGNORE INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data, remote_id) VALUES (?, ?, ?, ?, ?, ?)",
                    )
                    .bind(collector_id)
                    .bind(channel_id)
                    .bind(message_id)
                    .bind(post_time)
                    .bind(raw_data)
                    .bind(remote_id)
                    .execute(&mut *tx)
                    .await
                        && r.rows_affected() > 0
                    {
                            inserted += 1;
                    }
                }
                if let Err(e) = tx.commit().await {
                    tracing::error!("采集批次事务提交失败（整批回滚）: {e}");
                    return 0;
                }
            }
        }
        crate::state::DbPool::Postgres(pool) => {
            if let Ok(mut tx) = pool.begin().await {
                for (message_id, post_time, raw_data, remote_id) in batch {
                    if let Ok(r) = sqlx::query(
                        "INSERT INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data, remote_id) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (channel_id, message_id) DO NOTHING",
                    )
                    .bind(collector_id)
                    .bind(channel_id)
                    .bind(message_id)
                    .bind(post_time)
                    .bind(raw_data)
                    .bind(remote_id)
                    .execute(&mut *tx)
                    .await
                        && r.rows_affected() > 0
                    {
                            inserted += 1;
                    }
                }
                if let Err(e) = tx.commit().await {
                    tracing::error!("采集批次事务提交失败（整批回滚）: {e}");
                    return 0;
                }
            }
        }
    }
    inserted
}

/// Save a real-time collected message
pub async fn save_realtime_message(
    collector_id: i64,
    channel_id: i64,
    message_id: i64,
    raw_data: &str,
    post_time: NaiveDateTime,
    remote_id: Option<&str>,
    db: &DbPool,
) -> Result<(), AppError> {
    match db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT OR IGNORE INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data, remote_id) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(collector_id)
            .bind(channel_id)
            .bind(message_id)
            .bind(post_time)
            .bind(raw_data)
            .bind(remote_id)
            .execute(pool)
            .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data, remote_id) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (channel_id, message_id) DO NOTHING",
            )
            .bind(collector_id)
            .bind(channel_id)
            .bind(message_id)
            .bind(post_time)
            .bind(raw_data)
            .bind(remote_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 提取消息中的图片 photo_id（直接从消息获取，无需转发图床）
pub fn extract_photo_id(msg: &grammers_client::types::Message) -> Option<String> {
    let media = msg.media()?;
    match media {
        grammers_client::types::Media::Photo(photo) => Some(format!("{}", photo.id())),
        _ => None,
    }
}

/// Serialize a grammers Message to JSON for collection storage
fn serialize_message_for_collection(msg: &grammers_client::types::Message) -> String {
    let mut json = serde_json::json!({
        "id": msg.id(),
        "date": msg.date().timestamp(),
        "text": msg.text(),
        "outgoing": msg.outgoing(),
        "chat_id": msg.chat().id(),
    });

    // 包含媒体信息
    if let Some(media) = msg.media() {
        match media {
            grammers_client::types::Media::Photo(photo) => {
                json["media_type"] = serde_json::json!("photo");
                json["photo_id"] = serde_json::json!(format!("{}", photo.id()));
            }
            grammers_client::types::Media::Document(doc) => {
                json["media_type"] = serde_json::json!("document");
                json["document_name"] = serde_json::json!(doc.name());
            }
            _ => {}
        }
    }

    json.to_string()
}
