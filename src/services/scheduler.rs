// Task scheduler using tokio::time
// Supports dynamic interval updates and start/stop
// Two schedulers: push scheduler + extract scheduler

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct SchedulerState {
    pub running: bool,
    pub interval_minutes: u64,
    pub last_run_at: Option<std::time::Instant>,
    /// 调度启动时刻 — 用于修正重启后 next_run 计算（last_run_at 重启即丢）
    pub started_at: Option<std::time::Instant>,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
}

pub type SchedulerHandle = Arc<RwLock<SchedulerState>>;

/// Create a new scheduler handle
pub fn create_scheduler() -> SchedulerHandle {
    Arc::new(RwLock::new(SchedulerState {
        running: false,
        interval_minutes: 1,
        last_run_at: None,
        started_at: None,
        handle: None,
        cancel: None,
    }))
}

/// Start the push scheduler — 固定 1 分钟 tick，检查每个配置是否到达其 push_interval
pub async fn start_scheduler(
    scheduler: SchedulerHandle,
    _interval_minutes: u64,
    db: crate::state::DbPool,
    option_cache: crate::state::OptionCache,
) {
    let mut state = scheduler.write().await;
    if state.running {
        return;
    }

    let cancel = CancellationToken::new();
    state.cancel = Some(cancel.clone());
    state.running = true;
    state.interval_minutes = 1;
    state.started_at = Some(std::time::Instant::now());

    let sched = scheduler.clone();
    let handle = tokio::spawn(async move {
        // 固定 1 分钟 tick
        let duration = std::time::Duration::from_secs(60);
        // 记录每个配置的上次推送时间 (config_id -> last_pushed_at)
        let mut config_last_run: std::collections::HashMap<i64, std::time::Instant> =
            std::collections::HashMap::new();

        loop {
            tokio::select! {
                _ = tokio::time::sleep(duration) => {
                    tracing::info!("Push scheduler tick: scanning active push configs");
                    run_push_tick(&db, &option_cache, &mut config_last_run).await;
                    {
                        let mut s = sched.write().await;
                        s.last_run_at = Some(std::time::Instant::now());
                    }
                }
                _ = cancel.cancelled() => {
                    tracing::info!("Push scheduler cancelled");
                    break;
                }
            }
        }
        let mut s = sched.write().await;
        s.running = false;
        s.handle = None;
        s.cancel = None;
    });

    state.handle = Some(handle);
}

/// 推送调度器单次 tick：查询活跃配置，串行推送
async fn run_push_tick(
    db: &crate::state::DbPool,
    option_cache: &crate::state::OptionCache,
    config_last_run: &mut std::collections::HashMap<i64, std::time::Instant>,
) {
    let configs: Vec<crate::models::push_config::PushConfig> = match db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as(
                "SELECT * FROM push_configs WHERE is_active = 1 AND auto_push = 1 ORDER BY id ASC",
            )
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT * FROM push_configs WHERE is_active = TRUE AND auto_push = TRUE ORDER BY id ASC",
            )
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        }
    };

    let now = std::time::Instant::now();
    for config in &configs {
        let last = config_last_run.get(&config.id).copied();
        let interval_secs = (config.push_interval as u64) * 60;
        let should_run = match last {
            Some(t) => now.duration_since(t).as_secs() >= interval_secs,
            None => true, // 从未运行过，立即执行
        };

        if !should_run {
            continue;
        }

        tracing::info!(
            "Push scheduler: executing config '{}' (id={})",
            config.name,
            config.id
        );
        match crate::services::push_config::push_for_config(db, option_cache, config.id, None).await
        {
            Ok(result) => {
                tracing::info!("Push config '{}' result: {:?}", config.name, result);
            }
            Err(e) => {
                tracing::warn!("Push config '{}' failed: {e}", config.name);
            }
        }
        config_last_run.insert(config.id, now);
    }
}

/// Stop the scheduler
pub async fn stop_scheduler(scheduler: SchedulerHandle) {
    let mut state = scheduler.write().await;
    if let Some(cancel) = state.cancel.take() {
        cancel.cancel();
    }
    if let Some(handle) = state.handle.take() {
        handle.abort();
    }
    state.running = false;
}

/// Update push scheduler — restarts the scheduler
/// interval_minutes 参数保留以兼容旧调用，实际使用固定 1 分钟 tick
#[allow(clippy::too_many_arguments)]
pub async fn update_scheduler(
    scheduler: SchedulerHandle,
    minutes: u64,
    _api_url: String,
    _api_token: String,
    _target: String,
    _batch_size: i64,
    db: crate::state::DbPool,
    option_cache: crate::state::OptionCache,
) {
    if minutes > 0 {
        stop_scheduler(scheduler.clone()).await;
        start_scheduler(scheduler, minutes, db, option_cache).await;
    } else {
        stop_scheduler(scheduler).await;
    }
}

// ─── 提取调度器 ──────────────────────────────────────────────────────────────

