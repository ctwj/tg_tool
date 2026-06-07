// 图片转发队列 — 入队、去重、后台处理
// 资源提取时将图片入队，后台按间隔通过 Bot API sendPhoto 发送到图床群组

use crate::errors::AppError;
use crate::models::forward_task::ForwardTask;
use crate::state::{AppState, DbPool};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// 转发调度器状态
#[derive(Debug)]
pub struct ForwardSchedulerState {
    pub running: bool,
    pub interval_secs: u64,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
}

pub type ForwardSchedulerHandle = Arc<RwLock<ForwardSchedulerState>>;

pub fn create_forward_scheduler() -> ForwardSchedulerHandle {
    Arc::new(RwLock::new(ForwardSchedulerState {
        running: false,
        interval_secs: 2,
        handle: None,
        cancel: None,
    }))
}

/// 启动转发调度器
pub async fn start_forward_scheduler(
    scheduler: ForwardSchedulerHandle,
    interval_secs: u64,
    state: AppState,
) {
    let mut sched = scheduler.write().await;
    if sched.running {
        return;
    }

    let cancel = CancellationToken::new();
    sched.cancel = Some(cancel.clone());
    sched.running = true;
    sched.interval_secs = interval_secs;

    let sched_clone = scheduler.clone();
    let handle = tokio::spawn(async move {
        let duration = std::time::Duration::from_secs(interval_secs);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(duration) => {
                    if let Err(e) = process_next_task(&state).await {
                        tracing::warn!("转发任务处理失败: {e}");
                    }
                }
                _ = cancel.cancelled() => {
                    tracing::info!("转发调度器已停止");
                    break;
                }
            }
        }
        let mut s = sched_clone.write().await;
        s.running = false;
        s.handle = None;
        s.cancel = None;
    });

    sched.handle = Some(handle);
    tracing::info!("转发调度器已启动，间隔 {interval_secs} 秒");
}

/// 停止转发调度器
pub async fn stop_forward_scheduler(scheduler: ForwardSchedulerHandle) {
    let mut state = scheduler.write().await;
    if let Some(cancel) = state.cancel.take() {
        cancel.cancel();
    }
    if let Some(handle) = state.handle.take() {
        handle.abort();
    }
    state.running = false;
}

/// 入队转发任务（三层去重）
pub async fn enqueue(
    state: &AppState,
    remote_id: &str,
    channel_id: Option<i64>,
    message_id: Option<i64>,
    title: Option<&str>,
    description: Option<&str>,
    link: Option<&str>,
) -> Result<(), AppError> {
    if remote_id.is_empty() {
        return Ok(());
    }

    // 去重层 1: image_mappings 已有映射
    let already_mapped = match &state.db {
        DbPool::Sqlite(pool) => {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT file_id FROM image_mappings WHERE remote_id = ?")
                    .bind(remote_id)
                    .fetch_optional(pool)
                    .await?;
            row.is_some()
        }
        DbPool::Postgres(pool) => {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT file_id FROM image_mappings WHERE remote_id = $1")
                    .bind(remote_id)
                    .fetch_optional(pool)
                    .await?;
            row.is_some()
        }
    };
    if already_mapped {
        tracing::debug!("图片 {remote_id} 已有映射，跳过入队");
        return Ok(());
    }

    // 去重层 2: forward_tasks 已有 pending/forwarded 任务
    let already_queued = match &state.db {
        DbPool::Sqlite(pool) => {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM forward_tasks WHERE remote_id = ? AND status IN ('pending', 'forwarded')",
            )
            .bind(remote_id)
            .fetch_optional(pool)
            .await?;
            row.is_some()
        }
        DbPool::Postgres(pool) => {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM forward_tasks WHERE remote_id = $1 AND status IN ('pending', 'forwarded')",
            )
            .bind(remote_id)
            .fetch_optional(pool)
            .await?;
            row.is_some()
        }
    };
    if already_queued {
        tracing::debug!("图片 {remote_id} 已在队列中，跳过入队");
        return Ok(());
    }

    // 去重层 3: INSERT 时 remote_id UNIQUE 约束（数据库层面）
    match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT OR IGNORE INTO forward_tasks (remote_id, channel_id, message_id, title, description, link) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(remote_id)
            .bind(channel_id)
            .bind(message_id)
            .bind(title)
            .bind(description)
            .bind(link)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO forward_tasks (remote_id, channel_id, message_id, title, description, link) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            )
            .bind(remote_id)
            .bind(channel_id)
            .bind(message_id)
            .bind(title)
            .bind(description)
            .bind(link)
            .execute(pool)
            .await?;
        }
    }

    tracing::info!("图片转发任务已入队: {remote_id}");
    Ok(())
}

