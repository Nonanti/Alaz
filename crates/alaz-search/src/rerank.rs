//! Reranking module supporting cross-encoder (TEI) and LLM-based reranking.
//!
//! Both methods degrade gracefully: if the TEI service is unavailable,
//! equal scores are returned. LLM reranking asks the model to rate each
//! document on a 0-10 scale with an explanation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alaz_core::traits::SearchResult;
use alaz_core::{AlazError, CircuitBreaker, Result};
use alaz_intel::LlmClient;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Configuration for the 3-stage reranking pipeline.
#[derive(Debug, Clone)]
pub struct RerankConfig {
    /// Max results after cross-encoder stage
    pub stage2_top_k: usize,
    /// Min cross-encoder score to keep
    pub stage2_min_score: f64,
    /// Max results after LLM stage
    pub stage3_top_k: usize,
    /// Weight for bi-encoder (stage 1) score
    pub w_bi: f64,
    /// Weight for cross-encoder (stage 2) score
    pub w_cross: f64,
    /// Weight for LLM (stage 3) score
    pub w_llm: f64,
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            stage2_top_k: 20,
            stage2_min_score: 0.3,
            stage3_top_k: 5,
            w_bi: 0.2,
            w_cross: 0.5,
            w_llm: 0.3,
        }
    }
}

struct CacheEntry {
    scores: Vec<f64>,
    created_at: Instant,
}

pub struct RerankCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    ttl: Duration,
    max_entries: usize,
}

impl Default for RerankCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RerankCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(600), // 10 minutes
            max_entries: 500,
        }
    }

    async fn get(&self, key: &str) -> Option<Vec<f64>> {
        let entries = self.entries.lock().await;
        if let Some(entry) = entries.get(key)
            && entry.created_at.elapsed() < self.ttl
        {
            return Some(entry.scores.clone());
        }
        None
    }

    async fn set(&self, key: String, scores: Vec<f64>) {
        let mut entries = self.entries.lock().await;
        // Evict oldest if at capacity
        if entries.len() >= self.max_entries {
            let oldest = entries
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                entries.remove(&k);
            }
        }
        entries.insert(
            key,
            CacheEntry {
                scores,
                created_at: Instant::now(),
            },
        );
    }
}

/// Reranker supporting both cross-encoder and LLM-based approaches.
pub struct Reranker {
    tei_url: String,
    llm: Option<Arc<LlmClient>>,
    client: reqwest::Client,
    breaker: CircuitBreaker,
    cache: RerankCache,
}

#[derive(Serialize)]
struct RerankRequest<'a> {
    query: &'a str,
    texts: Vec<&'a str>,
}

#[derive(Deserialize)]
struct RerankResponseItem {
    #[allow(dead_code)]
    index: usize,
    score: f64,
}

impl Reranker {
    /// Create a new Reranker.
    ///
    /// - `tei_url`: Base URL for the TEI cross-encoder service
    /// - `llm`: Optional LLM client for LLM-based reranking
    pub fn new(tei_url: &str, llm: Option<Arc<LlmClient>>) -> Self {
        Self {
            tei_url: tei_url.trim_end_matches('/').to_string(),
            llm,
            client: reqwest::Client::new(),
            breaker: CircuitBreaker::new("tei-reranker", 5, 60),
            cache: RerankCache::new(),
        }
    }