/// 提取调度器状态（独立于推送调度器）
#[derive(Debug)]
pub struct ExtractSchedulerState {
    pub running: bool,
    pub interval_minutes: u64,
    pub last_run_at: Option<std::time::Instant>,
    /// 调度启动时刻 — 用于修正重启后 next_run 计算（last_run_at 重启即丢）
    pub started_at: Option<std::time::Instant>,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
}

pub type ExtractSchedulerHandle = Arc<RwLock<ExtractSchedulerState>>;

/// 创建提取调度器
pub fn create_extract_scheduler() -> ExtractSchedulerHandle {
    Arc::new(RwLock::new(ExtractSchedulerState {
        running: false,
        interval_minutes: 30,
        last_run_at: None,
        started_at: None,
        handle: None,
        cancel: None,
    }))
}

/// 启动提取调度器
pub async fn start_extract_scheduler(
    scheduler: ExtractSchedulerHandle,
    interval_minutes: u64,
    app_state: crate::state::AppState,
) {
    let mut state = scheduler.write().await;
    if state.running {
        return;
    }

    let cancel = CancellationToken::new();
    state.cancel = Some(cancel.clone());
    state.running = true;
    state.interval_minutes = interval_minutes;
    state.started_at = Some(std::time::Instant::now());

    let sched = scheduler.clone();
    let handle = tokio::spawn(async move {
        let duration = std::time::Duration::from_secs(interval_minutes * 60);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(duration) => {
                    run_extract_tick(&app_state, &sched).await;
                }
                _ = cancel.cancelled() => {
                    tracing::info!("Extract scheduler cancelled");
                    break;
                }
            }
        }
        let mut s = sched.write().await;
        s.running = false;
        s.handle = None;
        s.cancel = None;
    });

    state.handle = Some(handle);
}

/// 停止提取调度器
pub async fn stop_extract_scheduler(scheduler: ExtractSchedulerHandle) {
    let mut state = scheduler.write().await;
    if let Some(cancel) = state.cancel.take() {
        cancel.cancel();
    }
    if let Some(handle) = state.handle.take() {
        handle.abort();
    }
    state.running = false;
}

/// 更新提取调度器（重启）
pub async fn update_extract_scheduler(
    scheduler: ExtractSchedulerHandle,
    minutes: u64,
    app_state: crate::state::AppState,
) {
    stop_extract_scheduler(scheduler.clone()).await;
    if minutes > 0 {
        start_extract_scheduler(scheduler, minutes, app_state).await;
    }
}

/// 提取调度器单次 tick：执行提取 + 写入历史 + 更新 last_run_at
async fn run_extract_tick(app_state: &crate::state::AppState, sched: &ExtractSchedulerHandle) {
    tracing::info!("Extract scheduler tick: triggering extraction");
    let result = crate::services::resource::trigger_extraction(app_state, 1000).await;

    let (status, scanned, extracted, skipped, errors, msg) = match &result {
        Ok(r) => (
            "success",
            r.total_scanned,
            r.extracted,
            r.skipped,
            r.errors,
            None,
        ),
        Err(e) => ("failed", 0i64, 0i64, 0i64, 0i64, Some(e.to_string())),
    };
    tracing::info!(
        "Extract tick result: status={status}, scanned={scanned}, extracted={extracted}, skipped={skipped}, errors={errors}"
    );
    match crate::services::extract_history::insert(
        &app_state.db,
        status,
        scanned,
        extracted,
        skipped,
        errors,
        msg.as_deref(),
    )
    .await
    {
        Ok(()) => tracing::info!("Extract history record inserted successfully"),
        Err(e) => tracing::error!("写入提取历史失败: {e}"),
    }

    {
        let mut s = sched.write().await;
        s.last_run_at = Some(std::time::Instant::now());
    }

    match result {
        Ok(r) => tracing::info!(
            "Scheduled extraction result: scanned={}, extracted={}, skipped={}",
            r.total_scanned,
            r.extracted,
            r.skipped
        ),
        Err(e) => tracing::warn!("Scheduled extraction failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_scheduler_default() {
        let scheduler = create_scheduler();
        let state = scheduler.blocking_read();
        assert!(!state.running);
        assert_eq!(state.interval_minutes, 1); // 固定 1 分钟 tick
        assert!(state.handle.is_none());
        assert!(state.cancel.is_none());
    }

    #[tokio::test]
    async fn test_stop_scheduler_when_not_running() {
        let scheduler = create_scheduler();
        stop_scheduler(scheduler.clone()).await;
        let state = scheduler.read().await;
        assert!(!state.running);
    }

    #[test]
    fn test_create_extract_scheduler_default() {
        let scheduler = create_extract_scheduler();
        let state = scheduler.blocking_read();
        assert!(!state.running);
        assert_eq!(state.interval_minutes, 30);
        assert!(state.handle.is_none());
        assert!(state.cancel.is_none());
    }

    #[tokio::test]
    async fn test_stop_extract_scheduler_when_not_running() {
        let scheduler = create_extract_scheduler();
        stop_extract_scheduler(scheduler.clone()).await;
        let state = scheduler.read().await;
        assert!(!state.running);
    }
}