/// 处理下一个 pending 任务
async fn process_next_task(state: &AppState) -> Result<(), AppError> {
    // 读取配置
    let (bot_token, chat_id, proxy_url) = {
        let cache = state.option_cache.read().await;
        let bot_id = cache.get("ImageBotId").cloned().unwrap_or_default();
        let chat_id_val = cache.get("ImageGroupChatId").cloned().unwrap_or_default();

        if bot_id.is_empty() || chat_id_val.is_empty() {
            return Ok(()); // 未配置，静默跳过
        }

        // 获取 Bot Token
        let token = get_bot_token(state, &bot_id).await?;
        let proxy = cache
            .get("http_proxy_url")
            .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
            .or_else(|| {
                cache
                    .get("proxy_url")
                    .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
            });

        (token, chat_id_val, proxy)
    };

    // 取一个 pending 任务
    let task = fetch_pending_task(&state.db).await?;
    let task = match task {
        Some(t) => t,
        None => return Ok(()), // 队列空
    };

    tracing::info!("处理转发任务: id={}, remote_id={}", task.id, task.remote_id);

    // 下载图片：通过 MTProto 客户端从原始频道下载
    let photo_bytes = download_photo_from_channel(state, &task).await;

    let photo_bytes = match photo_bytes {
        Ok(data) => data,
        Err(e) => {
            // 下载失败，标记为 failed
            mark_task_failed(&state.db, task.id, &e.to_string()).await?;
            tracing::warn!("转发任务 {} 下载图片失败: {e}", task.id);
            return Ok(());
        }
    };

    // 构建 caption
    let caption = build_caption(
        task.title.as_ref().map(|s| s.as_str()),
        task.description.as_ref().map(|s| s.as_str()),
        task.link.as_ref().map(|s| s.as_str()),
    );

    // 发送图片到图床群组
    let send_result = crate::services::bot_api::send_photo(
        &bot_token,
        &chat_id,
        photo_bytes,
        Some(&caption),
        proxy_url.as_ref().map(|s| s.as_str()).as_deref(),
    )
    .await;

    match send_result {
        Ok(file_id) => {
            // 写入 image_mappings
            save_mapping(&state.db, &task.remote_id, &file_id).await?;
            // 更新任务状态为 forwarded
            mark_task_forwarded(&state.db, task.id, &file_id).await?;
            tracing::info!("转发成功: remote_id={}, file_id={}", task.remote_id, file_id);
        }
        Err(e) => {
            let err_str = e.to_string();
            let is_flood = err_str.contains("FLOOD_WAIT");
            mark_task_failed(&state.db, task.id, &err_str).await?;

            if is_flood {
                // FLOOD_WAIT: 解析等待秒数
                let wait_secs: u64 = err_str
                    .split("等待")
                    .nth(1)
                    .and_then(|s| s.split(' ').next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(5);
                tracing::warn!("FLOOD_WAIT: 等待 {wait_secs} 秒");
                tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
            }
        }
    }

    Ok(())
}

/// 从客户端获取 Bot Token
async fn get_bot_token(state: &AppState, bot_id: &str) -> Result<String, AppError> {
    let token = match &state.db {
        DbPool::Sqlite(pool) => {
            let row: Option<(Option<String>,)> =
                sqlx::query_as("SELECT token FROM clients WHERE id = ?")
                    .bind(bot_id)
                    .fetch_optional(pool)
                    .await?;
            row.and_then(|r| r.0)
        }
        DbPool::Postgres(pool) => {
            let row: Option<(Option<String>,)> =
                sqlx::query_as("SELECT token FROM clients WHERE id = $1")
                    .bind(bot_id)
                    .fetch_optional(pool)
                    .await?;
            row.and_then(|r| r.0)
        }
    };

    token.ok_or_else(|| AppError::NotFound(format!("Bot 客户端不存在: {bot_id}")))
}

/// 取一个 pending 任务
async fn fetch_pending_task(db: &DbPool) -> Result<Option<ForwardTask>, AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            let row = sqlx::query_as::<_, ForwardTask>(
                "SELECT * FROM forward_tasks WHERE status = 'pending' ORDER BY id ASC LIMIT 1",
            )
            .fetch_optional(pool)
            .await?;
            Ok(row)
        }
        DbPool::Postgres(pool) => {
            let row = sqlx::query_as::<_, ForwardTask>(
                "SELECT * FROM forward_tasks WHERE status = 'pending' ORDER BY id ASC LIMIT 1",
            )
            .fetch_optional(pool)
            .await?;
            Ok(row)
        }
    }
}

