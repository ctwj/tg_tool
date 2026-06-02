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
    tg_clients: &crate::state::TgClientMap,
    db: &DbPool,
    option_cache: &crate::state::OptionCache,
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

    let mut messages = client.iter_messages(packed).limit(1000);
    let mut collected = 0usize;

    while let Some(msg) = messages.next().await
        .map_err(|e| AppError::Internal(format!("获取消息失败: {e}")))?
    {
        let message_id = msg.id() as i64;
        let raw_data = serialize_message_for_collection(&msg);
        let post_time = msg.date().naive_utc();

        // 检查图片媒体并上传图床
        let remote_id = upload_photo_if_needed(&msg, tg_clients, option_cache).await;

        let inserted = match db {
            crate::state::DbPool::Sqlite(pool) => {
                let result = sqlx::query(
                    "INSERT OR IGNORE INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data, remote_id) VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(collector_id)
                .bind(channel_id)
                .bind(message_id)
                .bind(post_time)
                .bind(&raw_data)
                .bind(&remote_id)
                .execute(pool)
                .await;
                match result {
                    Ok(r) => r.rows_affected() > 0,
                    Err(_) => false,
                }
            }
            crate::state::DbPool::Postgres(pool) => {
                let result = sqlx::query(
                    "INSERT INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data, remote_id) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (channel_id, message_id) DO NOTHING",
                )
                .bind(collector_id)
                .bind(channel_id)
                .bind(message_id)
                .bind(post_time)
                .bind(&raw_data)
                .bind(&remote_id)
                .execute(pool)
                .await;
                match result {
                    Ok(r) => r.rows_affected() > 0,
                    Err(_) => false,
                }
            }
        };

        if inserted {
            collected += 1;
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
    db: &DbPool,
) -> Result<(), AppError> {
    match db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT OR IGNORE INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(collector_id)
            .bind(channel_id)
            .bind(message_id)
            .bind(post_time)
            .bind(raw_data)
            .execute(pool)
            .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO collector_histories (collector_id, channel_id, message_id, post_time, raw_data) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (channel_id, message_id) DO NOTHING",
            )
            .bind(collector_id)
            .bind(channel_id)
            .bind(message_id)
            .bind(post_time)
            .bind(raw_data)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 检查消息是否包含图片，如果是则上传到图床群组并返回 remote_id
async fn upload_photo_if_needed(
    msg: &grammers_client::types::Message,
    tg_clients: &crate::state::TgClientMap,
    option_cache: &crate::state::OptionCache,
) -> Option<String> {
    // 检查是否有图片媒体
    let media = msg.media()?;
    let photo = match media {
        grammers_client::types::Media::Photo(p) => p,
        _ => return None,
    };

    // 获取图床群组 ID
    let image_group_id: i64 = {
        let cache = option_cache.read().await;
        let group_str = cache.get("TelegramImageGroup")?;
        group_str.parse().ok()?
    };

    // 获取活跃客户端
    let client = {
        let clients = tg_clients.read().await;
        clients
            .values()
            .find(|e| e.status == "active" && e.client.is_some())
            .and_then(|e| e.client.clone())?
    };

    // 通过遍历 dialogs 解析图床群组的 PackedChat
    let mut dialogs = client.iter_dialogs();
    let mut target_packed = None;
    while let Ok(Some(dialog)) = dialogs.next().await {
        if dialog.chat().id() == image_group_id {
            target_packed = Some(dialog.chat().pack());
            break;
        }
    }

    let packed = target_packed?;

    // 转发原始消息到图床群组（保留图片、格式等完整内容）
    let chat = msg.chat().pack();
    match client.forward_messages(chat, &[msg.id()], packed).await {
        Ok(_) => {
            tracing::info!("已转发图片消息到图床群组 {}", image_group_id);
            Some(format!("{}", photo.id()))
        }
        Err(e) => {
            tracing::warn!("转发图片消息到图床失败: {e}");
            None
        }
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
