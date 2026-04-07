use std::net::IpAddr;
use std::time::Instant;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use dashmap::DashMap;

/// Simple in-memory token bucket rate limiter.
///
/// Tracks requests per IP address. Each IP gets `max_requests` tokens
/// that refill at a rate of `max_requests` per `window_secs` seconds.
pub struct RateLimiter {
    buckets: DashMap<IpAddr, Bucket>,
    max_requests: u32,
    window_secs: u64,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            buckets: DashMap::new(),
            max_requests,
            window_secs,
        }
    }

    /// Try to consume a token for the given IP. Returns true if allowed.
    pub fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let max = self.max_requests as f64;
        let refill_rate = max / self.window_secs as f64;

        let mut bucket = self.buckets.entry(ip).or_insert(Bucket {
            tokens: max,
            last_refill: now,
        });

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_rate).min(max);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Periodically clean up stale entries (IPs not seen for 2x window).
    pub fn cleanup(&self) {
        let now = Instant::now();
        let stale_threshold = self.window_secs * 2;
        self.buckets
            .retain(|_, b| now.duration_since(b.last_refill).as_secs() < stale_threshold);
    }
}

/// Axum middleware that enforces rate limiting per client IP.
///
/// IP resolution order:
/// 1. `CF-Connecting-IP` header (Cloudflare Tunnel real client IP)
/// 2. `X-Forwarded-For` header (first IP in chain)
/// 3. Fallback to 0.0.0.0 (counts as single bucket for direct connections)
pub async fn rate_limit_middleware(request: Request, next: Next) -> Response {
    let ip = request
        .headers()
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            request
                .headers()
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.trim().parse().ok())
        })
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));

    let limiter = request.extensions().get::<std::sync::Arc<RateLimiter>>();

    if let Some(limiter) = limiter
        && !limiter.check(ip)
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded — try again later",
        )
            .into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::thread;
    use std::time::Duration;

    fn ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn allows_requests_up_to_max() {
        let limiter = RateLimiter::new(3, 60);
        let addr = ip(1, 2, 3, 4);
        assert!(limiter.check(addr));
        assert!(limiter.check(addr));
        assert!(limiter.check(addr));
    }

    #[test]
    fn rejects_beyond_max() {
        let limiter = RateLimiter::new(2, 60);
        let addr = ip(10, 0, 0, 1);
        assert!(limiter.check(addr));
        assert!(limiter.check(addr));
        assert!(!limiter.check(addr)); // 3rd should fail
    }

    #[test]
    fn tokens_refill_over_time() {
        // 10 requests per 1 second → refill rate = 10/s
        let limiter = RateLimiter::new(10, 1);
        let addr = ip(10, 0, 0, 2);

        // Drain all tokens
        for _ in 0..10 {
            assert!(limiter.check(addr));
        }
        assert!(!limiter.check(addr));

        // Wait for refill (150ms should give ~1.5 tokens at 10/s rate)
        thread::sleep(Duration::from_millis(150));
        assert!(limiter.check(addr));
    }

    #[test]
    fn cleanup_removes_stale_entries() {
        // window = 1s, stale threshold = 2s
        let limiter = RateLimiter::new(5, 1);
        let addr = ip(10, 0, 0, 3);

        // Touch the bucket
        limiter.check(addr);

        // Immediately: cleanup should keep it
        limiter.cleanup();
        assert_eq!(limiter.buckets.len(), 1);

        // Wait past stale threshold (2x window = 2s)
        thread::sleep(Duration::from_millis(2100));
        limiter.cleanup();
        assert_eq!(limiter.buckets.len(), 0);
    }

    #[test]
    fn independent_buckets_per_ip() {
        let limiter = RateLimiter::new(1, 60);
        let addr_a = ip(192, 168, 0, 1);
        let addr_b = ip(192, 168, 0, 2);

        assert!(limiter.check(addr_a));
        assert!(!limiter.check(addr_a)); // exhausted

        // addr_b should still have its own bucket
        assert!(limiter.check(addr_b));
        assert!(!limiter.check(addr_b));
    }

    #[test]
    fn rapid_requests_drain_tokens() {
        let limiter = RateLimiter::new(5, 60);
        let addr = ip(10, 0, 0, 4);

        let mut allowed = 0;
        for _ in 0..10 {
            if limiter.check(addr) {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 5);
    }

    #[test]
    fn requests_allowed_after_cooldown() {
        // 5 requests per 1 second
        let limiter = RateLimiter::new(5, 1);
        let addr = ip(10, 0, 0, 5);

        // Exhaust
        for _ in 0..5 {
            assert!(limiter.check(addr));
        }
        assert!(!limiter.check(addr));

        // Cool down long enough for full refill
        thread::sleep(Duration::from_millis(1100));
        // Should have ~5 tokens again
        assert!(limiter.check(addr));
        assert!(limiter.check(addr));
    }
}