/// 通过 MTProto 客户端从原始频道下载图片
async fn download_photo_from_channel(
    state: &AppState,
    task: &ForwardTask,
) -> Result<Vec<u8>, AppError> {
    let channel_id = task
        .channel_id
        .ok_or_else(|| AppError::Internal("任务缺少 channel_id".to_string()))?;
    let message_id = task
        .message_id
        .ok_or_else(|| AppError::Internal("任务缺少 message_id".to_string()))?
        as i32;

    // 解析频道 PackedChat
    let packed_chat = crate::services::tg_api::resolve_peer(
        channel_id,
        &state.tg_clients,
        &state.peer_cache,
    )
    .await
    .map_err(|e| AppError::Internal(format!("解析频道失败: {e}")))?;

    // 获取 MTProto 客户端
    let tg_clients = state.tg_clients.read().await;
    let client = tg_clients
        .values()
        .find(|e| e.status == "active" && e.client.is_some())
        .and_then(|e| e.client.clone())
        .ok_or_else(|| AppError::Internal("无可用 Telegram 客户端".to_string()))?;
    drop(tg_clients);

    // 获取消息
    let messages = client
        .get_messages_by_id(packed_chat, &[message_id])
        .await
        .map_err(|e| AppError::Internal(format!("获取消息失败: {e}")))?;

    // 找到目标消息并提取 Media
    let mut media = None;
    for msg in messages.into_iter().flatten() {
        if msg.id() == message_id {
            media = msg.media();
            break;
        }
    }
    let media = media.ok_or_else(|| AppError::NotFound("消息无图片媒体".to_string()))?;

    // 下载（与 image_proxy.rs 相同模式）
    let downloadable = grammers_client::types::Downloadable::Media(media);
    let mut data = Vec::new();
    let mut download = client.iter_download(&downloadable);
    while let Some(chunk) = download
        .next()
        .await
        .map_err(|e| AppError::Internal(format!("下载图片失败: {e}")))?
    {
        data.extend_from_slice(&chunk);
    }

    if data.is_empty() {
        return Err(AppError::NotFound("下载的图片数据为空".to_string()));
    }

    Ok(data)
}

/// 构建 caption（标题 + 描述 + 链接，上限 1024 字符）
/// 多个 URL 用逗号分隔存储，展示时每个 URL 单独一行
fn build_caption(title: Option<&str>, description: Option<&str>, link: Option<&str>) -> String {
    let mut parts = Vec::new();

    if let Some(t) = title {
        if !t.is_empty() {
            parts.push(format!("📌 {t}"));
        }
    }
    if let Some(d) = description {
        if !d.is_empty() {
            parts.push(format!("📝 {d}"));
        }
    }
    if let Some(l) = link {
        if !l.is_empty() {
            // 多个 URL 逗号分隔，每个单独一行显示
            for url in l.split(',') {
                let url = url.trim();
                if !url.is_empty() {
                    parts.push(format!("🔗 {url}"));
                }
            }
        }
    }

    let caption = parts.join("\n");
    // 按 Unicode 字符截断到 1024（Telegram caption 上限）
    let truncated: String = caption.chars().take(1024).collect();
    truncated
}