    /// Rerank documents using a cross-encoder via TEI.
    ///
    /// Sends a POST to `{tei_url}/rerank` with the query and document texts.
    /// Returns a vector of relevance scores (one per document).
    ///
    /// If TEI is unavailable, returns equal scores for graceful degradation.
    pub async fn rerank_cross_encoder(
        &self,
        query: &str,
        docs: &[(String, String)],
    ) -> Result<Vec<f64>> {
        if docs.is_empty() {
            return Ok(vec![]);
        }

        if self.breaker.is_open() {
            return Ok(vec![1.0; docs.len()]); // Graceful degradation
        }

        let texts: Vec<&str> = docs.iter().map(|(_, content)| content.as_str()).collect();
        let request = RerankRequest { query, texts };

        let url = format!("{}/rerank", self.tei_url);

        let response = match self.client.post(&url).json(&request).send().await {
            Ok(resp) => resp,
            Err(e) => {
                self.breaker.record_failure();
                warn!(
                    error = %e,
                    "TEI reranker unavailable, returning equal scores"
                );
                return Ok(vec![1.0; docs.len()]);
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read body".to_string());
            self.breaker.record_failure();
            warn!(
                status = %status,
                body = %body,
                "TEI reranker returned error, returning equal scores"
            );
            return Ok(vec![1.0; docs.len()]);
        }

        let items: Vec<RerankResponseItem> = response.json().await.map_err(|e| {
            AlazError::Reranker(format!("failed to parse TEI rerank response: {e}"))
        })?;

        self.breaker.record_success();

        // Build scores array ordered by original index
        let mut scores = vec![0.0; docs.len()];
        for item in items {
            if item.index < scores.len() {
                scores[item.index] = item.score;
            }
        }

        debug!(
            query = %query,
            num_docs = docs.len(),
            "cross-encoder reranking complete"
        );

        Ok(scores)
    }

    /// Rerank documents using LLM-based scoring.
    ///
    /// Asks the LLM to rate each document 0-10 for relevance with an explanation.
    /// Returns `(normalized_score, explanation)` pairs.
    pub async fn rerank_llm(
        &self,
        query: &str,
        docs: &[(String, String)],
    ) -> Result<Vec<(f64, String)>> {
        let llm = self.llm.as_ref().ok_or_else(|| {
            AlazError::Reranker("LLM client not configured for reranking".to_string())
        })?;

        if docs.is_empty() {
            return Ok(vec![]);
        }

        let system_prompt = r#"You are a search relevance judge. For each document, rate its relevance to the query on a scale of 0-10. Return a JSON array of objects with "score" (integer 0-10) and "explanation" (brief string).

Return ONLY the JSON array, no other text."#;

        let mut doc_list = String::new();
        for (i, (title, content)) in docs.iter().enumerate() {
            doc_list.push_str(&format!(
                "\n--- Document {} ---\nTitle: {}\nContent: {}\n",
                i + 1,
                title,
                {
                    let end = content.len().min(500);
                    let end = if end < content.len() {
                        // Find safe UTF-8 boundary
                        let mut b = end;
                        while b > 0 && !content.is_char_boundary(b) {
                            b -= 1;
                        }
                        b
                    } else {
                        end
                    };
                    &content[..end]
                }
            ));
        }

        let user_prompt = format!(
            "Query: \"{query}\"\n\nDocuments:{doc_list}\n\nRate each document's relevance (0-10) and provide brief explanations."
        );

        let response = llm.chat(system_prompt, &user_prompt, 0.2).await?;

        // Parse the LLM response
        #[derive(Deserialize)]
        struct LlmScore {
            score: f64,
            explanation: String,
        }

        let trimmed_response = response.trim();
        let json_str = if let Some(rest) = trimmed_response.strip_prefix("```json") {
            rest.strip_suffix("```").unwrap_or(rest).trim()
        } else if let Some(rest) = trimmed_response.strip_prefix("```") {
            rest.strip_suffix("```").unwrap_or(rest).trim()
        } else {
            trimmed_response
        };

        let scores: Vec<LlmScore> = serde_json::from_str(json_str).unwrap_or_else(|e| {
            warn!(error = %e, "Failed to parse LLM rerank response, using default scores");
            docs.iter()
                .map(|_| LlmScore {
                    score: 5.0,
                    explanation: "Failed to parse LLM response".to_string(),
                })
                .collect()
        });

        let results: Vec<(f64, String)> = scores
            .into_iter()
            .take(docs.len())
            .map(|s| (s.score / 10.0, s.explanation))
            .collect();

        // Pad with defaults if LLM returned fewer results
        let mut final_results = results;
        while final_results.len() < docs.len() {
            final_results.push((0.5, "No LLM score available".to_string()));
        }

        debug!(
            query = %query,
            num_docs = docs.len(),
            "LLM reranking complete"
        );

        Ok(final_results)
    }

    /// 3-stage reranking pipeline.
    ///
    /// Stage 1: Bi-encoder scores (already in results from Qdrant dense search)
    /// Stage 2: Cross-encoder via TEI (fast, ~50ms)
    /// Stage 3: LLM reranking (optional, slow 30-90s)
    ///
    /// Final score = w_bi * bi_score + w_cross * cross_score + w_llm * llm_score
    pub async fn rerank_pipeline(
        &self,
        query: &str,
        results: Vec<SearchResult>,
        config: &RerankConfig,
        use_llm: bool,
    ) -> Result<Vec<SearchResult>> {
        if results.is_empty() {
            return Ok(results);
        }

        // Check cache — include entity IDs to avoid collisions between
        // different result sets with the same query and count
        let result_ids: String = results
            .iter()
            .map(|r| r.entity_id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let cache_key = format!("{}:{}:{}", query, results.len(), result_ids);
        if let Some(cached_scores) = self.cache.get(&cache_key).await
            && cached_scores.len() == results.len()
        {
            debug!(query = %query, "rerank cache hit");
            let mut results = results;
            for (i, result) in results.iter_mut().enumerate() {
                if let Some(&score) = cached_scores.get(i) {
                    result.score = score;
                }
            }
            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            return Ok(results);
        }

        let docs: Vec<(String, String)> = results
            .iter()
            .map(|r| (r.title.clone(), r.content.clone()))
            .collect();

        // Stage 1: bi-encoder scores are already in results.score
        let bi_scores: Vec<f64> = results.iter().map(|r| r.score).collect();

        // Normalize bi-scores to 0-1 range
        let bi_max = bi_scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let bi_min = bi_scores.iter().cloned().fold(f64::INFINITY, f64::min);
        let bi_range = if (bi_max - bi_min).abs() < f64::EPSILON {
            1.0
        } else {
            bi_max - bi_min
        };
        let bi_normalized: Vec<f64> = bi_scores.iter().map(|s| (s - bi_min) / bi_range).collect();

        // Stage 2: cross-encoder
        let cross_scores = self.rerank_cross_encoder(query, &docs).await?;

        // Normalize cross-encoder scores
        let cross_max = cross_scores
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let cross_min = cross_scores.iter().cloned().fold(f64::INFINITY, f64::min);
        let cross_range = if (cross_max - cross_min).abs() < f64::EPSILON {
            1.0
        } else {
            cross_max - cross_min
        };
        let cross_normalized: Vec<f64> = cross_scores
            .iter()
            .map(|s| (s - cross_min) / cross_range)
            .collect();

        // Stage 3: LLM (optional)
        let final_scores = if use_llm && self.llm.is_some() {
            // Only send top stage2_top_k to LLM
            let mut indexed: Vec<(usize, f64)> = cross_normalized
                .iter()
                .enumerate()
                .map(|(i, &s)| (i, s))
                .collect();
            indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            indexed.truncate(config.stage2_top_k);

            let top_docs: Vec<(String, String)> =
                indexed.iter().map(|(i, _)| docs[*i].clone()).collect();

            match self.rerank_llm(query, &top_docs).await {
                Ok(llm_results) => {
                    // Build LLM scores for all results (0.0 for those not sent to LLM)
                    let mut llm_scores = vec![0.0; results.len()];
                    for (rank, (orig_idx, _)) in indexed.iter().enumerate() {
                        if let Some((score, _)) = llm_results.get(rank) {
                            llm_scores[*orig_idx] = *score;
                        }
                    }

                    // Weighted combination: bi*w_bi + cross*w_cross + llm*w_llm
                    (0..results.len())
                        .map(|i| {
                            config.w_bi * bi_normalized[i]
                                + config.w_cross * cross_normalized[i]
                                + config.w_llm * llm_scores[i]
                        })
                        .collect::<Vec<f64>>()
                }
                Err(e) => {
                    warn!(error = %e, "LLM reranking failed, falling back to 2-stage");
                    // Fallback: bi*w_bi + cross*(w_cross+w_llm)
                    (0..results.len())
                        .map(|i| {
                            config.w_bi * bi_normalized[i]
                                + (config.w_cross + config.w_llm) * cross_normalized[i]
                        })
                        .collect::<Vec<f64>>()
                }
            }
        } else {
            // No LLM: bi*w_bi + cross*(w_cross+w_llm)
            (0..results.len())
                .map(|i| {
                    config.w_bi * bi_normalized[i]
                        + (config.w_cross + config.w_llm) * cross_normalized[i]
                })
                .collect::<Vec<f64>>()
        };

        // Cache the scores
        self.cache.set(cache_key, final_scores.clone()).await;

        // Apply final scores and sort
        let mut results = results;
        for (i, result) in results.iter_mut().enumerate() {
            result.score = final_scores[i];
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        info!(
            query = %query,
            num_results = results.len(),
            use_llm = use_llm,
            "3-stage reranking complete"
        );

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cache_with_ttl(ttl_ms: u64, max_entries: usize) -> RerankCache {
        RerankCache {
            entries: Mutex::new(HashMap::new()),
            ttl: Duration::from_millis(ttl_ms),
            max_entries,
        }
    }

    #[tokio::test]
    async fn test_cache_miss_on_empty() {
        let cache = RerankCache::new();
        let result = cache.get("nonexistent_key").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_set_get_roundtrip() {
        let cache = RerankCache::new();
        let scores = vec![0.9, 0.8, 0.7];
        cache.set("test_key".to_string(), scores.clone()).await;
        let result = cache.get("test_key").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap(), scores);
    }

    #[tokio::test]
    async fn test_cache_eviction_at_max_entries() {
        let cache = make_cache_with_ttl(60_000, 2); // max 2 entries

        cache.set("key1".to_string(), vec![1.0]).await;
        // Small sleep to ensure different Instant::now() values
        tokio::time::sleep(Duration::from_millis(5)).await;
        cache.set("key2".to_string(), vec![2.0]).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        // This should evict the oldest (key1)
        cache.set("key3".to_string(), vec![3.0]).await;

        let entries = cache.entries.lock().await;
        assert_eq!(entries.len(), 2);
        // key1 should have been evicted (oldest)
        assert!(!entries.contains_key("key1"));
        assert!(entries.contains_key("key2"));
        assert!(entries.contains_key("key3"));
    }

    #[tokio::test]
    async fn test_cache_ttl_expiry() {
        let cache = make_cache_with_ttl(50, 500); // 50ms TTL
        cache.set("expire_me".to_string(), vec![1.0]).await;

        // Should exist immediately
        assert!(cache.get("expire_me").await.is_some());

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should be expired now
        assert!(cache.get("expire_me").await.is_none());
    }

    #[test]
    fn test_rerank_config_defaults() {
        let config = RerankConfig::default();
        assert_eq!(config.stage2_top_k, 20);
        assert!((config.w_bi + config.w_cross + config.w_llm - 1.0).abs() < 1e-10);
    }
}
