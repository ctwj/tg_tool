//! 图片下载→上传图床异步管线（research.md R3 + R7）— Phase 3 T023
//!
//! 设计要点：
//! - 复用 `forward_queue.rs` 的 `XxxSchedulerState + CancellationToken` 模式
//! - 30s tick 扫描 `crawler_article_images` 中 `status IN ('pending','failed') AND retry_count < 3`
//! - 流程：reqwest 下载外站图 → 缓存到 `image_cache_dir/crawler/<sha1>.<ext>`
//!   → `grammers_client::Client::upload_file(local_path)` 得到 `Uploaded`
//!   → `send_message(image_bed_group_a, InputMessage::text("").photo(uploaded))` 取得消息 ID
//!   → 写回 `image_message_id + status='uploaded'`
//! - 失败：`retry_count++ + last_error + status='failed'`，指数退避(10s/30s/120s) 在 Rust 侧过滤
//! - 跳过条件：无可用 Telegram 客户端、`ImageGroupChatId` 未配置、已达 `MAX_RETRIES`(3)
//!
//! grammers-client 0.7 API（U1 已通过 context7 确认）：
//!   `client.upload_file(path: P) -> Result<Uploaded, io::Error>`
//!   `client.send_message(chat, InputMessage::text("...").photo(uploaded)) -> Result<Option<Message>,>`
//!   无 `send_file`；走 `upload_file + send_message(photo=Uploaded)`。

use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::state::{AppState, DbPool};

/// 单次最多处理的图片条目（防止 tick 过载）
const BATCH_LIMIT: i64 = 20;
/// 最大重试次数（FR-028a）
const MAX_RETRIES: i64 = 3;
/// 退避表（秒）：按 retry_count 索引
const BACKOFF_SECS: &[u64] = &[10, 30, 120];

/// 调度器运行时状态
#[derive(Debug)]
pub struct ImageUploaderState {
    pub running: bool,
    pub scan_interval_secs: u64,
    pub started_at: Option<std::time::Instant>,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
}

pub type ImageUploaderHandle = Arc<RwLock<ImageUploaderState>>;

/// 创建未启动的句柄
pub fn create_uploader() -> ImageUploaderHandle {
    Arc::new(RwLock::new(ImageUploaderState {
        running: false,
        scan_interval_secs: 30,
        started_at: None,
        handle: None,
        cancel: None,
    }))
}

/// 启动图片上传 worker
///
/// - 若已在运行：直接返回
/// - 否则：spawn 一个 30s tick 的 worker
pub async fn start_uploader(state: AppState) {
    let mut s = state.crawler_image_uploader.write().await;
    if s.running {
        return;
    }
    let cancel = CancellationToken::new();
    s.cancel = Some(cancel.clone());
    s.running = true;
    s.started_at = Some(std::time::Instant::now());

    let interval = s.scan_interval_secs;
    let handle = tokio::spawn(run_loop(state.clone(), cancel, interval));
    s.handle = Some(handle);
    tracing::info!("Crawler image uploader started ({interval}s tick)");
}

async fn run_loop(state: AppState, cancel: CancellationToken, interval_secs: u64) {
    let duration = Duration::from_secs(interval_secs);
    loop {
        tokio::select! {
            _ = tokio::time::sleep(duration) => {
                if let Err(e) = tick(&state).await {
                    tracing::warn!("Crawler image uploader tick error: {e}");
                }
            }
            _ = cancel.cancelled() => {
                tracing::info!("Crawler image uploader cancelled");
                break;
            }
        }
    }
}

/// 单次 tick：扫描待处理图片，逐条处理
async fn tick(state: &AppState) -> Result<(), String> {
    let rows = fetch_pending_images(&state.db, BATCH_LIMIT).await?;
    if rows.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().naive_utc();
    let mut processed = 0usize;
    let mut skipped_backoff = 0usize;

    for row in rows {
        // 指数退避：updated_at 距 now 不足窗口则跳过
        let backoff_idx = row.retry_count.clamp(0, BACKOFF_SECS.len() as i64 - 1) as usize;
        let wait_secs = BACKOFF_SECS[backoff_idx];
        let elapsed = now.signed_duration_since(row.updated_at).num_seconds();
        if elapsed < wait_secs as i64 {
            skipped_backoff += 1;
            continue;
        }

        match process_one_with_fail_track(state, &row).await {
            Ok(()) => processed += 1,
            Err(e) => {
                tracing::warn!(
                    "Crawler image upload failed (id={} url={}): {e}",
                    row.id,
                    row.original_url
                );
            }
        }
    }

    if processed > 0 || skipped_backoff > 0 {
        tracing::info!(
            "Crawler image uploader tick: processed={processed} backoff_skipped={skipped_backoff}"
        );
    }
    Ok(())
}

/// 待处理图片最小投影
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
struct PendingImage {
    id: i64,
    article_id: i64,
    original_url: String,
    /// 可选：任务级代理（通过 join crawler_articles + crawler_tasks 取得）
    task_proxy: Option<String>,
    task_user_agent: Option<String>,
    retry_count: i64,
    updated_at: chrono::NaiveDateTime,
}