/// 保存 remote_id -> file_id 映射
async fn save_mapping(db: &DbPool, remote_id: &str, file_id: &str) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT OR IGNORE INTO image_mappings (remote_id, file_id) VALUES (?, ?)",
            )
            .bind(remote_id)
            .bind(file_id)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO image_mappings (remote_id, file_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(remote_id)
            .bind(file_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 标记任务为 forwarded
async fn mark_task_forwarded(
    db: &DbPool,
    task_id: i64,
    file_id: &str,
) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET status = 'forwarded', file_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(file_id)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET status = 'forwarded', file_id = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            )
            .bind(file_id)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 标记任务为 failed
async fn mark_task_failed(db: &DbPool, task_id: i64, error: &str) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET status = 'failed', error = ?, retry_count = retry_count + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(error)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET status = 'failed', error = $1, retry_count = retry_count + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            )
            .bind(error)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

// ─── 队列管理 API 辅助函数 ─────────────────────────────────────────────────

/// 获取队列统计
pub async fn queue_status(db: &DbPool) -> Result<serde_json::Value, AppError> {
    let (pending, forwarded, failed) = match db {
        DbPool::Sqlite(pool) => {
            let pending: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM forward_tasks WHERE status = 'pending'",
            )
            .fetch_one(pool)
            .await?;
            let forwarded: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM forward_tasks WHERE status = 'forwarded'",
            )
            .fetch_one(pool)
            .await?;
            let failed: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM forward_tasks WHERE status = 'failed'",
            )
            .fetch_one(pool)
            .await?;
            (pending.0, forwarded.0, failed.0)
        }
        DbPool::Postgres(pool) => {
            let pending: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM forward_tasks WHERE status = 'pending'",
            )
            .fetch_one(pool)
            .await?;
            let forwarded: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM forward_tasks WHERE status = 'forwarded'",
            )
            .fetch_one(pool)
            .await?;
            let failed: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM forward_tasks WHERE status = 'failed'",
            )
            .fetch_one(pool)
            .await?;
            (pending.0, forwarded.0, failed.0)
        }
    };

    // 获取最近的 pending 任务列表
    let tasks = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, ForwardTask>(
                "SELECT * FROM forward_tasks WHERE status = 'pending' ORDER BY id ASC LIMIT 20",
            )
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, ForwardTask>(
                "SELECT * FROM forward_tasks WHERE status = 'pending' ORDER BY id ASC LIMIT 20",
            )
            .fetch_all(pool)
            .await?
        }
    };

    Ok(serde_json::json!({
        "pending": pending,
        "forwarded": forwarded,
        "failed": failed,
        "tasks": tasks,
    }))
}

/// 重试单个失败任务
pub async fn retry_task(db: &DbPool, task_id: i64) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            let result = sqlx::query(
                "UPDATE forward_tasks SET status = 'pending', error = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'failed'",
            )
            .bind(task_id)
            .execute(pool)
            .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("失败任务不存在".to_string()));
            }
        }
        DbPool::Postgres(pool) => {
            let result = sqlx::query(
                "UPDATE forward_tasks SET status = 'pending', error = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND status = 'failed'",
            )
            .bind(task_id)
            .execute(pool)
            .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("失败任务不存在".to_string()));
            }
        }
    }
    Ok(())
}

/// 重试所有失败任务
pub async fn retry_all_failed(db: &DbPool) -> Result<i64, AppError> {
    let count = match db {
        DbPool::Sqlite(pool) => {
            let result = sqlx::query(
                "UPDATE forward_tasks SET status = 'pending', error = NULL, updated_at = CURRENT_TIMESTAMP WHERE status = 'failed'",
            )
            .execute(pool)
            .await?;
            result.rows_affected() as i64
        }
        DbPool::Postgres(pool) => {
            let result = sqlx::query(
                "UPDATE forward_tasks SET status = 'pending', error = NULL, updated_at = CURRENT_TIMESTAMP WHERE status = 'failed'",
            )
            .execute(pool)
            .await?;
            result.rows_affected() as i64
        }
    };
    Ok(count)
}
