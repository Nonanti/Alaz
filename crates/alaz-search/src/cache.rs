//! Lightweight in-memory LRU cache with TTL for search results.
//!
//! Prevents duplicate pipeline executions when the same query arrives
//! within a short window (e.g., pi extension proactive context + manual search).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use alaz_core::traits::SearchResult;
use tokio::sync::Mutex;

struct CacheEntry {
    results: Vec<SearchResult>,
    created_at: Instant,
}

/// Thread-safe search result cache with TTL and max capacity.
pub struct SearchCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    ttl: Duration,
    max_entries: usize,
}

impl SearchCache {
    /// Create a new cache with given TTL (in seconds) and max entry count.
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    /// Build a deterministic cache key from query parameters.
    fn cache_key(query: &str, project: Option<&str>, rerank: bool, hyde: bool) -> String {
        format!(
            "{}:{}:{}:{}",
            query.to_lowercase().trim(),
            project.unwrap_or(""),
            rerank,
            hyde
        )
    }

    /// Get cached results if they exist and haven't expired.
    pub async fn get(
        &self,
        query: &str,
        project: Option<&str>,
        rerank: bool,
        hyde: bool,
    ) -> Option<Vec<SearchResult>> {
        let key = Self::cache_key(query, project, rerank, hyde);
        let mut entries = self.entries.lock().await;

        if let Some(entry) = entries.get(&key) {
            if entry.created_at.elapsed() < self.ttl {
                return Some(entry.results.clone());
            }
            // Expired — remove it
            entries.remove(&key);
        }

        None
    }

    /// Store results in cache. Evicts the oldest entry when at capacity.
    pub async fn put(
        &self,
        query: &str,
        project: Option<&str>,
        rerank: bool,
        hyde: bool,
        results: Vec<SearchResult>,
    ) {
        let key = Self::cache_key(query, project, rerank, hyde);
        let mut entries = self.entries.lock().await;

        // Evict oldest entry if at capacity (and we're inserting a new key)
        if entries.len() >= self.max_entries
            && !entries.contains_key(&key)
            && let Some(oldest_key) = entries
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
        {
            entries.remove(&oldest_key);
        }

        entries.insert(
            key,
            CacheEntry {
                results,
                created_at: Instant::now(),
            },
        );
    }

    /// Remove all expired entries.
    pub async fn cleanup(&self) {
        let mut entries = self.entries.lock().await;
        let ttl = self.ttl;
        entries.retain(|_, v| v.created_at.elapsed() < ttl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(id: &str, title: &str) -> SearchResult {
        SearchResult {
            entity_type: "knowledge_item".to_string(),
            entity_id: id.to_string(),
            title: title.to_string(),
            content: "test content".to_string(),
            score: 1.0,
            project: None,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn cache_miss_returns_none() {
        let cache = SearchCache::new(60, 100);
        let result = cache.get("nonexistent query", None, true, false).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn cache_hit_returns_results() {
        let cache = SearchCache::new(60, 100);
        let results = vec![
            make_result("id1", "Result 1"),
            make_result("id2", "Result 2"),
        ];

        cache
            .put(
                "test query",
                Some("myproject"),
                true,
                false,
                results.clone(),
            )
            .await;

        let cached = cache
            .get("test query", Some("myproject"), true, false)
            .await;

        assert!(cached.is_some());
        let cached = cached.unwrap();
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].entity_id, "id1");
        assert_eq!(cached[1].entity_id, "id2");
    }

    #[tokio::test]
    async fn ttl_expiry_evicts_entry() {
        let cache = SearchCache::new(0, 100); // 0-second TTL = instant expiry
        let results = vec![make_result("id1", "Result 1")];

        cache.put("query", None, true, false, results).await;

        // Even a tiny sleep ensures the 0s TTL has elapsed
        tokio::time::sleep(Duration::from_millis(10)).await;

        let cached = cache.get("query", None, true, false).await;
        assert!(cached.is_none(), "expired entry should not be returned");
    }

    #[tokio::test]
    async fn max_entries_evicts_oldest() {
        let cache = SearchCache::new(60, 2); // Only 2 slots

        cache
            .put("query1", None, true, false, vec![make_result("a", "A")])
            .await;

        // Small delay so query2 is definitively newer
        tokio::time::sleep(Duration::from_millis(5)).await;

        cache
            .put("query2", None, true, false, vec![make_result("b", "B")])
            .await;

        // This should evict query1 (oldest)
        cache
            .put("query3", None, true, false, vec![make_result("c", "C")])
            .await;

        assert!(
            cache.get("query1", None, true, false).await.is_none(),
            "oldest entry should have been evicted"
        );
        assert!(cache.get("query2", None, true, false).await.is_some());
        assert!(cache.get("query3", None, true, false).await.is_some());
    }

    #[tokio::test]
    async fn different_queries_dont_collide() {
        let cache = SearchCache::new(60, 100);

        let results_a = vec![make_result("a", "Result A")];
        let results_b = vec![make_result("b", "Result B")];

        cache.put("query alpha", None, true, false, results_a).await;
        cache.put("query beta", None, true, false, results_b).await;

        let cached_a = cache.get("query alpha", None, true, false).await.unwrap();
        let cached_b = cache.get("query beta", None, true, false).await.unwrap();

        assert_eq!(cached_a[0].entity_id, "a");
        assert_eq!(cached_b[0].entity_id, "b");
    }

    #[tokio::test]
    async fn same_query_different_options_dont_collide() {
        let cache = SearchCache::new(60, 100);

        let results_rerank = vec![make_result("r", "Reranked")];
        let results_no_rerank = vec![make_result("n", "Not reranked")];

        cache.put("query", None, true, false, results_rerank).await;
        cache
            .put("query", None, false, false, results_no_rerank)
            .await;

        let cached_rerank = cache.get("query", None, true, false).await.unwrap();
        let cached_no_rerank = cache.get("query", None, false, false).await.unwrap();

        assert_eq!(cached_rerank[0].entity_id, "r");
        assert_eq!(cached_no_rerank[0].entity_id, "n");
    }

    #[tokio::test]
    async fn cleanup_removes_expired_entries() {
        let cache = SearchCache::new(0, 100); // instant expiry

        cache
            .put("q1", None, true, false, vec![make_result("1", "One")])
            .await;
        cache
            .put("q2", None, true, false, vec![make_result("2", "Two")])
            .await;

        tokio::time::sleep(Duration::from_millis(10)).await;

        cache.cleanup().await;

        let entries = cache.entries.lock().await;
        assert!(
            entries.is_empty(),
            "cleanup should remove all expired entries"
        );
    }
}
