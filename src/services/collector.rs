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
        "text": message_text_with_links(msg),
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

/// 展开消息文本中隐藏的超链接（TextUrl entity）为 markdown 形式
///
/// Telegram 消息里 "点击跳转" 这类超链接的 URL 存于 message entity，不在纯文本中；
/// 采集入库只存文本会导致 URL 永久丢失、资源提取找不到任何链接。
/// 此函数把 entity 携带的 URL 以 `[显示文字](URL)` 内联回文本。
pub fn message_text_with_links(msg: &grammers_client::types::Message) -> String {
    let text = msg.text();
    let entities = match msg.fmt_entities() {
        Some(e) if !e.is_empty() => e,
        _ => return text.to_string(),
    };
    let links: Vec<(i32, i32, String)> = entities
        .iter()
        .filter_map(|e| match e {
            grammers_client::grammers_tl_types::enums::MessageEntity::TextUrl(t) => {
                Some((t.offset, t.length, t.url.clone()))
            }
            _ => None,
        })
        .collect();
    inline_text_urls(text, &links)
}

/// 将 `links`（UTF-16 offset/length + URL）以 `[原文](URL)` 形式内联替换进 `text`
///
/// Telegram entity 的 offset/length 按 UTF-16 code unit 计（emoji 占 2），
/// 需转换为 byte 偏移后从后向前替换，避免前面的替换使后面的偏移失效。
fn inline_text_urls(text: &str, links: &[(i32, i32, String)]) -> String {
    if links.is_empty() {
        return text.to_string();
    }
    // utf16_to_byte[i] = 前 i 个 UTF-16 code unit 对应的 byte 偏移
    let mut utf16_to_byte: Vec<usize> = Vec::with_capacity(text.encode_utf16().count() + 1);
    utf16_to_byte.push(0);
    let mut acc = 0usize;
    for c in text.chars() {
        acc += c.len_utf8();
        for _ in 0..c.len_utf16() {
            utf16_to_byte.push(acc);
        }
    }
    let total_u16 = utf16_to_byte.len() - 1;

    // 按 offset 降序替换：后面的改动不影响前面待替换区间的 byte 偏移
    let mut sorted = links.to_vec();
    sorted.sort_by_key(|(o, _, _)| std::cmp::Reverse(*o));

    let mut result = text.to_string();
    for (offset, length, url) in sorted {
        let start = offset.max(0) as usize;
        let end = (offset.saturating_add(length)).max(0) as usize;
        if start >= end || start >= total_u16 || end > total_u16 {
            tracing::debug!(
                "TextUrl entity 越界忽略: offset={offset} length={length} total_utf16={total_u16}"
            );
            continue;
        }
        let b_start = utf16_to_byte[start];
        let b_end = utf16_to_byte[end];
        if b_start >= b_end || b_end > result.len() {
            continue;
        }
        let label = result[b_start..b_end].to_string();
        result.replace_range(b_start..b_end, &format!("[{label}]({url})"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 计算子串的 UTF-16 code unit 偏移（测试辅助，避免手数 emoji 长度）
    fn utf16_offset(text: &str, needle: &str) -> i32 {
        let b = text.find(needle).expect("needle 不在文本中");
        text[..b].encode_utf16().count() as i32
    }

    #[test]
    fn t_inline_text_urls_expands_hidden_link_with_emoji_prefix() {
        // 复现用户场景：emoji 开头的消息，链接藏于 "点击跳转" entity
        let text = "🔗 链接： 点击跳转";
        let off = utf16_offset(text, "点击跳转");
        let out = inline_text_urls(
            text,
            &[(
                off,
                "点击跳转".encode_utf16().count() as i32,
                "https://pan.quark.cn/s/abc123".to_string(),
            )],
        );
        assert_eq!(out, "🔗 链接： [点击跳转](https://pan.quark.cn/s/abc123)");
    }

    #[test]
    fn t_inline_text_urls_plain_ascii_boundary() {
        let text = "see this link now";
        let off = utf16_offset(text, "link");
        let out = inline_text_urls(text, &[(off, 4, "https://example.com/x".to_string())]);
        assert_eq!(out, "see this [link](https://example.com/x) now");
    }

    #[test]
    fn t_inline_text_urls_no_links_returns_original() {
        assert_eq!(inline_text_urls("纯文本", &[]), "纯文本");
    }

    #[test]
    fn t_inline_text_urls_out_of_range_entity_ignored() {
        let text = "短文本";
        // 越界 entity 不 panic、不改动
        assert_eq!(
            inline_text_urls(text, &[(100, 5, "https://x.io/1".to_string())]),
            "短文本"
        );
        // 空长度忽略
        assert_eq!(
            inline_text_urls(text, &[(0, 0, "https://x.io/2".to_string())]),
            "短文本"
        );
    }

    #[test]
    fn t_inline_text_urls_multiple_links_reversed_order() {
        // 两个链接（乱序传入），均正确展开
        let text = "第一处 here 第二处 there";
        let o1 = utf16_offset(text, "here");
        let o2 = utf16_offset(text, "there");
        let links = vec![
            (o1, 4, "https://a.io/1".to_string()),
            (o2, 5, "https://b.io/2".to_string()),
        ];
        let out = inline_text_urls(text, &links);
        assert_eq!(
            out,
            "第一处 [here](https://a.io/1) 第二处 [there](https://b.io/2)"
        );
    }

    /// 端到端复现：修复前该消息提取结果为空（AI/规则均无反应），展开后规则引擎可识别
    #[test]
    fn t_expanded_text_yields_netdisk_resource() {
        let text = "🎬 袒露 (2026)\n\n🔗 链接： 点击跳转";
        let label_len = "点击跳转".encode_utf16().count() as i32;
        let off = utf16_offset(text, "点击跳转");
        let expanded = inline_text_urls(
            text,
            &[(
                off,
                label_len,
                "https://pan.quark.cn/s/abcdef123".to_string(),
            )],
        );
        let drafts = crate::services::extractor::extract_resources(&expanded);
        assert_eq!(drafts.len(), 1, "展开后应提取出 1 条资源: {expanded}");
        assert!(
            drafts[0]
                .url
                .iter()
                .any(|u| u == "https://pan.quark.cn/s/abcdef123"),
            "URL 不应带 markdown 尾括号: {:?}",
            drafts[0].url
        );
        assert_eq!(drafts[0].category, "quark");
    }

    /// markdown 内联展开后的 URL 提取：不平衡尾括号修剪、平衡括号保留
    #[test]
    fn t_url_extraction_trims_unbalanced_parens() {
        use crate::services::extractor::extract_all_urls;
        let urls = extract_all_urls("[跳转](https://pan.quark.cn/s/abc123)");
        assert_eq!(urls, vec!["https://pan.quark.cn/s/abc123".to_string()]);

        // 平衡括号 URL（如 Wikipedia）保持原样
        let balanced = extract_all_urls("[w](https://zh.wikipedia.org/wiki/Foo_(bar))");
        assert_eq!(
            balanced,
            vec!["https://zh.wikipedia.org/wiki/Foo_(bar)".to_string()]
        );
    }
}
