// 图片转发队列 — 入队、去重、后台处理（双群组两阶段架构）
//
// 阶段1（pending → awaiting_bot）：
//   MTProto 客户端用 copy_media 不下载地把图片转发到群组A，记录群组A 消息 ID
// 阶段2（awaiting_bot → forwarded）：
//   Bot 用 forwardMessage 把消息从群组A 转发到群组B，同步提取 file_id，写入 image_mappings
//
// 失败重试（FR-052）：根据 image_message_id 是否为空智能恢复
//   - 空（阶段1 失败）→ 恢复为 pending
//   - 非空（阶段2 失败）→ 恢复为 awaiting_bot，避免群组A 重复图片

use crate::errors::AppError;
use crate::models::forward_task::{ForwardTask, ForwardTaskWithCollector};
use crate::state::{AppState, DbPool};
use grammers_client::types::input_media::InputMedia;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// 转发调度器状态
#[derive(Debug)]
pub struct ForwardSchedulerState {
    pub running: bool,
    pub interval_secs: u64,
    /// 阶段1 worker（单 worker 模式 = 唯一 worker；双 worker 模式 = 阶段1 专用）
    pub handle: Option<tokio::task::JoinHandle<()>>,
    /// 阶段2 worker（仅当群组A ≠ 群组B 时存在，独立 2s 节流处理 awaiting_bot）
    pub handle_stage2: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
}

pub type ForwardSchedulerHandle = Arc<RwLock<ForwardSchedulerState>>;

pub fn create_forward_scheduler() -> ForwardSchedulerHandle {
    Arc::new(RwLock::new(ForwardSchedulerState {
        running: false,
        interval_secs: 2,
        handle: None,
        handle_stage2: None,
        cancel: None,
    }))
}

/// 启动转发调度器
///
/// 根据 `ImageGroupChatId` 与 `ImageGroupChatId2` 的关系自动选择调度模式：
/// - **单 worker 模式**（B 未配置 或 A == B）：一个 worker 串行处理 pending → awaiting_bot
///   - 适用于阶段1/阶段2 写同一群组（共享 FLOOD_WAIT 频率）
/// - **双 worker 模式**（A != B）：阶段1/阶段2 各自独立 worker，分别 2s 节流
///   - 适用于阶段1 写群组A、阶段2 写群组B，两条流水线互不冲突，吞吐量翻倍
///
/// 启动后配置变化需重启服务才生效。
pub async fn start_forward_scheduler(
    scheduler: ForwardSchedulerHandle,
    interval_secs: u64,
    state: AppState,
) {
    let mut sched = scheduler.write().await;
    if sched.running {
        return;
    }

    // 判定调度模式
    let use_dual_worker = {
        let cache = state.option_cache.read().await;
        let chat_id_a = cache.get("ImageGroupChatId").cloned().unwrap_or_default();
        let chat_id_b = cache.get("ImageGroupChatId2").cloned().unwrap_or_default();
        !chat_id_b.is_empty() && chat_id_a != chat_id_b
    };

    let cancel = CancellationToken::new();
    sched.cancel = Some(cancel.clone());
    sched.running = true;
    sched.interval_secs = interval_secs;

    let duration = std::time::Duration::from_secs(interval_secs);

    // 阶段1 worker（始终启动）
    // 单 worker 模式：调用 process_next_task 串行处理两阶段
    // 双 worker 模式：调用 process_stage1_task 专门处理 pending
    let state_a = state.clone();
    let cancel_a = cancel.clone();
    let sched_clone_a = scheduler.clone();
    let handle_a = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(duration) => {
                    let task_result = if use_dual_worker {
                        process_stage1_task(&state_a).await
                    } else {
                        process_next_task(&state_a).await
                    };
                    if let Err(e) = task_result {
                        tracing::warn!("转发任务处理失败: {e}");
                    }
                }
                _ = cancel_a.cancelled() => {
                    tracing::info!("转发调度器 worker-A 已停止");
                    break;
                }
            }
        }
        let mut s = sched_clone_a.write().await;
        s.running = false;
        s.handle = None;
        s.handle_stage2 = None;
        s.cancel = None;
    });
    sched.handle = Some(handle_a);

    // 阶段2 worker（仅双 worker 模式启动）
    if use_dual_worker {
        let state_b = state.clone();
        let cancel_b = cancel.clone();
        let sched_clone_b = scheduler.clone();
        let handle_b = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(duration) => {
                        if let Err(e) = process_stage2_task(&state_b).await {
                            tracing::warn!("阶段2 任务处理失败: {e}");
                        }
                    }
                    _ = cancel_b.cancelled() => {
                        tracing::info!("转发调度器 worker-B 已停止");
                        break;
                    }
                }
            }
            // 注：状态清理由 worker-A 统一处理（同一调度器，最后退出者执行）
            let mut s = sched_clone_b.write().await;
            if s.handle.is_none() {
                // worker-A 已退出并清理，这里只兜底
                s.running = false;
                s.handle_stage2 = None;
                s.cancel = None;
            }
        });
        sched.handle_stage2 = Some(handle_b);
        tracing::info!(
            "转发调度器已启动（双 worker 模式：群组A ≠ 群组B），间隔 {interval_secs} 秒"
        );
    } else {
        tracing::info!(
            "转发调度器已启动（单 worker 模式：群组A == 群组B 或群组B 未配置），间隔 {interval_secs} 秒"
        );
    }
}

