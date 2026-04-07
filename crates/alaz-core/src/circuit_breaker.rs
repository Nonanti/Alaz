use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Simple circuit breaker: after `threshold` consecutive failures,
/// enter open state for `backoff_secs` seconds. All calls during
/// open state return Err immediately without attempting the real call.
pub struct CircuitBreaker {
    name: String,
    threshold: u32,
    backoff_secs: u64,
    consecutive_failures: AtomicU32,
    open_until: AtomicU64, // epoch seconds
}

impl CircuitBreaker {
    pub fn new(name: &str, threshold: u32, backoff_secs: u64) -> Self {
        Self {
            name: name.to_string(),
            threshold,
            backoff_secs,
            consecutive_failures: AtomicU32::new(0),
            open_until: AtomicU64::new(0),
        }
    }

    /// Check if the circuit is open (should skip the call).
    pub fn is_open(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let until = self.open_until.load(Ordering::Relaxed);
        now < until
    }

    /// Record a successful call — resets failure counter.
    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    /// Record a failed call — increments counter, opens circuit if threshold reached.
    pub fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if failures >= self.threshold {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            self.open_until
                .store(now + self.backoff_secs, Ordering::Relaxed);
            tracing::warn!(
                service = %self.name,
                failures,
                backoff_secs = self.backoff_secs,
                "circuit breaker opened"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starts_closed() {
        let cb = CircuitBreaker::new("test", 3, 60);
        assert!(!cb.is_open());
    }

    #[test]
    fn test_opens_after_threshold() {
        let cb = CircuitBreaker::new("test", 3, 60);
        cb.record_failure();
        cb.record_failure();
        assert!(!cb.is_open());
        cb.record_failure(); // 3rd failure -> opens
        assert!(cb.is_open());
    }

    #[test]
    fn test_success_resets_counter() {
        let cb = CircuitBreaker::new("test", 3, 60);
        cb.record_failure();
        cb.record_failure();
        cb.record_success(); // reset
        cb.record_failure();
        cb.record_failure();
        assert!(!cb.is_open()); // only 2 consecutive failures
    }
}