async fn fetch_pending_images(
    db: &DbPool,
    limit: i64,
) -> Result<Vec<PendingImage>, String> {
    match db {
        DbPool::Sqlite(pool) => {
            let rows = sqlx::query_as::<_, PendingImage>(
                "SELECT i.id AS id, i.article_id AS article_id, i.original_url AS original_url, \
                 t.proxy AS task_proxy, t.user_agent AS task_user_agent, \
                 i.retry_count AS retry_count, i.updated_at AS updated_at \
                 FROM crawler_article_images i \
                 JOIN crawler_articles a ON a.id = i.article_id \
                 JOIN crawler_tasks t ON t.id = a.task_id \
                 WHERE i.status IN ('pending','failed') AND i.retry_count < ? \
                 ORDER BY i.id ASC LIMIT ?",
            )
            .bind(MAX_RETRIES)
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(rows)
        }
        DbPool::Postgres(pool) => {
            let rows = sqlx::query_as::<_, PendingImage>(
                "SELECT i.id AS id, i.article_id AS article_id, i.original_url AS original_url, \
                 t.proxy AS task_proxy, t.user_agent AS task_user_agent, \
                 i.retry_count AS retry_count, i.updated_at AS updated_at \
                 FROM crawler_article_images i \
                 JOIN crawler_articles a ON a.id = i.article_id \
                 JOIN crawler_tasks t ON t.id = a.task_id \
                 WHERE i.status IN ('pending','failed') AND i.retry_count < $1 \
                 ORDER BY i.id ASC LIMIT $2",
            )
            .bind(MAX_RETRIES)
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(rows)
        }
    }
}

/// 处理单张图片：下载 → 上传图床 → 更新状态
async fn process_one(state: &AppState, row: &PendingImage) -> Result<(), String> {
    // 1. 下载图片字节
    let sys_proxy = state.http_proxy_url().await;
    let proxy = row.task_proxy.clone().or(sys_proxy);
    let ua = row
        .task_user_agent
        .clone()
        .unwrap_or_else(|| crate::services::crawler::engine::DEFAULT_USER_AGENT.to_string());

    let (bytes, ext) = download_image(&row.original_url, Some(&ua), proxy.as_deref()).await?;

    // 2. 落盘缓存：image_cache_dir/crawler/<sha256>.<ext>
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha = hasher.finalize();
    let sha_hex: String = sha.iter().map(|b| format!("{:02x}", b)).collect();
    let cache_dir = state.image_cache_dir.join("crawler");
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .map_err(|e| format!("create cache dir: {e}"))?;
    let filename = format!("{sha_hex}.{ext}");
    let local_path = cache_dir.join(&filename);
    tokio::fs::write(&local_path, &bytes)
        .await
        .map_err(|e| format!("write cache: {e}"))?;

    // 3. 取图床群组A chat_id
    let group_a_id = {
        let cache = state.option_cache.read().await;
        cache.get("ImageGroupChatId").cloned()
    };
    let group_a_id = group_a_id
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "ImageGroupChatId 未配置".to_string())?;
    let target_id: i64 = group_a_id
        .parse()
        .map_err(|e| format!("无效 ImageGroupChatId: {e}"))?;

    // 4. 取一个 active grammers_client
    let tg_clients = state.tg_clients.read().await;
    let client = tg_clients
        .values()
        .find(|e| e.status == "active" && e.client.is_some())
        .and_then(|e| e.client.clone())
        .ok_or_else(|| "无可用 Telegram 客户端".to_string())?;
    drop(tg_clients);

    // 5. 解析群组A peer
    let target_chat = crate::services::tg_api::resolve_peer(target_id, &state.tg_clients, &state.peer_cache)
        .await
        .map_err(|e| format!("解析群组A失败: {e}"))?;

    // 6. upload_file + send_message(photo)
    let uploaded = client
        .upload_file(&local_path)
        .await
        .map_err(|e| format!("upload_file: {e}"))?;
    use grammers_client::InputMessage;
    let sent = client
        .send_message(target_chat, InputMessage::text("").photo(uploaded))
        .await
        .map_err(|e| format!("send_message: {e}"))?;
    let new_msg_id = sent.id() as i64;

    // 7. 写回：status='uploaded' + image_message_id
    update_uploaded(&state.db, row.id, new_msg_id, &local_path.to_string_lossy()).await?;

    tracing::debug!(
        "Crawler image uploaded: id={} msg_id={} path={}",
        row.id,
        new_msg_id,
        local_path.display()
    );
    Ok(())
}

