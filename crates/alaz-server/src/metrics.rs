//! System metrics for observability.
//!
//! Lightweight in-process metrics collection. No external dependency required.
//! Exposed via `GET /api/v1/system/metrics`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Global metrics singleton.
#[derive(Debug)]
pub struct Metrics {
    /// Total search requests served.
    pub search_count: AtomicU64,
    /// Total search latency in milliseconds (divide by search_count for avg).
    pub search_latency_ms_total: AtomicU64,
    /// Maximum search latency observed (ms).
    pub search_latency_ms_max: AtomicU64,
    /// Total LLM calls made.
    pub llm_call_count: AtomicU64,
    /// Total LLM errors.
    pub llm_error_count: AtomicU64,
    /// Total embedding requests.
    pub embedding_count: AtomicU64,
    /// Total entities embedded by backfill job.
    pub backfill_processed: AtomicU64,
    /// Total entities pruned by decay job.
    pub decay_pruned: AtomicU64,
    /// Total items consolidated.
    pub consolidation_merged: AtomicU64,
    /// Server start time.
    pub started_at: Instant,
}

impl Metrics {
    /// Create a new metrics instance.
    pub fn new() -> Self {
        Self {
            search_count: AtomicU64::new(0),
            search_latency_ms_total: AtomicU64::new(0),
            search_latency_ms_max: AtomicU64::new(0),
            llm_call_count: AtomicU64::new(0),
            llm_error_count: AtomicU64::new(0),
            embedding_count: AtomicU64::new(0),
            backfill_processed: AtomicU64::new(0),
            decay_pruned: AtomicU64::new(0),
            consolidation_merged: AtomicU64::new(0),
            started_at: Instant::now(),
        }
    }

    /// Record a search request with its latency.
    pub fn record_search(&self, latency_ms: u64) {
        self.search_count.fetch_add(1, Ordering::Relaxed);
        self.search_latency_ms_total
            .fetch_add(latency_ms, Ordering::Relaxed);
        self.search_latency_ms_max
            .fetch_max(latency_ms, Ordering::Relaxed);
    }

    /// Record an LLM call.
    pub fn record_llm_call(&self) {
        self.llm_call_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an LLM error.
    pub fn record_llm_error(&self) {
        self.llm_error_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an embedding request.
    pub fn record_embedding(&self) {
        self.embedding_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot all metrics as a JSON-serializable struct.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let search_count = self.search_count.load(Ordering::Relaxed);
        let latency_total = self.search_latency_ms_total.load(Ordering::Relaxed);
        let avg_latency = if search_count > 0 {
            latency_total / search_count
        } else {
            0
        };

        MetricsSnapshot {
            uptime_seconds: self.started_at.elapsed().as_secs(),
            search_count,
            search_avg_latency_ms: avg_latency,
            search_max_latency_ms: self.search_latency_ms_max.load(Ordering::Relaxed),
            llm_call_count: self.llm_call_count.load(Ordering::Relaxed),
            llm_error_count: self.llm_error_count.load(Ordering::Relaxed),
            embedding_count: self.embedding_count.load(Ordering::Relaxed),
            backfill_processed: self.backfill_processed.load(Ordering::Relaxed),
            decay_pruned: self.decay_pruned.load(Ordering::Relaxed),
            consolidation_merged: self.consolidation_merged.load(Ordering::Relaxed),
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable snapshot of current metrics.
#[derive(Debug, serde::Serialize)]
pub struct MetricsSnapshot {
    pub uptime_seconds: u64,
    pub search_count: u64,
    pub search_avg_latency_ms: u64,
    pub search_max_latency_ms: u64,
    pub llm_call_count: u64,
    pub llm_error_count: u64,
    pub embedding_count: u64,
    pub backfill_processed: u64,
    pub decay_pruned: u64,
    pub consolidation_merged: u64,
}

/// Shared metrics handle for use across the application.
pub type SharedMetrics = Arc<Metrics>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_metrics_are_zero() {
        let m = Metrics::new();
        let snap = m.snapshot();
        assert_eq!(snap.search_count, 0);
        assert_eq!(snap.llm_call_count, 0);
    }

    #[test]
    fn record_search_updates_counters() {
        let m = Metrics::new();
        m.record_search(50);
        m.record_search(100);
        let snap = m.snapshot();
        assert_eq!(snap.search_count, 2);
        assert_eq!(snap.search_avg_latency_ms, 75);
        assert_eq!(snap.search_max_latency_ms, 100);
    }

    #[test]
    fn record_llm_calls() {
        let m = Metrics::new();
        m.record_llm_call();
        m.record_llm_call();
        m.record_llm_error();
        let snap = m.snapshot();
        assert_eq!(snap.llm_call_count, 2);
        assert_eq!(snap.llm_error_count, 1);
    }

    #[test]
    fn record_embedding_updates_counter() {
        let m = Metrics::new();
        m.record_embedding();
        m.record_embedding();
        m.record_embedding();
        let snap = m.snapshot();
        assert_eq!(snap.embedding_count, 3);
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let m = Metrics::new();
        m.record_search(42);
        m.record_llm_call();
        m.record_embedding();
        let snap = m.snapshot();
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["search_count"], 1);
        assert_eq!(json["search_avg_latency_ms"], 42);
        assert_eq!(json["search_max_latency_ms"], 42);
        assert_eq!(json["llm_call_count"], 1);
        assert_eq!(json["embedding_count"], 1);
        assert_eq!(json["backfill_processed"], 0);
        assert_eq!(json["decay_pruned"], 0);
        assert_eq!(json["consolidation_merged"], 0);
        assert!(json["uptime_seconds"].is_number());
    }

    #[test]
    fn backfill_and_decay_counters() {
        use std::sync::atomic::Ordering;
        let m = Metrics::new();
        m.backfill_processed.fetch_add(25, Ordering::Relaxed);
        m.decay_pruned.fetch_add(3, Ordering::Relaxed);
        m.consolidation_merged.fetch_add(7, Ordering::Relaxed);
        let snap = m.snapshot();
        assert_eq!(snap.backfill_processed, 25);
        assert_eq!(snap.decay_pruned, 3);
        assert_eq!(snap.consolidation_merged, 7);
    }

    #[test]
    fn search_max_latency_tracks_highest() {
        let m = Metrics::new();
        m.record_search(10);
        m.record_search(200);
        m.record_search(50);
        let snap = m.snapshot();
        assert_eq!(snap.search_max_latency_ms, 200);
        assert_eq!(snap.search_count, 3);
        // avg = (10 + 200 + 50) / 3 = 86 (integer division)
        assert_eq!(snap.search_avg_latency_ms, 86);
    }
}
