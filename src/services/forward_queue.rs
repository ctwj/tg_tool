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

/// recover_stuck_tasks 判定「卡住」的时间阈值倍数（×interval_secs）
pub const STUCK_THRESHOLD_MULT: u64 = 5;

/// 失败任务自动重试上限（feature 029 LOGIC-004）；retry_count >= MAX_RETRIES 为死信
pub const MAX_RETRIES: i64 = 5;

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

    // D4：启动时恢复一次卡住任务（上次进程崩溃残留的 stage1/stage2_running）
    if let Err(e) = recover_stuck_tasks(&state.db, 0).await {
        tracing::warn!("启动 recover_stuck_tasks 失败: {e}");
    }

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
                    // D4：每周期先恢复卡住任务（stage1/stage2_running 超阈值），再处理正常任务
                    let threshold = interval_secs * STUCK_THRESHOLD_MULT;
                    if let Err(e) = recover_stuck_tasks(&state_a.db, threshold).await {
                        tracing::warn!("recover_stuck_tasks 失败: {e}");
                    }
                    // feature 029 LOGIC-004：自动指数退避重试 eligible failed
                    if let Err(e) = retry_eligible_failed(&state_a.db).await {
                        tracing::warn!("retry_eligible_failed 失败: {e}");
                    }
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
                "SELECT id FROM forward_tasks WHERE remote_id = ? AND status IN ('pending', 'stage1_running', 'awaiting_bot', 'stage2_running', 'forwarded')",
            )
            .bind(remote_id)
            .fetch_optional(pool)
            .await?;
            row.is_some()
        }
        DbPool::Postgres(pool) => {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM forward_tasks WHERE remote_id = $1 AND status IN ('pending', 'stage1_running', 'awaiting_bot', 'stage2_running', 'forwarded')",
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
///
/// 任务进入时已是 `stage1_running`（由 `fetch_pending_task` 原子转移）。
/// 采用「副作用标记优先」两步（D2）：send_album 成功后**先持久化 image_message_id**
/// （保持 stage1_running），再转 awaiting_bot。崩溃恢复（recover_stuck_tasks）据此
/// 标记判定「已发」→ 不重发（修复 LOGIC-001，SC-001）。
async fn execute_stage1(state: &AppState, task: &ForwardTask, chat_id_a: &str) {
    match run_stage_1_client_forward(state, task, chat_id_a).await {
        Ok(image_message_id) => {
            // D2 步骤1：副作用标记优先持久化（保持 stage1_running，作为「已发」凭证）
            if let Err(e) = persist_stage1_marker(&state.db, task.id, image_message_id).await {
                // 标记持久化失败：副作用已发生但未记录（D6 孤儿窗口，物理限制）
                tracing::warn!(
                    "任务 {} 阶段1 副作用已发生但标记持久化失败（孤儿 remote_id={}）: {e}",
                    task.id,
                    task.remote_id
                );
                return;
            }
            // D2 步骤2：finalize 转 awaiting_bot（标记已落库，失败由 recover 兜底）
            if let Err(e) = mark_task_awaiting_bot(&state.db, task.id, image_message_id).await {
                tracing::warn!(
                    "任务 {} finalize awaiting_bot 失败（将由 recover_stuck_tasks 兜底）: {e}",
                    task.id
                );
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
            if let Err(db_err) =
                mark_task_failed(&state.db, task.id, &task.remote_id, &err_str).await
            {
                tracing::warn!("任务 {} 标记 failed 失败: {db_err}", task.id);
            }
            tracing::warn!("任务 {} {err_str}", task.id);
        }
    }
}

/// D2：阶段1 副作用标记优先持久化——写 `image_message_id`，**保持 `stage1_running`**
/// （仅记录「已发」凭证，不转移状态）。崩溃恢复据此判定。
async fn persist_stage1_marker(
    db: &DbPool,
    task_id: i64,
    image_message_id: i64,
) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET image_message_id = ?, updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ? AND status = 'stage1_running'",
            )
            .bind(image_message_id)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET image_message_id = $1, updated_at = CURRENT_TIMESTAMP \
                 WHERE id = $2 AND status = 'stage1_running'",
            )
            .bind(image_message_id)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 执行单个任务的阶段2：Bot forwardMessage 转发到群组B 取 file_id