/// 停止转发调度器（同时关闭阶段1 和阶段2 worker）
pub async fn stop_forward_scheduler(scheduler: ForwardSchedulerHandle) {
    let mut state = scheduler.write().await;
    if let Some(cancel) = state.cancel.take() {
        cancel.cancel();
    }
    if let Some(handle) = state.handle.take() {
        handle.abort();
    }
    if let Some(handle) = state.handle_stage2.take() {
        handle.abort();
    }
    state.running = false;
}

/// 入队转发任务（三层去重 + 总开关）
///
/// 总开关：`image_storage_enabled` 配置为 `"false"`（精确匹配）时直接返回，不入队。
/// 缺失或其他值（包括 `"true"`）按现有逻辑入队。
/// 关闭开关时已在队列中的存量任务继续被调度器处理（FR-003）。
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

    // ── 总开关检查（FR-001 / FR-002） ──────────────────────────────────
    // OptionCache 缺失该键视为开启（保持现有部署行为兼容）
    let enabled = {
        let cache = state.option_cache.read().await;
        cache.get("image_storage_enabled").map(|v| v.as_str()) != Some("false")
    };
    if !enabled {
        tracing::debug!("图片转存功能已关闭，跳过入队: {remote_id}");
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

    // 去重层 2: forward_tasks 已有 pending/awaiting_bot/forwarded 任务
    // awaiting_bot 也加入白名单，避免两阶段之间被重复入队（data-model 不变量 4）
    let already_queued = match &state.db {
        DbPool::Sqlite(pool) => {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM forward_tasks WHERE remote_id = ? AND status IN ('pending', 'awaiting_bot', 'forwarded')",
            )
            .bind(remote_id)
            .fetch_optional(pool)
            .await?;
            row.is_some()
        }
        DbPool::Postgres(pool) => {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM forward_tasks WHERE remote_id = $1 AND status IN ('pending', 'awaiting_bot', 'forwarded')",
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

/// 读取阶段1+阶段2 全套配置（多入口共用）
///
/// 返回 `(bot_token, chat_id_a, chat_id_b, delete_temp, proxy_url)`
/// 任一必填项缺失时返回 None，调用方应静默跳过本次 tick。
async fn read_pipeline_config(
    state: &AppState,
) -> Option<(String, String, String, bool, Option<String>)> {
    let cache = state.option_cache.read().await;
    let bot_id = cache.get("ImageBotId").cloned().unwrap_or_default();
    let chat_id_val = cache.get("ImageGroupChatId").cloned().unwrap_or_default();

    if bot_id.is_empty() || chat_id_val.is_empty() {
        return None; // 群组A/Bot 未配置
    }
    let chat_id_a = chat_id_val;

    let proxy = cache
        .get("http_proxy_url")
        .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
        .or_else(|| {
            cache
                .get("proxy_url")
                .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
        });

    let chat_id_b_val = cache.get("ImageGroupChatId2").cloned().unwrap_or_default();
    let delete_temp = cache.get("delete_bot_forward_message").map(|v| v.as_str()) == Some("true");
    drop(cache);

    let token = get_bot_token(state, &bot_id).await.ok()?;
    Some((token, chat_id_a, chat_id_b_val, delete_temp, proxy))
}

/// 执行单个任务的阶段1：客户端 copy_media 转存到群组A
async fn execute_stage1(state: &AppState, task: &ForwardTask, chat_id_a: &str) {
    match run_stage_1_client_forward(state, task, chat_id_a).await {
        Ok(image_message_id) => {
            if let Err(e) = mark_task_awaiting_bot(&state.db, task.id, image_message_id).await {
                tracing::warn!("任务 {} 标记 awaiting_bot 失败: {e}", task.id);
            }
            tracing::info!(
                "阶段1 成功: task={}, image_message_id={}",
                task.id,
                image_message_id
            );
        }
        Err(e) => {
            // 文案修正：阶段1 失败不一定发生在 copy_media/send_album 调用环节
            // （可能是更早的"源消息无图片媒体"），统一描述为"阶段1 失败"
            let err_str = format!("阶段1 失败: {e}");
            if let Err(db_err) = mark_task_failed(&state.db, task.id, &err_str).await {
                tracing::warn!("任务 {} 标记 failed 失败: {db_err}", task.id);
            }
            tracing::warn!("任务 {} {err_str}", task.id);
        }
    }
}

/// 执行单个任务的阶段2：Bot forwardMessage 转发到群组B 取 file_id
async fn execute_stage2(
    state: &AppState,
    task: &ForwardTask,
    bot_token: &str,
    chat_id_a: &str,
    chat_id_b: &str,
    delete_temp: bool,
    proxy_url: Option<&str>,
) {
    match run_stage_2_bot_fetch_file_id(
        task,
        bot_token,
        chat_id_a,
        chat_id_b,
        delete_temp,
        proxy_url,
    )
    .await
    {
        Ok(file_id) => {
            if let Err(e) = save_mapping(&state.db, &task.remote_id, &file_id).await {
                tracing::warn!("任务 {} 保存映射失败: {e}", task.id);
            }
            if let Err(e) = mark_task_forwarded(&state.db, task.id, &file_id).await {
                tracing::warn!("任务 {} 标记 forwarded 失败: {e}", task.id);
            }
            tracing::info!("阶段2 成功: task={}, file_id={}", task.id, file_id);
        }
        Err(e) => {
            let err_str = e.to_string();
            let is_flood = err_str.contains("FLOOD_WAIT");
            if let Err(db_err) = mark_task_failed(&state.db, task.id, &err_str).await {
                tracing::warn!("任务 {} 标记 failed 失败: {db_err}", task.id);
            }
            tracing::warn!("任务 {} 阶段2 失败: {err_str}", task.id);

            if is_flood {
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
}

/// 单 worker 模式入口：阶段1 优先，无则阶段2（保持旧行为，群组A==B 或 B 未配置时使用）
async fn process_next_task(state: &AppState) -> Result<(), AppError> {
    let (bot_token, chat_id_a, chat_id_b, delete_temp, proxy_url) =
        match read_pipeline_config(state).await {
            Some(cfg) => cfg,
            None => return Ok(()), // 配置不全，静默跳过
        };

    // 阶段1 优先：先取 pending 任务，无则取 awaiting_bot
    let task = match fetch_pending_task(&state.db).await? {
        Some(t) => t,
        None => match fetch_awaiting_bot_task(&state.db).await? {
            Some(t) => t,
            None => return Ok(()), // 两类队列都空
        },
    };

    tracing::info!(
        "处理转发任务: id={}, remote_id={}, status={}",
        task.id,
        task.remote_id,
        task.status
    );

    match task.status.as_str() {
        "pending" => execute_stage1(state, &task, &chat_id_a).await,
        "awaiting_bot" => {
            execute_stage2(
                state,
                &task,
                &bot_token,
                &chat_id_a,
                &chat_id_b,
                delete_temp,
                proxy_url.as_deref(),
            )
            .await
        }
        other => {
            tracing::warn!("任务 {} 处于非预期状态: {other}", task.id);
        }
    }

    Ok(())
}

/// 双 worker 模式入口：阶段1 worker 专用，只取 pending 任务
async fn process_stage1_task(state: &AppState) -> Result<(), AppError> {
    let (_bot_token, chat_id_a, _chat_id_b, _delete_temp, _proxy_url) =
        match read_pipeline_config(state).await {
            Some(cfg) => cfg,
            None => return Ok(()),
        };

    let task = match fetch_pending_task(&state.db).await? {
        Some(t) => t,
        None => return Ok(()), // pending 队列空，让出本 tick
    };

    tracing::info!(
        "[worker-A] 处理阶段1 任务: id={}, remote_id={}",
        task.id,
        task.remote_id
    );
    execute_stage1(state, &task, &chat_id_a).await;
    Ok(())
}

/// 双 worker 模式入口：阶段2 worker 专用，只取 awaiting_bot 任务
async fn process_stage2_task(state: &AppState) -> Result<(), AppError> {
    let (bot_token, chat_id_a, chat_id_b, delete_temp, proxy_url) =
        match read_pipeline_config(state).await {
            Some(cfg) => cfg,
            None => return Ok(()),
        };

    let task = match fetch_awaiting_bot_task(&state.db).await? {
        Some(t) => t,
        None => return Ok(()),
    };

    tracing::info!(
        "[worker-B] 处理阶段2 任务: id={}, remote_id={}",
        task.id,
        task.remote_id
    );
    execute_stage2(
        state,
        &task,
        &bot_token,
        &chat_id_a,
        &chat_id_b,
        delete_temp,
        proxy_url.as_deref(),
    )
    .await;
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

/// 取一个 pending 任务（阶段1）
async fn fetch_pending_task(db: &DbPool) -> Result<Option<ForwardTask>, AppError> {
    let sql = "SELECT * FROM forward_tasks WHERE status = 'pending' ORDER BY id ASC LIMIT 1";
    match db {
        DbPool::Sqlite(pool) => Ok(sqlx::query_as::<_, ForwardTask>(sql)
            .fetch_optional(pool)
            .await?),
        DbPool::Postgres(pool) => Ok(sqlx::query_as::<_, ForwardTask>(sql)
            .fetch_optional(pool)
            .await?),
    }
}

/// 取一个 awaiting_bot 任务（阶段2）
async fn fetch_awaiting_bot_task(db: &DbPool) -> Result<Option<ForwardTask>, AppError> {
    let sql = "SELECT * FROM forward_tasks WHERE status = 'awaiting_bot' ORDER BY id ASC LIMIT 1";
    match db {
        DbPool::Sqlite(pool) => Ok(sqlx::query_as::<_, ForwardTask>(sql)
            .fetch_optional(pool)
            .await?),
        DbPool::Postgres(pool) => Ok(sqlx::query_as::<_, ForwardTask>(sql)
            .fetch_optional(pool)
            .await?),
    }
}

/// 阶段1：客户端 copy_media 不下载地把图片转发到群组A，返回群组A 中的新消息 ID
///
/// 参考 forwarder.rs::forward_chat 的现成模式：
/// `InputMedia::caption(text).copy_media(&media)` + `client.send_album(target, vec![input_media])`
async fn run_stage_1_client_forward(
    state: &AppState,
    task: &ForwardTask,
    target_chat_id: &str,
) -> Result<i64, AppError> {
    let channel_id = task
        .channel_id
        .ok_or_else(|| AppError::Internal("任务缺少 channel_id".to_string()))?;
    let message_id =
        task.message_id
            .ok_or_else(|| AppError::Internal("任务缺少 message_id".to_string()))? as i32;

    let target_id: i64 = target_chat_id
        .parse()
        .map_err(|e| AppError::Internal(format!("无效的群组A chat_id: {e}")))?;

    // 解析源频道 peer
    let source_chat =
        crate::services::tg_api::resolve_peer(channel_id, &state.tg_clients, &state.peer_cache)
            .await
            .map_err(|e| AppError::Internal(format!("阶段1 解析频道失败: {e}")))?;

    // 取 active 客户端
    let tg_clients = state.tg_clients.read().await;
    let client = tg_clients
        .values()
        .find(|e| e.status == "active" && e.client.is_some())
        .and_then(|e| e.client.clone())
        .ok_or_else(|| AppError::Internal("阶段1 无可用 Telegram 客户端".to_string()))?;
    drop(tg_clients);

    // 取源消息
    let messages = client
        .get_messages_by_id(source_chat, &[message_id])
        .await
        .map_err(|e| AppError::Internal(format!("阶段1 获取源消息失败: {e}")))?;

    let mut media = None;
    for msg in messages.into_iter().flatten() {
        if msg.id() == message_id {
            media = msg.media();
            break;
        }
    }
    let media = media.ok_or_else(|| AppError::NotFound("阶段1 源消息无图片媒体".to_string()))?;

    // 解析目标群组A peer
    let target_chat =
        crate::services::tg_api::resolve_peer(target_id, &state.tg_clients, &state.peer_cache)
            .await
            .map_err(|e| AppError::Internal(format!("阶段1 解析群组A 失败: {e}")))?;

    // 构造 caption + copy_media
    let caption = build_caption(
        task.title.as_deref(),
        task.description.as_deref(),
        task.link.as_deref(),
    );
    let input_media = InputMedia::caption(caption).copy_media(&media);

    // 发送（不下载，仅引用原图 remote reference）
    let result = client
        .send_album(target_chat, vec![input_media])
        .await
        .map_err(|e| AppError::Internal(format!("阶段1 send_album 失败: {e}")))?;

    // send_album 返回 Vec<Option<Message>>，取第一个
    let new_msg_id = result
        .into_iter()
        .next()
        .flatten()
        .map(|m| m.id() as i64)
        .ok_or_else(|| AppError::Internal("阶段1 send_album 未返回新消息".to_string()))?;

    Ok(new_msg_id)
}

/// 阶段2：Bot 用 forwardMessage 把消息从群组A 转发到群组B，同步提取 file_id
///
/// 流程：
/// 1. 若群组B 未配置 → 返回错误（任务保持 awaiting_bot，不影响阶段1 已转存的图片）
/// 2. forward_message(token, 群组B, 群组A, image_message_id) → (fwd_msg_id, file_id_opt)
/// 3. 若 file_id_opt 为 None → delete 临时消息并返回错误（防止群组B 累积无 file_id 的垃圾消息）
/// 4. 若 delete_temp == true → delete 临时消息（失败仅 warn）
/// 5. 返回 file_id
async fn run_stage_2_bot_fetch_file_id(
    task: &ForwardTask,
    bot_token: &str,
    image_group_chat_id: &str,   // 群组A（from）
    image_group_chat_id_2: &str, // 群组B（to）
    delete_temp: bool,
    proxy_url: Option<&str>,
) -> Result<String, AppError> {
    if image_group_chat_id_2.is_empty() {
        return Err(AppError::Internal(
            "阶段2 失败: 群组B 未配置 (ImageGroupChatId2 为空)".to_string(),
        ));
    }

    let image_message_id = task
        .image_message_id
        .ok_or_else(|| AppError::Internal("阶段2 失败: 任务缺少 image_message_id".to_string()))?;

    let (fwd_msg_id, file_id_opt) = crate::services::bot_api::forward_message(
        bot_token,
        image_group_chat_id_2,
        image_group_chat_id,
        image_message_id,
        proxy_url,
    )
    .await?;

    let file_id = match file_id_opt {
        Some(fid) => fid,
        None => {
            // forwardMessage 成功但无 photo — 清理临时消息后返回错误
            if let Err(e) = crate::services::bot_api::delete_message(
                bot_token,
                image_group_chat_id_2,
                fwd_msg_id,
                proxy_url,
            )
            .await
            {
                tracing::warn!(
                    "任务 {} 清理无 photo 的临时消息失败 (fwd_msg_id={}): {e}",
                    task.id,
                    fwd_msg_id
                );
            }
            return Err(AppError::Internal(
                "阶段2 失败: forwardMessage 返回无 photo".to_string(),
            ));
        }
    };

    if delete_temp
        && let Err(e) = crate::services::bot_api::delete_message(
            bot_token,
            image_group_chat_id_2,
            fwd_msg_id,
            proxy_url,
        )
        .await
    {
        // 删除失败仅 warn，不影响任务成功（file_id 已就绪）
        tracing::warn!(
            "任务 {} 阶段2 deleteMessage 失败 (fwd_msg_id={}): {e}",
            task.id,
            fwd_msg_id
        );
    }

    Ok(file_id)
}

/// 标记任务为 awaiting_bot（阶段1 成功，等待阶段2）
async fn mark_task_awaiting_bot(
    db: &DbPool,
    task_id: i64,
    image_message_id: i64,
) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET status = 'awaiting_bot', image_message_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(image_message_id)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET status = 'awaiting_bot', image_message_id = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            )
            .bind(image_message_id)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 构建 caption（标题 + 描述 + 链接，上限 1024 字符）
/// 多个 URL 用逗号分隔存储，展示时每个 URL 单独一行
fn build_caption(title: Option<&str>, description: Option<&str>, link: Option<&str>) -> String {
    let mut parts = Vec::new();

    if let Some(t) = title
        && !t.is_empty()
    {
        parts.push(format!("📌 {t}"));
    }
    if let Some(d) = description
        && !d.is_empty()
    {
        parts.push(format!("📝 {d}"));
    }
    if let Some(l) = link
        && !l.is_empty()
    {
        // 多个 URL 逗号分隔，每个单独一行显示
        for url in l.split(',') {
            let url = url.trim();
            if !url.is_empty() {
                parts.push(format!("🔗 {url}"));
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
            sqlx::query("INSERT OR IGNORE INTO image_mappings (remote_id, file_id) VALUES (?, ?)")
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
async fn mark_task_forwarded(db: &DbPool, task_id: i64, file_id: &str) -> Result<(), AppError> {
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
            let pending: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE status = 'pending'")
                    .fetch_one(pool)
                    .await?;
            let forwarded: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE status = 'forwarded'")
                    .fetch_one(pool)
                    .await?;
            let failed: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE status = 'failed'")
                    .fetch_one(pool)
                    .await?;
            (pending.0, forwarded.0, failed.0)
        }
        DbPool::Postgres(pool) => {
            let pending: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE status = 'pending'")
                    .fetch_one(pool)
                    .await?;
            let forwarded: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE status = 'forwarded'")
                    .fetch_one(pool)
                    .await?;
            let failed: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE status = 'failed'")
                    .fetch_one(pool)
                    .await?;
            (pending.0, forwarded.0, failed.0)
        }
    };

    // 获取最近的 pending 任务列表（关联 collectors 带出频道名与采集器 ID）
    let tasks = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, ForwardTaskWithCollector>(
                "SELECT ft.*,
                  (SELECT c.id FROM collectors c WHERE c.channel_id = ft.channel_id LIMIT 1) AS collector_id,
                  (SELECT c.channel_name FROM collectors c WHERE c.channel_id = ft.channel_id LIMIT 1) AS channel_name
                 FROM forward_tasks ft WHERE ft.status = 'pending' ORDER BY ft.id ASC LIMIT 20",
            )
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, ForwardTaskWithCollector>(
                "SELECT ft.*,
                  (SELECT c.id FROM collectors c WHERE c.channel_id = ft.channel_id LIMIT 1) AS collector_id,
                  (SELECT c.channel_name FROM collectors c WHERE c.channel_id = ft.channel_id LIMIT 1) AS channel_name
                 FROM forward_tasks ft WHERE ft.status = 'pending' ORDER BY ft.id ASC LIMIT 20",
            )
            .fetch_all(pool)
            .await?
        }
    };

    // 获取失败任务列表（按更新时间倒序，LIMIT 50，关联 collectors 带出频道名与采集器 ID）
    let failed_tasks = match db {
        DbPool::Sqlite(pool) => sqlx::query_as::<_, ForwardTaskWithCollector>(
            "SELECT ft.*,
              (SELECT c.id FROM collectors c WHERE c.channel_id = ft.channel_id LIMIT 1) AS collector_id,
              (SELECT c.channel_name FROM collectors c WHERE c.channel_id = ft.channel_id LIMIT 1) AS channel_name
             FROM forward_tasks ft WHERE ft.status = 'failed' ORDER BY ft.updated_at DESC LIMIT 50",
        )
        .fetch_all(pool)
        .await?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, ForwardTaskWithCollector>(
            "SELECT ft.*,
              (SELECT c.id FROM collectors c WHERE c.channel_id = ft.channel_id LIMIT 1) AS collector_id,
              (SELECT c.channel_name FROM collectors c WHERE c.channel_id = ft.channel_id LIMIT 1) AS channel_name
             FROM forward_tasks ft WHERE ft.status = 'failed' ORDER BY ft.updated_at DESC LIMIT 50",
        )
        .fetch_all(pool)
        .await?,
    };

    Ok(serde_json::json!({
        "pending": pending,
        "forwarded": forwarded,
        "failed": failed,
        "tasks": tasks,
        "failed_tasks": failed_tasks,
    }))
}

/// 重试单个失败任务（智能恢复 — FR-052）
///
/// 根据 `image_message_id` 是否为空决定恢复目标：
/// - NULL（阶段1 失败）→ `pending`，重做阶段1
/// - 非空（阶段2 失败）→ `awaiting_bot`，仅重做阶段2，避免群组A 重复图片
pub async fn retry_task(db: &DbPool, task_id: i64) -> Result<(), AppError> {
    let rows = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE forward_tasks
                 SET status = CASE WHEN image_message_id IS NULL THEN 'pending' ELSE 'awaiting_bot' END,
                     error = NULL,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = ? AND status = 'failed'",
            )
            .bind(task_id)
            .execute(pool)
            .await?
            .rows_affected()
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE forward_tasks
                 SET status = CASE WHEN image_message_id IS NULL THEN 'pending' ELSE 'awaiting_bot' END,
                     error = NULL,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = $1 AND status = 'failed'",
            )
            .bind(task_id)
            .execute(pool)
            .await?
            .rows_affected()
        }
    };
    if rows == 0 {
        return Err(AppError::NotFound("失败任务不存在".to_string()));
    }
    Ok(())
}

/// 重试所有失败任务（智能恢复 — FR-052）
pub async fn retry_all_failed(db: &DbPool) -> Result<i64, AppError> {
    let count = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE forward_tasks
                 SET status = CASE WHEN image_message_id IS NULL THEN 'pending' ELSE 'awaiting_bot' END,
                     error = NULL,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE status = 'failed'",
            )
            .execute(pool)
            .await?
            .rows_affected() as i64
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE forward_tasks
                 SET status = CASE WHEN image_message_id IS NULL THEN 'pending' ELSE 'awaiting_bot' END,
                     error = NULL,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE status = 'failed'",
            )
            .execute(pool)
            .await?
            .rows_affected() as i64
        }
    };
    Ok(count)
}
