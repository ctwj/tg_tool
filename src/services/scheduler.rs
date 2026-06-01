// Task scheduler using tokio::time
// Supports dynamic interval updates and start/stop

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct SchedulerState {
    pub running: bool,
    pub interval_minutes: u64,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
}

pub type SchedulerHandle = Arc<RwLock<SchedulerState>>;

/// Create a new scheduler handle
pub fn create_scheduler() -> SchedulerHandle {
    Arc::new(RwLock::new(SchedulerState {
        running: false,
        interval_minutes: 30,
        handle: None,
        cancel: None,
    }))
}

/// Start the scheduler with a given interval
pub async fn start_scheduler(
    scheduler: SchedulerHandle,
    _interval_minutes: u64,
) {
    let mut state = scheduler.write().await;
    if state.running {
        return;
    }
    // TODO: Implement actual scheduler loop
    // 1. Create CancellationToken
    // 2. Spawn tokio task with interval loop
    // 3. On each tick, call push::trigger_push()
    // 4. Check cancel token on each iteration
    state.running = true;
    state.interval_minutes = _interval_minutes;
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

/// Update scheduler interval
pub async fn update_interval(scheduler: SchedulerHandle, minutes: u64) {
    stop_scheduler(scheduler.clone()).await;
    start_scheduler(scheduler.clone(), minutes).await;
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
    async fn test_start_scheduler() {
        let scheduler = create_scheduler();
        start_scheduler(scheduler.clone(), 15).await;

        let state = scheduler.read().await;
        assert!(state.running);
        assert_eq!(state.interval_minutes, 15);
    }

    #[tokio::test]
    async fn test_start_scheduler_idempotent() {
        let scheduler = create_scheduler();
        start_scheduler(scheduler.clone(), 10).await;
        start_scheduler(scheduler.clone(), 20).await; // should not update interval if already running

        let state = scheduler.read().await;
        assert!(state.running);
        assert_eq!(state.interval_minutes, 10); // keeps original
    }

    #[tokio::test]
    async fn test_stop_scheduler() {
        let scheduler = create_scheduler();
        start_scheduler(scheduler.clone(), 5).await;
        stop_scheduler(scheduler.clone()).await;

        let state = scheduler.read().await;
        assert!(!state.running);
        assert!(state.handle.is_none());
    }

    #[tokio::test]
    async fn test_stop_scheduler_when_not_running() {
        let scheduler = create_scheduler();
        // Should not panic
        stop_scheduler(scheduler.clone()).await;
        let state = scheduler.read().await;
        assert!(!state.running);
    }

    #[tokio::test]
    async fn test_update_interval() {
        let scheduler = create_scheduler();
        update_interval(scheduler.clone(), 45).await;

        let state = scheduler.read().await;
        assert!(state.running);
        assert_eq!(state.interval_minutes, 45);
    }
}
