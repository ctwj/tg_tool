// Collection service
// Full collection (batch fetch history) and real-time collection

use crate::errors::AppError;
use crate::state::DbPool;
use chrono::NaiveDateTime;

/// Trigger full history collection for a collector
pub async fn full_collect(
    collector_id: i64,
    client_id: &str,
    channel_id: i64,
    limit: i64,
    tg_clients: &crate::state::TgClientMap,
    db: &DbPool,
) -> Result<usize, AppError> {
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

    let packed = target_packed
        .ok_or_else(|| AppError::NotFound(format!("未找到频道: {channel_id}")))?;

    let mut messages = client.iter_messages(packed).limit(limit as usize);
    let mut collected = 0usize;

    // 使用事务批量写入，大幅提升 SQLite 性能
    // 先收集所有消息到内存，然后一次性写入数据库
    let mut batch: Vec<(i64, NaiveDateTime, String, Option<String>)> = Vec::new();

    while let Some(msg) = messages.next().await
        .map_err(|e| AppError::Internal(format!("获取消息失败: {e}")))?
    {
        let message_id = msg.id() as i64;
        let raw_data = serialize_message_for_collection(&msg);
        let post_time = msg.date().naive_utc();

        // 提取图片 media 的 photo_id（无需转发图床，由图片代理按需下载）
        let remote_id = extract_photo_id(&msg);

        batch.push((message_id, post_time, raw_data, remote_id));
    }

    // 批量写入数据库（使用事务）
    if !batch.is_empty() {
        match db {
            crate::state::DbPool::Sqlite(pool) => {
                // 开启事务
                let mut tx = pool.begin().await
                    .map_err(|e| AppError::Internal(format!("开启事务失败: {e}")))?;

                for (message_id, post_time, raw_data, remote_id) in &batch {
                    let result = sqlx::query(
                        "INSERT OR IGNORE INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data, remote_id) VALUES (?, ?, ?, ?, ?, ?)",
                    )
                    .bind(collector_id)
                    .bind(channel_id)
                    .bind(message_id)
                    .bind(post_time)
                    .bind(raw_data)
                    .bind(remote_id)
                    .execute(&mut *tx)
                    .await;
                    if let Ok(r) = result {
                        if r.rows_affected() > 0 {
                            collected += 1;
                        }
                    }
                }

                tx.commit().await
                    .map_err(|e| AppError::Internal(format!("提交事务失败: {e}")))?;
            }
            crate::state::DbPool::Postgres(pool) => {
                let mut tx = pool.begin().await
                    .map_err(|e| AppError::Internal(format!("开启事务失败: {e}")))?;

                for (message_id, post_time, raw_data, remote_id) in &batch {
                    let result = sqlx::query(
                        "INSERT INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data, remote_id) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (channel_id, message_id) DO NOTHING",
                    )
                    .bind(collector_id)
                    .bind(channel_id)
                    .bind(message_id)
                    .bind(post_time)
                    .bind(raw_data)
                    .bind(remote_id)
                    .execute(&mut *tx)
                    .await;
                    if let Ok(r) = result {
                        if r.rows_affected() > 0 {
                            collected += 1;
                        }
                    }
                }

                tx.commit().await
                    .map_err(|e| AppError::Internal(format!("提交事务失败: {e}")))?;
            }
        }
    }

    tracing::info!("Collected {} new messages for collector {}", collected, collector_id);
    Ok(collected)
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
