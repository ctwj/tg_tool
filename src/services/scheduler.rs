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
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
    pub api_url: String,
    pub api_token: String,
    pub target: String,
    pub batch_size: i64,
}

pub type SchedulerHandle = Arc<RwLock<SchedulerState>>;

/// Create a new scheduler handle
pub fn create_scheduler() -> SchedulerHandle {
    Arc::new(RwLock::new(SchedulerState {
        running: false,
        interval_minutes: 30,
        handle: None,
        cancel: None,
        api_url: String::new(),
        api_token: String::new(),
        target: "external_api".to_string(),
        batch_size: 1000,
    }))
}

/// Start the push scheduler with a given interval
pub async fn start_scheduler(
    scheduler: SchedulerHandle,
    interval_minutes: u64,
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
    state.interval_minutes = interval_minutes;

    let api_url = state.api_url.clone();
    let api_token = state.api_token.clone();
    let target = state.target.clone();
    let batch_size = state.batch_size;

    let sched = scheduler.clone();
    let handle = tokio::spawn(async move {
        let duration = std::time::Duration::from_secs(interval_minutes * 60);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(duration) => {
                    tracing::info!("Scheduler tick: triggering push");
                    let result = crate::services::push::trigger_push(
                        &api_url,
                        &api_token,
                        &target,
                        batch_size,
                        &db,
                        &option_cache,
                    )
                    .await;
                    match result {
                        Ok(v) => tracing::info!("Scheduled push result: {:?}", v),
                        Err(e) => tracing::warn!("Scheduled push failed: {e}"),
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

/// Update push scheduler interval (restarts the scheduler)
#[allow(clippy::too_many_arguments)]
pub async fn update_scheduler(
    scheduler: SchedulerHandle,
    minutes: u64,
    api_url: String,
    api_token: String,
    target: String,
    batch_size: i64,
    db: crate::state::DbPool,
    option_cache: crate::state::OptionCache,
) {
    {
        let mut state = scheduler.write().await;
        state.api_url = api_url;
        state.api_token = api_token;
        state.target = target;
        state.batch_size = batch_size;
    }

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
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
}

pub type ExtractSchedulerHandle = Arc<RwLock<ExtractSchedulerState>>;

/// 创建提取调度器
pub fn create_extract_scheduler() -> ExtractSchedulerHandle {
    Arc::new(RwLock::new(ExtractSchedulerState {
        running: false,
        interval_minutes: 30,
        handle: None,
        cancel: None,
    }))
}

/// 启动提取调度器
pub async fn start_extract_scheduler(
    scheduler: ExtractSchedulerHandle,
    interval_minutes: u64,
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
    state.interval_minutes = interval_minutes;

    let sched = scheduler.clone();
    let handle = tokio::spawn(async move {
        let duration = std::time::Duration::from_secs(interval_minutes * 60);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(duration) => {
                    tracing::info!("Extract scheduler tick: triggering extraction");
                    let result = crate::services::resource::trigger_extraction(
                        &db,
                        &option_cache,
                        1000,
                    )
                    .await;
                    match result {
                        Ok(r) => tracing::info!(
                            "Scheduled extraction result: scanned={}, extracted={}, skipped={}",
                            r.total_scanned, r.extracted, r.skipped
                        ),
                        Err(e) => tracing::warn!("Scheduled extraction failed: {e}"),
                    }
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
    db: crate::state::DbPool,
    option_cache: crate::state::OptionCache,
) {
    stop_extract_scheduler(scheduler.clone()).await;
    if minutes > 0 {
        start_extract_scheduler(scheduler, minutes, db, option_cache).await;
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
        assert_eq!(state.interval_minutes, 30);
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