///
/// 任务进入时已是 `stage2_running`（由 `fetch_awaiting_bot_task` 原子转移）。
/// 采用「副作用标记优先」三步（D2）：forwardMessage 成功后**先持久化 file_id**
/// （保持 stage2_running）→ save_mapping（去重核心，UNIQUE remote_id）→ 转 forwarded。
/// 崩溃恢复据此判定「已转」→ 不重发 + 补全映射（修复 LOGIC-002，SC-002）。
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
            // D2 步骤1：副作用标记优先持久化（file_id，保持 stage2_running，作为「已转」凭证）
            if let Err(e) = persist_stage2_marker(&state.db, task.id, &file_id).await {
                tracing::warn!(
                    "任务 {} 阶段2 副作用已发生但标记持久化失败（孤儿 remote_id={}）: {e}",
                    task.id,
                    task.remote_id
                );
                return;
            }
            // D2 步骤2：save_mapping 优先（去重核心，UNIQUE remote_id；失败由 recover 补全）
            if let Err(e) = save_mapping(&state.db, &task.remote_id, &file_id).await {
                tracing::warn!(
                    "任务 {} save_mapping 失败（将由 recover_stuck_tasks 补全）: {e}",
                    task.id
                );
            }
            // D2 步骤3：finalize 转 forwarded（失败由 recover 兜底：stage2_running+file_id 非空→forwarded）
            if let Err(e) = mark_task_forwarded(&state.db, task.id, &file_id).await {
                tracing::warn!(
                    "任务 {} finalize forwarded 失败（将由 recover_stuck_tasks 兜底）: {e}",
                    task.id
                );
            }
            tracing::info!("阶段2 成功: task={}, file_id={}", task.id, file_id);
        }
        Err(e) => {
            let err_str = e.to_string();
            let is_flood = err_str.contains("FLOOD_WAIT");
            if let Err(db_err) =
                mark_task_failed(&state.db, task.id, &task.remote_id, &err_str).await
            {
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

/// D2：阶段2 副作用标记优先持久化——写 `file_id`，**保持 `stage2_running`**
/// （仅记录「已转」凭证，不转移状态）。崩溃恢复据此判定并补全映射。
async fn persist_stage2_marker(db: &DbPool, task_id: i64, file_id: &str) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET file_id = ?, updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ? AND status = 'stage2_running'",
            )
            .bind(file_id)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE forward_tasks SET file_id = $1, updated_at = CURRENT_TIMESTAMP \
                 WHERE id = $2 AND status = 'stage2_running'",
            )
            .bind(file_id)
            .bind(task_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 单 worker 模式入口：阶段1 优先，无则阶段2（保持旧行为，群组A==B 或 B 未配置时使用）
async fn process_next_task(state: &AppState) -> Result<(), AppError> {
    let (bot_token, chat_id_a, chat_id_b, delete_temp, proxy_url) =
        match read_pipeline_config(state).await {
            Some(cfg) => cfg,
            None => return Ok(()), // 配置不全，静默跳过
        };

    // 公平调度（D5，修复 LOGIC-003）：awaiting_bot 优先，消除 pending 独占饥饿。
    // awaiting_bot 非空先取（保证有界处理 SC-003），无则取 pending（产生新 awaiting_bot）；
    // 因 awaiting_bot 由 pending→阶段1 产生，awaiting_bot 消化后必回到 pending，两边皆不饿死。
    let task = match fetch_awaiting_bot_task(&state.db).await? {
        Some(t) => t,
        None => match fetch_pending_task(&state.db).await? {
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
        // fetch_pending_task / fetch_awaiting_bot_task 已原子转移到处理中态
        "stage1_running" => execute_stage1(state, &task, &chat_id_a).await,
        "stage2_running" => {
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
            tracing::warn!(
                "任务 {} 处于非预期状态: {other}（应为 stage1/stage2_running）",
                task.id
            );
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

/// 取一个 pending 任务并**原子转移**到 `stage1_running`（D3，防重取）
///
/// 用 `UPDATE ... WHERE id = (SELECT ... pending LIMIT 1) RETURNING *` 保证
/// 多 worker / 恢复并发下恰好一个执行者取得任务（FR-005 / contracts §2 不变量2）。
async fn fetch_pending_task(db: &DbPool) -> Result<Option<ForwardTask>, AppError> {
    let sql = "UPDATE forward_tasks SET status = 'stage1_running', updated_at = CURRENT_TIMESTAMP \
               WHERE id = (SELECT id FROM forward_tasks WHERE status = 'pending' ORDER BY id ASC LIMIT 1) \
               RETURNING *";
    match db {
        DbPool::Sqlite(pool) => Ok(sqlx::query_as::<_, ForwardTask>(sql)
            .fetch_optional(pool)
            .await?),
        DbPool::Postgres(pool) => Ok(sqlx::query_as::<_, ForwardTask>(sql)
            .fetch_optional(pool)
            .await?),
    }
}

/// 取一个 awaiting_bot 任务并**原子转移**到 `stage2_running`（D3，防重取）
async fn fetch_awaiting_bot_task(db: &DbPool) -> Result<Option<ForwardTask>, AppError> {
    let sql = "UPDATE forward_tasks SET status = 'stage2_running', updated_at = CURRENT_TIMESTAMP \
               WHERE id = (SELECT id FROM forward_tasks WHERE status = 'awaiting_bot' ORDER BY id ASC LIMIT 1) \
               RETURNING *";
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

/// 标记任务为 failed。feature 040：若 retry_count 自增后 >= MAX_RETRIES（死信），
/// 在**同一事务**内清空关联资源 `extracted_resources.img`（WHERE img = remote_id）
/// 并删除该失败任务行——任一步失败整体回滚（FR-006 原子性）。日志在 commit 成功后记录，
/// 避免出现"日志说清了但事务回滚"的误导（research D7）。
async fn mark_task_failed(
    db: &DbPool,
    task_id: i64,
    remote_id: &str,
    error: &str,
) -> Result<(), AppError> {
    let cleaned = match db {
        DbPool::Sqlite(pool) => {
            let mut tx = pool.begin().await?;
            sqlx::query(
                "UPDATE forward_tasks SET status='failed', error=?, retry_count=retry_count+1, updated_at=CURRENT_TIMESTAMP WHERE id=?",
            )
            .bind(error)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
            let (retry_count,): (i64,) =
                sqlx::query_as("SELECT retry_count FROM forward_tasks WHERE id=?")
                    .bind(task_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if retry_count >= MAX_RETRIES {
                sqlx::query("UPDATE extracted_resources SET img=NULL WHERE img=?")
                    .bind(remote_id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM forward_tasks WHERE id=?")
                    .bind(task_id)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                true
            } else {
                tx.commit().await?;
                false
            }
        }
        DbPool::Postgres(pool) => {
            let mut tx = pool.begin().await?;
            sqlx::query(
                "UPDATE forward_tasks SET status='failed', error=$1, retry_count=retry_count+1, updated_at=CURRENT_TIMESTAMP WHERE id=$2",
            )
            .bind(error)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
            let (retry_count,): (i64,) =
                sqlx::query_as("SELECT retry_count FROM forward_tasks WHERE id=$1")
                    .bind(task_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if retry_count >= MAX_RETRIES {
                sqlx::query("UPDATE extracted_resources SET img=NULL WHERE img=$1")
                    .bind(remote_id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM forward_tasks WHERE id=$1")
                    .bind(task_id)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                true
            } else {
                tx.commit().await?;
                false
            }
        }
    };

    if cleaned {
        tracing::info!(
            "死信自动清理: task={} remote_id={} cleared_resource_img=true",
            task_id,
            remote_id
        );
    } else {
        tracing::debug!(
            "任务标记失败（未达死信阈值）: task={} remote_id={}",
            task_id,
            remote_id
        );
    }
    Ok(())
}

// ─── 队列管理 API 辅助函数 ─────────────────────────────────────────────────

/// 获取队列统计
pub async fn queue_status(db: &DbPool) -> Result<serde_json::Value, AppError> {
    // 单次 GROUP BY 取全部状态计数（FR-007 可观测，避免多次 COUNT 往返）
    let counts: std::collections::HashMap<String, i64> = match db {
        DbPool::Sqlite(pool) => {
            let rows: Vec<(String, i64)> =
                sqlx::query_as("SELECT status, COUNT(*) FROM forward_tasks GROUP BY status")
                    .fetch_all(pool)
                    .await?;
            rows.into_iter().collect()
        }
        DbPool::Postgres(pool) => {
            let rows: Vec<(String, i64)> =
                sqlx::query_as("SELECT status, COUNT(*) FROM forward_tasks GROUP BY status")
                    .fetch_all(pool)
                    .await?;
            rows.into_iter().collect()
        }
    };
    let pending = *counts.get("pending").unwrap_or(&0);
    let stage1_running = *counts.get("stage1_running").unwrap_or(&0);
    let awaiting_bot = *counts.get("awaiting_bot").unwrap_or(&0);
    let stage2_running = *counts.get("stage2_running").unwrap_or(&0);
    let forwarded = *counts.get("forwarded").unwrap_or(&0);
    let failed = *counts.get("failed").unwrap_or(&0);
    // feature 029 LOGIC-004：死信计数（failed AND retry_count >= MAX_RETRIES，需人工处理）
    let dead = match db {
        DbPool::Sqlite(pool) => {
            let (d,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM forward_tasks WHERE status='failed' AND retry_count >= ?",
            )
            .bind(MAX_RETRIES)
            .fetch_one(pool)
            .await?;
            d
        }
        DbPool::Postgres(pool) => {
            let (d,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM forward_tasks WHERE status='failed' AND retry_count >= $1",
            )
            .bind(MAX_RETRIES)
            .fetch_one(pool)
            .await?;
            d
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
        "stage1_running": stage1_running,
        "awaiting_bot": awaiting_bot,
        "stage2_running": stage2_running,
        "forwarded": forwarded,
        "failed": failed,
        "dead": dead,
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
///
/// 用户主动触发「全部重试」：循环分批 UPDATE（每批 100 条小事务），直到清空所有 failed。
/// 自动防风暴由 `retry_eligible_failed`（指数退避）负责，与本函数职责不同。
/// 每批 UPDATE 后被恢复的行 status 不再是 'failed'，下一轮子查询天然不重复命中，无死循环风险。
pub async fn retry_all_failed(db: &DbPool) -> Result<i64, AppError> {
    // 双方言占位符：SQLite 用 ?，PostgreSQL 用 $N（sqlx::query 运行时不做转换）。
    const BATCH: i64 = 100;
    let mut total: i64 = 0;
    loop {
        let n = match db {
            DbPool::Sqlite(pool) => {
                sqlx::query(
                    "UPDATE forward_tasks
                     SET status = CASE WHEN image_message_id IS NULL THEN 'pending' ELSE 'awaiting_bot' END,
                         error = NULL, updated_at = CURRENT_TIMESTAMP
                     WHERE id IN (SELECT id FROM forward_tasks WHERE status = 'failed' ORDER BY id LIMIT ?)",
                )
                .bind(BATCH)
                .execute(pool)
                .await?
                .rows_affected() as i64
            }
            DbPool::Postgres(pool) => {
                sqlx::query(
                    "UPDATE forward_tasks
                     SET status = CASE WHEN image_message_id IS NULL THEN 'pending' ELSE 'awaiting_bot' END,
                         error = NULL, updated_at = CURRENT_TIMESTAMP
                     WHERE id IN (SELECT id FROM forward_tasks WHERE status = 'failed' ORDER BY id LIMIT $1)",
                )
                .bind(BATCH)
                .execute(pool)
                .await?
                .rows_affected() as i64
            }
        };
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

/// feature 029 LOGIC-004：自动指数退避重试 eligible failed 任务。
///
/// 扫描 `retry_count < MAX_RETRIES` 的 failed，过退避窗口（`2^retry_count` 秒，cap 300s）则
/// 智能恢复（image_message_id NULL→pending，非空→awaiting_bot），**保留 retry_count**（累计退避）。
/// 死信（retry_count >= MAX_RETRIES）不自动回（需人工 retry_task reset）。调度循环每 tick 调用。
/// 应用层计算退避（避开 SQLite 无 power() 的双方言难题）。
pub async fn retry_eligible_failed(db: &DbPool) -> Result<i64, AppError> {
    let rows: Vec<(i64, i64, Option<i64>, chrono::NaiveDateTime)> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT id, retry_count, image_message_id, updated_at FROM forward_tasks
                 WHERE status='failed' AND retry_count < ?",
            )
            .bind(MAX_RETRIES)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT id, retry_count, image_message_id, updated_at FROM forward_tasks
                 WHERE status='failed' AND retry_count < $1",
            )
            .bind(MAX_RETRIES)
            .fetch_all(pool)
            .await?
        }
    };
    let now = chrono::Utc::now().naive_utc();
    let mut recovered: i64 = 0;
    for (id, retry_count, image_message_id, updated_at) in rows {
        let backoff = (1i64 << retry_count.min(10)).min(300); // 2^retry_count，cap 300s
        if (now - updated_at).num_seconds() < backoff {
            continue; // 退避窗口未到
        }
        let status = if image_message_id.is_none() {
            "pending"
        } else {
            "awaiting_bot"
        };
        match db {
            DbPool::Sqlite(pool) => {
                let _ = sqlx::query(
                    "UPDATE forward_tasks SET status=?, updated_at=CURRENT_TIMESTAMP WHERE id=?",
                )
                .bind(status)
                .bind(id)
                .execute(pool)
                .await;
            }
            DbPool::Postgres(pool) => {
                let _ = sqlx::query(
                    "UPDATE forward_tasks SET status=$1, updated_at=CURRENT_TIMESTAMP WHERE id=$2",
                )
                .bind(status)
                .bind(id)
                .execute(pool)
                .await;
            }
        };
        recovered += 1;
    }
    if recovered > 0 {
        tracing::info!("retry_eligible_failed 恢复 {recovered} 个 failed 任务（指数退避自动重试）");
    }
    Ok(recovered)
}

/// D4：崩溃恢复扫描——处理 `stage1_running` / `stage2_running` 的「卡住」任务
///
/// 启动时 + 调度器周期触发。基于副作用标记（image_message_id / file_id）**确定性**恢复
///（contracts/state-machine.md 不变量3）：
/// - `stage1_running` + `image_message_id` NULL → `pending`（副作用未完成，安全重试）
/// - `stage1_running` + `image_message_id` 非空 → `awaiting_bot`（已发，**不重发**）
/// - `stage2_running` + `file_id` NULL → `awaiting_bot`（未转，重试）
/// - `stage2_running` + `file_id` 非空 → 补 `image_mappings` → `forwarded`（已转，**不重发**）
///
/// `threshold_secs`：仅恢复 `updated_at` 早于 `now - threshold` 的任务（避免误伤进行中任务）。
/// 返回恢复的任务数。
pub async fn recover_stuck_tasks(db: &DbPool, threshold_secs: u64) -> Result<i64, AppError> {
    let mut recovered: i64 = 0;
    match db {
        DbPool::Sqlite(pool) => {
            let cutoff = format!("-{} seconds", threshold_secs as i64);
            // 阶段1：image_message_id NULL → pending（安全重试）
            recovered += sqlx::query(
                "UPDATE forward_tasks SET status='pending', updated_at=CURRENT_TIMESTAMP \
                 WHERE status='stage1_running' AND image_message_id IS NULL \
                 AND updated_at < datetime('now', ?)",
            )
            .bind(&cutoff)
            .execute(pool)
            .await?
            .rows_affected() as i64;
            // 阶段1：image_message_id 非空 → awaiting_bot（不重发）
            recovered += sqlx::query(
                "UPDATE forward_tasks SET status='awaiting_bot', updated_at=CURRENT_TIMESTAMP \
                 WHERE status='stage1_running' AND image_message_id IS NOT NULL \
                 AND updated_at < datetime('now', ?)",
            )
            .bind(&cutoff)
            .execute(pool)
            .await?
            .rows_affected() as i64;
            // 阶段2：file_id NULL → awaiting_bot（重试）
            recovered += sqlx::query(
                "UPDATE forward_tasks SET status='awaiting_bot', updated_at=CURRENT_TIMESTAMP \
                 WHERE status='stage2_running' AND file_id IS NULL \
                 AND updated_at < datetime('now', ?)",
            )
            .bind(&cutoff)
            .execute(pool)
            .await?
            .rows_affected() as i64;
            // 阶段2：file_id 非空 → 先补 image_mappings，再 forwarded（不重发）
            let stuck_done: Vec<(String, String)> = sqlx::query_as(
                "SELECT remote_id, file_id FROM forward_tasks \
                 WHERE status='stage2_running' AND file_id IS NOT NULL \
                 AND updated_at < datetime('now', ?)",
            )
            .bind(&cutoff)
            .fetch_all(pool)
            .await?;
            for (remote_id, file_id) in &stuck_done {
                let _ = sqlx::query(
                    "INSERT OR IGNORE INTO image_mappings (remote_id, file_id) VALUES (?, ?)",
                )
                .bind(remote_id)
                .bind(file_id)
                .execute(pool)
                .await;
            }
            recovered += sqlx::query(
                "UPDATE forward_tasks SET status='forwarded', updated_at=CURRENT_TIMESTAMP \
                 WHERE status='stage2_running' AND file_id IS NOT NULL \
                 AND updated_at < datetime('now', ?)",
            )
            .bind(&cutoff)
            .execute(pool)
            .await?
            .rows_affected() as i64;
        }
        DbPool::Postgres(pool) => {
            // threshold_secs 为内部常量派生，非用户输入，format! 拼接 interval 无注入风险
            let intv = format!("interval '{} seconds'", threshold_secs as i64);
            recovered += sqlx::query(&format!(
                "UPDATE forward_tasks SET status='pending', updated_at=CURRENT_TIMESTAMP \
                 WHERE status='stage1_running' AND image_message_id IS NULL \
                 AND updated_at < now() - {intv}"
            ))
            .execute(pool)
            .await?
            .rows_affected() as i64;
            recovered += sqlx::query(&format!(
                "UPDATE forward_tasks SET status='awaiting_bot', updated_at=CURRENT_TIMESTAMP \
                 WHERE status='stage1_running' AND image_message_id IS NOT NULL \
                 AND updated_at < now() - {intv}"
            ))
            .execute(pool)
            .await?
            .rows_affected() as i64;
            recovered += sqlx::query(&format!(
                "UPDATE forward_tasks SET status='awaiting_bot', updated_at=CURRENT_TIMESTAMP \
                 WHERE status='stage2_running' AND file_id IS NULL \
                 AND updated_at < now() - {intv}"
            ))
            .execute(pool)
            .await?
            .rows_affected() as i64;
            let stuck_done: Vec<(String, String)> = sqlx::query_as(&format!(
                "SELECT remote_id, file_id FROM forward_tasks \
                 WHERE status='stage2_running' AND file_id IS NOT NULL \
                 AND updated_at < now() - {intv}"
            ))
            .fetch_all(pool)
            .await?;
            for (remote_id, file_id) in &stuck_done {
                let _ = sqlx::query(
                    "INSERT INTO image_mappings (remote_id, file_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(remote_id)
                .bind(file_id)
                .execute(pool)
                .await;
            }
            recovered += sqlx::query(&format!(
                "UPDATE forward_tasks SET status='forwarded', updated_at=CURRENT_TIMESTAMP \
                 WHERE status='stage2_running' AND file_id IS NOT NULL \
                 AND updated_at < now() - {intv}"
            ))
            .execute(pool)
            .await?
            .rows_affected() as i64;
        }
    }
    if recovered > 0 {
        tracing::info!("recover_stuck_tasks 恢复 {recovered} 个卡住任务（stage1/stage2_running）");
    }
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 测试用 SQLite 内存库（008 迁移 + image_message_id 字段）
    async fn setup_db() -> DbPool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect test db");
        sqlx::raw_sql(include_str!("../../migrations/008_image_tables_sqlite.sql"))
            .execute(&pool)
            .await
            .expect("run 008 migration");
        // image_message_id 由 Migration 015（main.rs 内联）添加，测试手动补字段
        let _ = sqlx::raw_sql("ALTER TABLE forward_tasks ADD COLUMN image_message_id INTEGER")
            .execute(&pool)
            .await;
        // feature 040：死信清理测试需要 extracted_resources 表（003 migration）。
        // sqlx 的 SQLite 连接默认 foreign_keys=ON，003 的 collector_history_id
        // 外键需要依赖表存在，故先加载 001_init（含 collector_histories）。
        sqlx::raw_sql(include_str!("../../migrations/001_init_sqlite.sql"))
            .execute(&pool)
            .await
            .expect("run 001 migration");
        sqlx::raw_sql(include_str!(
            "../../migrations/003_extracted_resources_sqlite.sql"
        ))
        .execute(&pool)
        .await
        .expect("run 003 migration");
        DbPool::Sqlite(pool)
    }

    async fn insert_task(
        db: &DbPool,
        remote_id: &str,
        status: &str,
        img_msg: Option<i64>,
        file_id: Option<&str>,
    ) -> i64 {
        match db {
            DbPool::Sqlite(pool) => sqlx::query(
                "INSERT INTO forward_tasks (remote_id, status, image_message_id, file_id) VALUES (?, ?, ?, ?)",
            )
            .bind(remote_id)
            .bind(status)
            .bind(img_msg)
            .bind(file_id)
            .execute(pool)
            .await
            .expect("insert task")
            .last_insert_rowid(),
            _ => unreachable!(),
        }
    }

    /// feature 040：插入一条 extracted_resources 记录。
    /// sqlx SQLite 连接默认 foreign_keys=ON，需先建幂等占位外键链：
    /// users(1) → collectors(1) → collector_histories(1) → extracted_resources。
    async fn insert_resource(db: &DbPool, title: &str, img: Option<&str>) -> i64 {
        match db {
            DbPool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT OR IGNORE INTO users (id, username, password) VALUES (1, 'test', 'p')",
                )
                .execute(pool)
                .await
                .expect("placeholder user");
                sqlx::query(
                    "INSERT OR IGNORE INTO collectors (id, user_id, channel_id, collector_type) \
                     VALUES (1, 1, 1, 'test')",
                )
                .execute(pool)
                .await
                .expect("placeholder collector");
                sqlx::query(
                    "INSERT OR IGNORE INTO collector_histories \
                     (id, collector_id, channel_id, message_id) VALUES (1, 1, 1, 1)",
                )
                .execute(pool)
                .await
                .expect("placeholder collector_history");
                sqlx::query(
                    "INSERT INTO extracted_resources (collector_history_id, title, img, source) \
                     VALUES (?, ?, ?, 'tg')",
                )
                .bind(1i64)
                .bind(title)
                .bind(img)
                .execute(pool)
                .await
                .expect("insert resource")
                .last_insert_rowid()
            }
            _ => unreachable!(),
        }
    }

    /// feature 040：读取资源的 img 字段（死信清理后应为 None）。
    async fn get_resource_img(db: &DbPool, id: i64) -> Option<String> {
        match db {
            DbPool::Sqlite(pool) => {
                let (v,): (Option<String>,) =
                    sqlx::query_as("SELECT img FROM extracted_resources WHERE id = ?")
                        .bind(id)
                        .fetch_one(pool)
                        .await
                        .expect("get resource img");
                v
            }
            _ => unreachable!(),
        }
    }

    async fn get_status(db: &DbPool, id: i64) -> String {
        match db {
            DbPool::Sqlite(pool) => {
                let (s,): (String,) =
                    sqlx::query_as("SELECT status FROM forward_tasks WHERE id = ?")
                        .bind(id)
                        .fetch_one(pool)
                        .await
                        .expect("get status");
                s
            }
            _ => unreachable!(),
        }
    }

    async fn get_image_message_id(db: &DbPool, id: i64) -> Option<i64> {
        match db {
            DbPool::Sqlite(pool) => {
                let (v,): (Option<i64>,) =
                    sqlx::query_as("SELECT image_message_id FROM forward_tasks WHERE id = ?")
                        .bind(id)
                        .fetch_one(pool)
                        .await
                        .expect("get img id");
                v
            }
            _ => unreachable!(),
        }
    }

    /// 模拟任务「卡住」——把 updated_at 设为 N 秒前（recover 仅恢复超 threshold 的任务）
    async fn set_updated_ago(db: &DbPool, id: i64, secs_ago: i64) {
        let cutoff = format!("-{} seconds", secs_ago);
        match db {
            DbPool::Sqlite(pool) => {
                sqlx::query(
                    "UPDATE forward_tasks SET updated_at = datetime('now', ?) WHERE id = ?",
                )
                .bind(&cutoff)
                .bind(id)
                .execute(pool)
                .await
                .expect("set updated_at");
            }
            _ => unreachable!(),
        }
    }

    /// T007 / SC-001：阶段1 副作用已发生（image_message_id 已持久化）后崩溃，
    /// recover 转 awaiting_bot——**不回 pending、不重发**群组A。
    #[tokio::test]
    async fn test_stage1_marker_present_no_resend() {
        let db = setup_db().await;
        let id = insert_task(&db, "img1", "stage1_running", Some(999), None).await;
        set_updated_ago(&db, id, 100).await; // 模拟卡住 100s
        let recovered = recover_stuck_tasks(&db, 0).await.expect("recover");
        assert_eq!(recovered, 1);
        assert_eq!(get_status(&db, id).await, "awaiting_bot"); // 不回 pending → 不重发
        assert_eq!(get_image_message_id(&db, id).await, Some(999)); // 标记不丢失
    }

    /// 阶段1 副作用未完成（image_message_id NULL）→ recover 回 pending（安全重试）。
    #[tokio::test]
    async fn test_stage1_marker_absent_safe_retry() {
        let db = setup_db().await;
        let id = insert_task(&db, "img2", "stage1_running", None, None).await;
        set_updated_ago(&db, id, 100).await;
        recover_stuck_tasks(&db, 0).await.expect("recover");
        assert_eq!(get_status(&db, id).await, "pending");
    }

    /// T010 / SC-002：阶段2 副作用已发生（file_id 已持久化）后崩溃，
    /// recover 转 forwarded + 补全 image_mappings——**不重发**群组B、去重恢复。
    #[tokio::test]
    async fn test_stage2_marker_present_no_resend_and_mapping() {
        let db = setup_db().await;
        let id = insert_task(&db, "img3", "stage2_running", Some(100), Some("file123")).await;
        set_updated_ago(&db, id, 100).await;
        recover_stuck_tasks(&db, 0).await.expect("recover");
        assert_eq!(get_status(&db, id).await, "forwarded"); // 不重发
        match &db {
            DbPool::Sqlite(pool) => {
                let (fid,): (String,) =
                    sqlx::query_as("SELECT file_id FROM image_mappings WHERE remote_id = ?")
                        .bind("img3")
                        .fetch_one(pool)
                        .await
                        .expect("mapping should be backfilled");
                assert_eq!(fid, "file123"); // 去重映射恢复
            }
            _ => unreachable!(),
        }
    }

    /// 阶段2 副作用未完成（file_id NULL）→ recover 回 awaiting_bot（重试）。
    #[tokio::test]
    async fn test_stage2_marker_absent_retry() {
        let db = setup_db().await;
        let id = insert_task(&db, "img4", "stage2_running", Some(100), None).await;
        set_updated_ago(&db, id, 100).await;
        recover_stuck_tasks(&db, 0).await.expect("recover");
        assert_eq!(get_status(&db, id).await, "awaiting_bot");
    }

    /// T011 / SC-004：fetch 原子转移——pending→stage1_running，且两次 fetch 不取同一任务（防重取）。
    #[tokio::test]
    async fn test_fetch_atomic_no_double_take() {
        let db = setup_db().await;
        let id = insert_task(&db, "img5", "pending", None, None).await;
        let t1 = fetch_pending_task(&db)
            .await
            .expect("fetch1")
            .expect("got task");
        assert_eq!(t1.id, id);
        assert_eq!(t1.status, "stage1_running"); // 原子转移
        // 第二次 fetch pending → None（已被转移，防重取）
        let t2 = fetch_pending_task(&db).await.expect("fetch2");
        assert!(t2.is_none());
    }

    /// T014 / SC-003：公平调度——awaiting_bot 优先取（不饥饿）。
    /// pending 与 awaiting_bot 共存时，process_next_task 的公平策略令 awaiting_bot 先被 fetch。
    #[tokio::test]
    async fn test_fair_scheduling_awaiting_bot_first() {
        let db = setup_db().await;
        let _pid = insert_task(&db, "p1", "pending", None, None).await;
        let aid = insert_task(&db, "a1", "awaiting_bot", Some(50), None).await;
        // 公平策略：awaiting_bot 优先 → fetch_awaiting_bot_task 先返回它
        let t = fetch_awaiting_bot_task(&db)
            .await
            .expect("fetch")
            .expect("got task");
        assert_eq!(t.id, aid); // awaiting_bot 被取，不饥饿
        assert_eq!(t.status, "stage2_running");
    }

    /// 辅助：设置 retry_count + updated_at（模拟 failed 任务的退避状态）
    async fn set_retry_count_and_ago(db: &DbPool, id: i64, retry_count: i64, secs_ago: i64) {
        let cutoff = format!("-{} seconds", secs_ago);
        match db {
            DbPool::Sqlite(pool) => {
                sqlx::query(
                    "UPDATE forward_tasks SET retry_count=?, updated_at=datetime('now', ?) WHERE id=?",
                )
                .bind(retry_count)
                .bind(&cutoff)
                .bind(id)
                .execute(pool)
                .await
                .expect("set retry_count");
            }
            _ => unreachable!(),
        }
    }

    /// feature 029 LOGIC-004：退避窗口未到不自动回
    #[tokio::test]
    async fn test_retry_backoff_not_yet() {
        let db = setup_db().await;
        let id = insert_task(&db, "rt1", "failed", None, None).await;
        set_retry_count_and_ago(&db, id, 1, 1).await; // retry_count=1 退避 2s，updated 1s 前
        let recovered = retry_eligible_failed(&db).await.unwrap();
        assert_eq!(recovered, 0); // 退避未到
        assert_eq!(get_status(&db, id).await, "failed");
    }

    /// 退避窗口已到 → 自动恢复（image_message_id NULL → pending）
    #[tokio::test]
    async fn test_retry_backoff_elapsed() {
        let db = setup_db().await;
        let id = insert_task(&db, "rt2", "failed", None, None).await;
        set_retry_count_and_ago(&db, id, 1, 3).await; // 退避 2s，updated 3s 前（已过）
        let recovered = retry_eligible_failed(&db).await.unwrap();
        assert_eq!(recovered, 1);
        assert_eq!(get_status(&db, id).await, "pending");
    }

    /// 死信（retry_count >= MAX_RETRIES）不自动回
    #[tokio::test]
    async fn test_dead_letter_not_retried() {
        let db = setup_db().await;
        let id = insert_task(&db, "rt3", "failed", None, None).await;
        set_retry_count_and_ago(&db, id, MAX_RETRIES, 1000).await; // 死信 + 很久前
        let recovered = retry_eligible_failed(&db).await.unwrap();
        assert_eq!(recovered, 0); // 死信不回（WHERE retry_count < MAX 不查）
        assert_eq!(get_status(&db, id).await, "failed");
    }

    /// 「全部重试」必须恢复所有 failed 任务，而非仅前 100 条。
    /// 回归 feature 029 LIMIT 100 的硬编码上限——用户主动点击应一次清空。
    #[tokio::test]
    async fn test_retry_all_failed_recovers_all_beyond_batch_limit() {
        let db = setup_db().await;
        // 插入 150 条 failed（超过单批 100）
        let mut ids = Vec::with_capacity(150);
        for i in 0..150 {
            ids.push(insert_task(&db, &format!("rid-batch-{i}"), "failed", None, None).await);
        }
        // 混入几条带 image_message_id 的，验证 awaiting_bot 分支也在批量内
        for i in 0..5 {
            ids.push(
                insert_task(
                    &db,
                    &format!("rid-batch-img-{i}"),
                    "failed",
                    Some(999_000 + i),
                    None,
                )
                .await,
            );
        }

        let retried = retry_all_failed(&db).await.expect("retry all");
        assert_eq!(retried, 155); // 全部恢复，不是 100

        // 全部离开 failed 状态
        let still_failed: i64 = match &db {
            DbPool::Sqlite(p) => {
                sqlx::query_scalar("SELECT COUNT(*) FROM forward_tasks WHERE status = 'failed'")
                    .fetch_one(p)
                    .await
                    .unwrap()
            }
            _ => unreachable!(),
        };
        assert_eq!(still_failed, 0);

        // 再次调用应返回 0（已清空，无死循环）
        let again = retry_all_failed(&db).await.unwrap();
        assert_eq!(again, 0);
    }

    // ─── feature 040：死信自动清理（清空资源 img + 删除失败任务）─────────────

    /// FR-001/FR-002：retry_count 达 MAX_RETRIES 的瞬间，事务内清空关联资源 img
    /// 并删除该失败任务行。
    #[tokio::test]
    async fn test_dead_letter_clears_resource_img_and_deletes_task() {
        let db = setup_db().await;
        // 资源 img = remote_id
        let rid = insert_resource(&db, "死信测试资源", Some("rid-dl")).await;
        // 任务已失败 4 次（再失败一次即 retry_count=5=MAX_RETRIES → 死信）
        let tid = insert_task(&db, "rid-dl", "failed", None, None).await;
        set_retry_count_and_ago(&db, tid, MAX_RETRIES - 1, 0).await;

        mark_task_failed(&db, tid, "rid-dl", "第 5 次失败")
            .await
            .expect("mark failed");

        // (a) 任务行被删除
        let still_exists: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM forward_tasks WHERE id = ?")
                .bind(tid)
                .fetch_optional(match &db {
                    DbPool::Sqlite(p) => p,
                    _ => unreachable!(),
                })
                .await
                .expect("check task");
        assert!(still_exists.is_none(), "死信任务行应被删除");

        // (b) 资源 img 被置 NULL（资源记录本身保留 — 由 US2 测试覆盖更多字段）
        assert_eq!(get_resource_img(&db, rid).await, None);
    }

    /// FR-001 反向：retry_count < MAX_RETRIES 时，仅自增计数，不清 img 不删行。
    #[tokio::test]
    async fn test_failed_below_threshold_keeps_task_and_img() {
        let db = setup_db().await;
        let rid = insert_resource(&db, "非死信资源", Some("rid-ok")).await;
        let tid = insert_task(&db, "rid-ok", "failed", None, None).await;
        set_retry_count_and_ago(&db, tid, 3, 0).await; // retry_count=3

        mark_task_failed(&db, tid, "rid-ok", "第 4 次失败")
            .await
            .expect("mark failed");

        // 任务仍在、status=failed、retry_count=4
        assert_eq!(get_status(&db, tid).await, "failed");
        let rc: i64 = sqlx::query_scalar("SELECT retry_count FROM forward_tasks WHERE id = ?")
            .bind(tid)
            .fetch_one(match &db {
                DbPool::Sqlite(p) => p,
                _ => unreachable!(),
            })
            .await
            .expect("get retry_count");
        assert_eq!(rc, 4);
        // 资源 img 保持原值
        assert_eq!(get_resource_img(&db, rid).await.as_deref(), Some("rid-ok"));
    }

    /// FR-003 / User Story 2：死信清理仅清空 img，资源记录其他字段（title/url/desc/
    /// category/tags/is_pushed/is_edited）100% 保留。
    #[tokio::test]
    #[allow(clippy::type_complexity)] // 测试用 8 元组一次性取全字段，比拆多次 SELECT 更直观
    async fn test_resource_non_img_fields_preserved_after_cleanup() {
        let db = setup_db().await;
        let pool = match &db {
            DbPool::Sqlite(p) => p,
            _ => unreachable!(),
        };
        // 先建占位链，再用完整字段插入资源
        insert_resource(&db, "占位", Some("rid-preserve")).await;
        sqlx::query(
            "UPDATE extracted_resources SET \
             title='完整资源', url='https://example.com/x', description='描述', \
             category='quark', tags='电影,动作', is_pushed=1, is_edited=1 \
             WHERE img='rid-preserve'",
        )
        .execute(pool)
        .await
        .expect("update resource fields");
        let rid: i64 =
            sqlx::query_scalar("SELECT id FROM extracted_resources WHERE img='rid-preserve'")
                .fetch_one(pool)
                .await
                .expect("get resource id");
        let tid = insert_task(&db, "rid-preserve", "failed", None, None).await;
        set_retry_count_and_ago(&db, tid, MAX_RETRIES - 1, 0).await;

        mark_task_failed(&db, tid, "rid-preserve", "死信")
            .await
            .expect("mark failed");

        // 资源行仍在，非 img 字段原样保留，仅 img=NULL
        let row: (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            bool,
            bool,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT title, url, description, category, tags, is_pushed, is_edited, img \
             FROM extracted_resources WHERE id=?",
        )
        .bind(rid)
        .fetch_one(pool)
        .await
        .expect("fetch resource");
        assert_eq!(row.0, "完整资源");
        assert_eq!(row.1.as_deref(), Some("https://example.com/x"));
        assert_eq!(row.2.as_deref(), Some("描述"));
        assert_eq!(row.3.as_deref(), Some("quark"));
        assert_eq!(row.4.as_deref(), Some("电影,动作"));
        assert!(row.5, "is_pushed 保留");
        assert!(row.6, "is_edited 保留");
        assert_eq!(row.7, None, "img 被清空");
    }

    /// Edge Case：资源 img 已是 NULL（用户手动清空过）→ 清理仍 OK，幂等不报错。
    #[tokio::test]
    async fn test_dead_letter_clear_idempotent_on_null_img() {
        let db = setup_db().await;
        let rid = insert_resource(&db, "已无图资源", None).await; // img 已 NULL
        let tid = insert_task(&db, "rid-null", "failed", None, None).await;
        set_retry_count_and_ago(&db, tid, MAX_RETRIES - 1, 0).await;

        // remote_id 与资源 img 不同（资源 img 已是 NULL，不存在匹配行）
        mark_task_failed(&db, tid, "rid-null", "死信")
            .await
            .expect("mark failed should be ok");

        // 任务行被删
        let pool = match &db {
            DbPool::Sqlite(p) => p,
            _ => unreachable!(),
        };
        let still: Option<(i64,)> = sqlx::query_as("SELECT id FROM forward_tasks WHERE id=?")
            .bind(tid)
            .fetch_optional(pool)
            .await
            .expect("check task");
        assert!(still.is_none(), "任务仍应被删除");
        // 资源行仍在、img 仍为 NULL（幂等）
        assert_eq!(get_resource_img(&db, rid).await, None);
    }

    /// Edge Case：同一 remote_id 多条任务——清理某死信时只删当前任务，
    /// 不影响仍在重试中的其他任务；资源 img 以"是否存在死信"为准清空（一次 UPDATE）。
    #[tokio::test]
    async fn test_multiple_tasks_same_remote_id_only_deletes_current_task() {
        let db = setup_db().await;
        let rid = insert_resource(&db, "多任务资源", Some("rid-multi")).await;
        // 任务 A：retry_count=4，将被 mark_task_failed → 5（死信）→ 删除
        let tid_a = insert_task(&db, "rid-multi", "failed", None, None).await;
        set_retry_count_and_ago(&db, tid_a, MAX_RETRIES - 1, 0).await;
        // 任务 B：retry_count=2，独立任务行，不应被触碰
        let tid_b = insert_task(&db, "rid-multi", "failed", None, None).await;
        set_retry_count_and_ago(&db, tid_b, 2, 0).await;

        mark_task_failed(&db, tid_a, "rid-multi", "A 死信")
            .await
            .expect("mark A failed");

        let pool = match &db {
            DbPool::Sqlite(p) => p,
            _ => unreachable!(),
        };
        // A 被删
        let a: Option<(i64,)> = sqlx::query_as("SELECT id FROM forward_tasks WHERE id=?")
            .bind(tid_a)
            .fetch_optional(pool)
            .await
            .expect("check A");
        assert!(a.is_none(), "任务 A（死信）应被删除");
        // B 仍在、retry_count 不变
        let b: Option<(i64, i64)> =
            sqlx::query_as("SELECT id, retry_count FROM forward_tasks WHERE id=?")
                .bind(tid_b)
                .fetch_optional(pool)
                .await
                .expect("check B");
        let (b_id, b_rc) = b.expect("任务 B 应保留");
        assert_eq!(b_id, tid_b);
        assert_eq!(b_rc, 2, "任务 B 的 retry_count 不应被改动");
        // 资源 img 被清空（一次 UPDATE 清掉所有 img='rid-multi' 的资源行）
        assert_eq!(get_resource_img(&db, rid).await, None);
    }
}
