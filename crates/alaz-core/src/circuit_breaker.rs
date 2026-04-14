use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Circuit breaker with half-open state for resilient external service calls.
///
/// States:
/// - **Closed**: Normal operation. Failures counted.
/// - **Open**: After `threshold` consecutive failures, block all calls for `backoff_secs`.
/// - **Half-open**: After backoff expires, allow ONE probe call through.
///   If it succeeds → Closed. If it fails → Open again.
pub struct CircuitBreaker {
    name: String,
    threshold: u32,
    backoff_secs: u64,
    consecutive_failures: AtomicU32,
    open_until: AtomicU64,       // epoch seconds
    half_open_probe: AtomicBool, // true = one probe in progress
}

impl CircuitBreaker {
    pub fn new(name: &str, threshold: u32, backoff_secs: u64) -> Self {
        Self {
            name: name.to_string(),
            threshold,
            backoff_secs,
            consecutive_failures: AtomicU32::new(0),
            open_until: AtomicU64::new(0),
            half_open_probe: AtomicBool::new(false),
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Check if the circuit is open (should skip the call).
    ///
    /// Returns `false` (allow call) in these cases:
    /// - Circuit is closed (no failures or below threshold)
    /// - Circuit was open but backoff expired → enters half-open, allows one probe
    ///
    /// Returns `true` (block call) when:
    /// - Circuit is open and backoff hasn't expired
    /// - Circuit is half-open and another probe is already in progress
    pub fn is_open(&self) -> bool {
        let until = self.open_until.load(Ordering::SeqCst);
        if until == 0 {
            return false; // Never opened
        }

        let now = Self::now_secs();
        if now >= until {
            // Backoff expired → try half-open probe
            // compare_exchange: only one thread gets to probe
            if self
                .half_open_probe
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                tracing::debug!(service = %self.name, "circuit breaker half-open: allowing probe");
                return false; // Allow this one call through
            }
            // Another thread is already probing — block
            return true;
        }

        true // Still in backoff window
    }

    /// Record a successful call — resets to closed state.
    pub fn record_success(&self) {
        let was_probing = self.half_open_probe.swap(false, Ordering::SeqCst);
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.open_until.store(0, Ordering::SeqCst);
        if was_probing {
            tracing::info!(service = %self.name, "circuit breaker closed (probe succeeded)");
        }
    }

    /// Record a failed call — increments counter, opens circuit if threshold reached.
    pub fn record_failure(&self) {
        let was_probing = self.half_open_probe.swap(false, Ordering::SeqCst);

        if was_probing {
            // Half-open probe failed → re-open with fresh backoff
            let now = Self::now_secs();
            self.open_until
                .store(now + self.backoff_secs, Ordering::SeqCst);
            tracing::warn!(
                service = %self.name,
                backoff_secs = self.backoff_secs,
                "circuit breaker re-opened (probe failed)"
            );
            return;
        }

        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if failures >= self.threshold {
            let now = Self::now_secs();
            self.open_until
                .store(now + self.backoff_secs, Ordering::SeqCst);
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

    #[test]
    fn test_half_open_allows_one_probe() {
        let cb = CircuitBreaker::new("test", 2, 0); // 0s backoff = immediate half-open
        cb.record_failure();
        cb.record_failure(); // Opens
        // Backoff is 0s so immediately half-open
        assert!(!cb.is_open()); // First call: probe allowed
        assert!(cb.is_open()); // Second call: blocked (probe in progress)
    }

    #[test]
    fn test_half_open_probe_success_closes() {
        let cb = CircuitBreaker::new("test", 2, 0);
        cb.record_failure();
        cb.record_failure(); // Opens
        assert!(!cb.is_open()); // Probe allowed
        cb.record_success(); // Probe succeeded → closed
        assert!(!cb.is_open()); // Should be closed now
    }

    #[test]
    fn test_half_open_probe_failure_reopens() {
        let cb = CircuitBreaker::new("test", 2, 0);
        cb.record_failure();
        cb.record_failure(); // Opens
        assert!(!cb.is_open()); // Probe allowed
        cb.record_failure(); // Probe failed → re-opens
        // New probe attempt
        assert!(!cb.is_open()); // Half-open again (0s backoff)
    }
}