/// 下载图片字节，返回 (bytes, ext)
async fn download_image(
    url: &str,
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<(Vec<u8>, String), String> {
    let client = crate::services::crawler::engine::build_reqwest_client_pub(user_agent, proxy)
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    // 扩展名优先来自 Content-Type
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let ext = mime_to_ext(&ctype)
        .or_else(|| ext_from_url(url))
        .unwrap_or_else(|| "jpg".to_string());
    let bytes = resp.bytes().await.map_err(|e| format!("read body: {e}"))?;
    Ok((bytes.to_vec(), ext))
}

fn mime_to_ext(ctype: &str) -> Option<String> {
    let primary = ctype.split(';').next().unwrap_or("").trim();
    match primary {
        "image/jpeg" | "image/jpg" => Some("jpg".into()),
        "image/png" => Some("png".into()),
        "image/gif" => Some("gif".into()),
        "image/webp" => Some("webp".into()),
        "image/bmp" => Some("bmp".into()),
        "image/svg+xml" => Some("svg".into()),
        _ => None,
    }
}

fn ext_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let last = path.rsplit('/').next()?;
    let dot = last.rfind('.')?;
    let ext = last[dot + 1..].to_lowercase();
    if ext.chars().all(|c| c.is_ascii_alphanumeric()) && (1..=5).contains(&ext.len()) {
        Some(ext)
    } else {
        None
    }
}

async fn update_uploaded(
    db: &DbPool,
    image_id: i64,
    message_id: i64,
    local_path: &str,
) -> Result<(), String> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE crawler_article_images \
                 SET status='uploaded', image_message_id=?, local_path=?, \
                 last_error=NULL, updated_at=CURRENT_TIMESTAMP \
                 WHERE id=?",
            )
            .bind(message_id)
            .bind(local_path)
            .bind(image_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE crawler_article_images \
                 SET status='uploaded', image_message_id=$1, local_path=$2, \
                 last_error=NULL, updated_at=CURRENT_TIMESTAMP \
                 WHERE id=$3",
            )
            .bind(message_id)
            .bind(local_path)
            .bind(image_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 标记失败：retry_count++ + last_error + status='failed'
async fn mark_failed(db: &DbPool, image_id: i64, err: &str) -> Result<(), String> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE crawler_article_images \
                 SET status='failed', retry_count=retry_count+1, last_error=?, \
                 updated_at=CURRENT_TIMESTAMP WHERE id=?",
            )
            .bind(err)
            .bind(image_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE crawler_article_images \
                 SET status='failed', retry_count=retry_count+1, last_error=$1, \
                 updated_at=CURRENT_TIMESTAMP WHERE id=$2",
            )
            .bind(err)
            .bind(image_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// 包装 process_one：失败时把失败写回 DB（独立 await 点，避免借用冲突）
async fn process_one_with_fail_track(state: &AppState, row: &PendingImage) -> Result<(), String> {
    let result = process_one(state, row).await;
    if let Err(e) = &result {
        // 截断错误信息，避免 last_error 列过长 —— 按字符切，防多字节 UTF-8 panic
        let clipped = if e.chars().count() > 500 {
            let truncated: String = e.chars().take(500).collect();
            truncated
        } else {
            e.clone()
        };
        let _ = mark_failed(&state.db, row.id, &clipped).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_to_ext_jpeg() {
        assert_eq!(mime_to_ext("image/jpeg").unwrap(), "jpg");
        assert_eq!(mime_to_ext("image/png").unwrap(), "png");
        assert_eq!(mime_to_ext("image/gif").unwrap(), "gif");
        assert_eq!(mime_to_ext("image/webp").unwrap(), "webp");
        assert_eq!(mime_to_ext("image/svg+xml").unwrap(), "svg");
    }

    #[test]
    fn mime_to_ext_with_parameters() {
        assert_eq!(mime_to_ext("image/jpeg; charset=utf-8").unwrap(), "jpg");
        assert_eq!(mime_to_ext("image/png; boundary=something").unwrap(), "png");
    }

    #[test]
    fn mime_to_ext_unknown() {
        assert!(mime_to_ext("application/octet-stream").is_none());
        assert!(mime_to_ext("").is_none());
    }

    #[test]
    fn ext_from_url_simple() {
        assert_eq!(ext_from_url("https://example.com/a.jpg").unwrap(), "jpg");
        assert_eq!(ext_from_url("https://example.com/path/x.png").unwrap(), "png");
    }

    #[test]
    fn ext_from_url_with_query() {
        assert_eq!(ext_from_url("https://example.com/a.jpeg?token=abc").unwrap(), "jpeg");
    }

    #[test]
    fn ext_from_url_no_ext() {
        assert!(ext_from_url("https://example.com/noext").is_none());
    }

    #[test]
    fn ext_from_url_invalid() {
        // 过长
        assert!(ext_from_url("https://example.com/a.jpeeeeeg").is_none());
        // 含非字母数字
        assert!(ext_from_url("https://example.com/a.j-pg").is_none());
    }

    #[test]
    fn backoff_table_sensible() {
        assert_eq!(BACKOFF_SECS[0], 10);
        assert_eq!(BACKOFF_SECS[1], 30);
        assert_eq!(BACKOFF_SECS[2], 120);
    }

    #[test]
    fn max_retries_is_three() {
        assert_eq!(MAX_RETRIES, 3);
    }
}
