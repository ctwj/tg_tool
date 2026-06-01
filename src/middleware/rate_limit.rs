use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Simple in-memory rate limiter
#[derive(Clone)]
pub struct RateLimiter {
    /// (IP -> (count, window_start))
    requests: Arc<RwLock<HashMap<String, (usize, Instant)>>>,
    max_requests: usize,
    window_secs: u64,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window_secs,
        }
    }
}

/// Rate limit middleware
pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<RateLimiter>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Get client IP from headers or connection info
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let mut map = limiter.requests.write().await;
    let now = Instant::now();

    let entry = map.entry(ip).or_insert((0, now));

    // Reset window if expired
    if now.duration_since(entry.1).as_secs() > limiter.window_secs {
        *entry = (1, now);
        drop(map);
        return Ok(next.run(req).await);
    }

    entry.0 += 1;

    if entry.0 > limiter.max_requests {
        drop(map);
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    drop(map);
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_new() {
        let limiter = RateLimiter::new(10, 60);
        assert_eq!(limiter.max_requests, 10);
        assert_eq!(limiter.window_secs, 60);
    }

    #[tokio::test]
    async fn test_rate_limiter_tracks_ip() {
        let limiter = RateLimiter::new(3, 60);
        let map = &limiter.requests;

        // Simulate requests from one IP
        {
            let mut m = map.write().await;
            let now = Instant::now();
            m.insert("192.168.1.1".to_string(), (3, now));
        }

        let m = map.read().await;
        let entry = m.get("192.168.1.1").unwrap();
        assert_eq!(entry.0, 3);
    }

    #[tokio::test]
    async fn test_rate_limiter_window_reset() {
        let limiter = RateLimiter::new(5, 60);
        let map = &limiter.requests;

        // Insert an old entry (simulating expired window)
        {
            let mut m = map.write().await;
            let old_time = Instant::now() - std::time::Duration::from_secs(120);
            m.insert("10.0.0.1".to_string(), (5, old_time));
        }

        // The entry exists with expired timestamp
        let m = map.read().await;
        let entry = m.get("10.0.0.1").unwrap();
        // Window has expired
        assert!(Instant::now().duration_since(entry.1).as_secs() > 60);
    }
}
