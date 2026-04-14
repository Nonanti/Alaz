//! Search pipeline orchestrating the 6-signal hybrid search.
//!
//! The pipeline runs FTS, dense text, and ColBERT signals concurrently,
//! then uses the top results as seeds for graph expansion and RAPTOR search.
//! For temporal/causal queries, a 6th cue search signal targets episodes
//! via 5W cues (who/what/where/when/why).
//! All signals are fused via weighted RRF — where the per-signal weights are
//! determined by the adaptive query classifier — optionally reranked, and
//! hydrated from the DB.

use std::collections::HashMap;
use std::sync::Arc;

use alaz_core::Result;
use alaz_core::traits::{SearchQuery, SearchResult, SignalResult};
use alaz_db::repos::{
    CoreMemoryRepo, EpisodeRepo, KnowledgeRepo, ProcedureRepo, ReflectionRepo, SearchQueryRepo,
};
use alaz_intel::{ColbertService, EmbeddingService, HydeGenerator, LlmClient};
use alaz_vector::QdrantManager;
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::cache::SearchCache;
use crate::classifier::{self, QueryType, SearchWeights};
use crate::fusion::{self, FusionExplanation};
use crate::rerank::Reranker;
use crate::signals;

/// The main search pipeline combining 6 retrieval signals.
pub struct SearchPipeline {
    pub pool: PgPool,
    pub qdrant: Arc<QdrantManager>,
    pub embedding: Arc<EmbeddingService>,
    pub colbert: Arc<ColbertService>,
    pub reranker: Reranker,
    pub hyde: HydeGenerator,
    pub cache: SearchCache,
}

impl SearchPipeline {
    /// Execute a hybrid search across all signals.
    pub async fn hybrid_search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let rerank = query.rerank.unwrap_or(true);
        let hyde = query.hyde.unwrap_or(false);

        let limit = query.limit.unwrap_or(10);

        // Check cache first
        if let Some(cached) = self
            .cache
            .get(&query.query, query.project.as_deref(), rerank, hyde, limit)
            .await
        {
            debug!(
                query = %query.query,
                results = cached.len(),
                "cache hit — returning cached results"
            );
            return Ok(cached);
        }
        let fetch_limit = limit * 3;

        // Steps 0-2: Classify query, optional HyDE, embed
        let (query_type, text_embedding) = self.classify_and_embed(query).await?;
        let weights = query_type.weights(&self.pool).await;

        // Step 3: Primary signals (FTS, dense, ColBERT)
        let (fts, text, colbert) = self
            .run_primary_signals(
                &query.query,
                &text_embedding,
                query.project.as_deref(),
                fetch_limit,
            )
            .await;

        // Steps 4-5: Build seeds and run secondary signals
        let mut seeds: Vec<(String, String)> = Vec::new();
        for r in fts.iter().chain(text.iter()).take(10) {
            seeds.push((r.entity_type.clone(), r.entity_id.clone()));
        }
        let mut seen = std::collections::HashSet::new();
        seeds.retain(|s| seen.insert(s.clone()));

        let (graph, raptor, cue) = self
            .run_secondary_signals(
                &seeds,
                &text_embedding,
                query.project.as_deref(),
                fetch_limit,
                &weights,
                query.graph_expand.unwrap_or(true),
                &query.query,
            )
            .await;

        // Steps 6-8: Fuse all signals via weighted RRF
        let all_signals = vec![fts, text, colbert, graph, raptor, cue];
        let signal_attribution = fusion::build_signal_attribution(&all_signals);
        let (candidates, fusion_explanations) =
            self.fuse_signals(all_signals, &weights, &query_type, limit);

        // Step 9: Batch-hydrate results from DB
        let mut results = self.hydrate_candidates(candidates).await;

        // Step 10: Re-sort by decayed score, then optional reranking
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let results = if rerank {
            let fallback = results.clone();
            let config = crate::rerank::RerankConfig::default();
            match self
                .reranker
                .rerank_pipeline(&query.query, results, &config, true)
                .await
            {
                Ok(reranked) => reranked,
                Err(e) => {
                    warn!(error = %e, "reranking failed, returning original order");
                    fallback
                }
            }
        } else {
            results
        };

        // Step 11: Async search logging
        self.log_search_query(
            &query.query,
            query.project.as_deref(),
            &query_type,
            &results,
            &signal_attribution,
            &fusion_explanations,
        );

        debug!(results = results.len(), "search pipeline complete");

        // Store in cache
        self.cache
            .put(
                &query.query,
                query.project.as_deref(),
                rerank,
                hyde,
                limit,
                results.clone(),
            )
            .await;

        Ok(results)
    }

    // ── RAG Fusion Search ───────────────────────────────────────────

    /// RAG fusion search: expand query into multiple formulations, search each,
    /// and fuse results via RRF.
    ///
    /// Uses the LLM to generate 3 alternative phrasings of the original query,
    /// runs `hybrid_search` for each (with reranking disabled for speed), then
    /// fuses all result lists through weighted RRF with equal weights.
    pub async fn rag_fusion_search(
        &self,
        query: &SearchQuery,
        llm: &LlmClient,
    ) -> Result<Vec<SearchResult>> {
        // 1. Expand query into 3-4 formulations via LLM
        let queries = expand_query(llm, &query.query).await?;

        debug!(
            original = %query.query,
            expanded_count = queries.len(),
            "RAG fusion: expanded queries"
        );

        // 2. Run hybrid_search for each (sequentially to avoid overwhelming the system)
        //    Disable reranking for sub-queries (speed)
        let mut all_signal_results: Vec<Vec<SignalResult>> = Vec::new();

        for q in &queries {
            let sub_query = SearchQuery {
                query: q.clone(),
                project: query.project.clone(),
                limit: query.limit,
                rerank: Some(false),
                hyde: Some(false),
                graph_expand: Some(true),
            };

            let results = self.hybrid_search(&sub_query).await.unwrap_or_default();

            // Convert SearchResults back to SignalResults for RRF
            let signals: Vec<SignalResult> = results
                .into_iter()
                .enumerate()
                .map(|(rank, r)| SignalResult {
                    entity_type: r.entity_type,
                    entity_id: r.entity_id,
                    rank,
                })
                .collect();

            all_signal_results.push(signals);
        }

        // 3. Fuse all results via RRF with equal weights
        let weights: Vec<f32> = vec![1.0; all_signal_results.len()];
        let (fused, _) = fusion::weighted_rrf_with_explanations(all_signal_results, &weights);

        // 4. Take top results and hydrate
        let limit = query.limit.unwrap_or(10);
        let candidates: Vec<_> = fused.into_iter().take(limit).collect();
        let results = self.hydrate_candidates(candidates).await;

        Ok(results)
    }

    // ── Steps 0-2: Query classification + HyDE + embedding ──────────

    /// Classify the query, optionally generate a HyDE document, and embed.
    async fn classify_and_embed(&self, query: &SearchQuery) -> Result<(QueryType, Vec<f32>)> {
        // 0. Classify
        let query_type = classifier::classify_query(&query.query);
        info!(
            query = %query.query,
            query_type = %query_type,
            "query classified"
        );

        // 1. Optional HyDE
        let search_text = if query.hyde.unwrap_or(false) {
            self.hyde.generate(&query.query).await.unwrap_or_else(|e| {
                warn!(error = %e, "HyDE generation failed, using original query");
                query.query.clone()
            })
        } else {
            query.query.clone()
        };

        // 2. Embed
        let text_embed = self.embedding.embed_text(&[&search_text]).await?;
        let text_embedding = text_embed.into_iter().next().unwrap_or_default();

        Ok((query_type, text_embedding))
    }

    // ── Step 3: Primary signals (FTS + dense + ColBERT) ─────────────

    /// Run FTS, dense text, and ColBERT signals concurrently.
    async fn run_primary_signals(
        &self,
        query_text: &str,
        text_embedding: &[f32],
        project: Option<&str>,
        fetch_limit: usize,
    ) -> (Vec<SignalResult>, Vec<SignalResult>, Vec<SignalResult>) {
        let (fts_res, text_res, colbert_res) = tokio::join!(
            signals::fts::execute(&self.pool, query_text, project, fetch_limit),
            signals::dense_text::execute(&self.qdrant, text_embedding, project, fetch_limit),
            signals::colbert::execute(
                &self.qdrant,
                &self.colbert,
                query_text,
                project,
                fetch_limit
            ),
        );

        let fts = fts_res.unwrap_or_default();
        let text = text_res.unwrap_or_default();
        let colbert = colbert_res.unwrap_or_default();

        debug!(
            fts = fts.len(),
            text = text.len(),
            colbert = colbert.len(),
            "signals 1-3 complete"
        );

        (fts, text, colbert)
    }

    // ── Steps 4-5: Secondary signals (graph + RAPTOR + cue) ────────

    /// Run graph expansion, RAPTOR, and conditional cue search concurrently.
    #[allow(clippy::too_many_arguments)]
    async fn run_secondary_signals(
        &self,
        seeds: &[(String, String)],
        text_embedding: &[f32],
        project: Option<&str>,
        fetch_limit: usize,
        weights: &SearchWeights,
        graph_expand: bool,
        query_text: &str,
    ) -> (Vec<SignalResult>, Vec<SignalResult>, Vec<SignalResult>) {
        let (graph_res, raptor_res, cue_res) = tokio::join!(
            signals::graph::execute(&self.pool, seeds, fetch_limit),
            signals::raptor::execute(
                &self.pool,
                &self.qdrant,
                text_embedding,
                project,
                fetch_limit
            ),
            async {
                if weights.cue_search {
                    signals::cue::execute(&self.pool, query_text, project, fetch_limit).await
                } else {
                    Ok(vec![])
                }
            },
        );

        let graph = if graph_expand {
            graph_res.unwrap_or_default()
        } else {
            vec![]
        };
        let raptor = raptor_res.unwrap_or_default();
        let cue = cue_res.unwrap_or_default();

        debug!(
            graph = graph.len(),
            raptor = raptor.len(),
            cue = cue.len(),
            "signals 4-6 complete"
        );

        (graph, raptor, cue)
    }

    // ── Steps 6-8: RRF fusion ──────────────────────────────────────

    /// Apply weighted RRF fusion and take top candidates.
    fn fuse_signals(
        &self,
        all_signals: Vec<Vec<SignalResult>>,
        weights: &SearchWeights,
        query_type: &QueryType,
        limit: usize,
    ) -> (Vec<(String, String, f64)>, Vec<FusionExplanation>) {
        let cue_weight = if weights.cue_search { 1.5 } else { 0.0 };
        let signal_weights = [
            weights.fts,    // FTS
            weights.dense,  // dense text
            weights.dense,  // ColBERT (also dense-family)
            weights.graph,  // graph expansion
            weights.raptor, // RAPTOR
            cue_weight,     // cue search (episodic 5W)
        ];

        let (fused, fusion_explanations) =
            fusion::weighted_rrf_with_explanations(all_signals, &signal_weights);

        debug!(
            total_fused = fused.len(),
            query_type = %query_type,
            fts_weight = weights.fts,
            dense_weight = weights.dense,
            graph_weight = weights.graph,
            raptor_weight = weights.raptor,
            cue_weight = cue_weight,
            "weighted RRF fusion complete"
        );

        let candidates: Vec<_> = fused.into_iter().take(limit).collect();
        (candidates, fusion_explanations)
    }

    // ── Step 9: Batch hydration from DB ─────────────────────────────

    /// Batch-fetch entities from DB and hydrate into `SearchResult`s.
    ///
    /// Applies staleness filtering and memory decay/boost scoring.
    async fn hydrate_candidates(
        &self,
        candidates: Vec<(String, String, f64)>,
    ) -> Vec<SearchResult> {
        // Collect IDs grouped by entity type
        let mut knowledge_ids: Vec<String> = Vec::new();
        let mut episode_ids: Vec<String> = Vec::new();
        let mut procedure_ids: Vec<String> = Vec::new();
        let mut core_memory_ids: Vec<String> = Vec::new();
        let mut reflection_ids: Vec<String> = Vec::new();

        for (entity_type, entity_id, _) in &candidates {
            match entity_type.as_str() {
                "knowledge_item" | "knowledge" => knowledge_ids.push(entity_id.clone()),
                "episode" => episode_ids.push(entity_id.clone()),
                "procedure" => procedure_ids.push(entity_id.clone()),
                "core_memory" => core_memory_ids.push(entity_id.clone()),
                "reflection" => reflection_ids.push(entity_id.clone()),
                _ => {}
            }
        }

        // Batch fetch all types concurrently
        let (ki_result, ep_result, proc_result, cm_result, ref_result) = tokio::join!(
            KnowledgeRepo::get_many(&self.pool, &knowledge_ids),
            EpisodeRepo::get_many(&self.pool, &episode_ids),
            ProcedureRepo::get_many(&self.pool, &procedure_ids),
            CoreMemoryRepo::get_many(&self.pool, &core_memory_ids),
            ReflectionRepo::get_many(&self.pool, &reflection_ids),
        );

        let ki_map: HashMap<String, _> = ki_result
            .unwrap_or_else(|e| {
                warn!(error = %e, "batch knowledge fetch failed");
                vec![]
            })
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();

        let ep_map: HashMap<String, _> = ep_result
            .unwrap_or_else(|e| {
                warn!(error = %e, "batch episode fetch failed");
                vec![]
            })
            .into_iter()
            .map(|ep| (ep.id.clone(), ep))
            .collect();

        let proc_map: HashMap<String, _> = proc_result
            .unwrap_or_else(|e| {
                warn!(error = %e, "batch procedure fetch failed");
                vec![]
            })
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect();

        let cm_map: HashMap<String, _> = cm_result
            .unwrap_or_else(|e| {
                warn!(error = %e, "batch core_memory fetch failed");
                vec![]
            })
            .into_iter()
            .map(|cm| (cm.id.clone(), cm))
            .collect();

        let ref_map: HashMap<String, _> = ref_result
            .unwrap_or_else(|e| {
                warn!(error = %e, "batch reflection fetch failed");
                vec![]
            })
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();

        // Hydrate in fused-score order
        let mut results = Vec::new();
        for (entity_type, entity_id, score) in candidates {
            let result = match entity_type.as_str() {
                "knowledge_item" | "knowledge" => {
                    hydrate_knowledge(&ki_map, &entity_type, &entity_id, score)
                }
                "episode" => hydrate_episode(&ep_map, &entity_type, &entity_id, score),
                "procedure" => hydrate_procedure(&proc_map, &entity_type, &entity_id, score),
                "core_memory" => cm_map.get(&entity_id).map(|cm| SearchResult {
                    entity_type: entity_type.clone(),
                    entity_id: entity_id.clone(),
                    title: format!("[{}] {}", cm.category, cm.key),
                    content: cm.value.clone(),
                    score,
                    project: cm.project_id.clone(),
                    metadata: Some(serde_json::json!({
                        "category": cm.category,
                        "confidence": cm.confidence,
                    })),
                }),
                "reflection" => ref_map.get(&entity_id).map(|r| SearchResult {
                    entity_type: entity_type.clone(),
                    entity_id: entity_id.clone(),
                    title: format!("Reflection: {}", r.session_id),
                    content: r.lessons_learned.clone().unwrap_or_default(),
                    score,
                    project: r.project_id.clone(),
                    metadata: Some(serde_json::json!({
                        "effectiveness": r.effectiveness_score,
                        "overall": r.overall_score,
                    })),
                }),
                _ => {
                    warn!(entity_type = %entity_type, entity_id = %entity_id, "unknown entity type in hydration");
                    None
                }
            };

            if let Some(r) = result {
                results.push(r);
            } else {
                debug!(
                    entity_type = %entity_type,
                    entity_id = %entity_id,
                    "entity not found or stale during hydration, skipping"
                );
            }
        }

        results
    }

    // ── Step 11: Async search logging ───────────────────────────────

    /// Spawn a background task to log the search query for feedback tracking.
    fn log_search_query(
        &self,
        query_text: &str,
        project: Option<&str>,
        query_type: &QueryType,
        results: &[SearchResult],
        signal_attribution: &HashMap<String, Vec<String>>,
        fusion_explanations: &[FusionExplanation],
    ) {
        let log_pool = self.pool.clone();
        let log_query = query_text.to_string();
        let log_project = project.map(String::from);
        let log_query_type = query_type.to_string();
        let result_ids: Vec<String> = results.iter().map(|r| r.entity_id.clone()).collect();

        let signal_sources = build_signal_sources_json(&result_ids, signal_attribution);
        let explanations_json = build_explanations_json(&result_ids, fusion_explanations);

        tokio::spawn(async move {
            if let Err(e) = SearchQueryRepo::log(
                &log_pool,
                &log_query,
                log_project.as_deref(),
                &result_ids,
                Some(&log_query_type),
                Some(&signal_sources),
                Some(&explanations_json),
            )
            .await
            {
                warn!(error = %e, "failed to log search query for feedback");
            }
        });
    }
}

// ── Hydration helpers ───────────────────────────────────────────────

/// Hydrate a knowledge item from the lookup map.
fn hydrate_knowledge(
    ki_map: &HashMap<String, alaz_core::models::KnowledgeItem>,
    entity_type: &str,
    entity_id: &str,
    score: f64,
) -> Option<SearchResult> {
    ki_map.get(entity_id).and_then(|item| {
        if is_stale(item.superseded_by.as_deref(), item.valid_until) {
            debug!(entity_id, "skipping stale knowledge item");
            return None;
        }
        let decayed_score = decay_and_boost(
            score,
            item.last_accessed_at,
            item.access_count,
            item.feedback_boost,
        );
        Some(SearchResult {
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            title: item.title.clone(),
            content: item.content.clone(),
            score: decayed_score,
            project: item.project_id.clone(),
            metadata: Some(serde_json::json!({
                "kind": item.kind,
                "language": item.language,
                "tags": item.tags,
                "file_path": item.file_path,
            })),
        })
    })
}

/// Hydrate an episode from the lookup map.
fn hydrate_episode(
    ep_map: &HashMap<String, alaz_core::models::Episode>,
    entity_type: &str,
    entity_id: &str,
    score: f64,
) -> Option<SearchResult> {
    ep_map.get(entity_id).and_then(|ep| {
        if is_stale(ep.superseded_by.as_deref(), ep.valid_until) {
            debug!(entity_id, "skipping stale episode");
            return None;
        }
        let decayed_score = decay_and_boost(
            score,
            ep.last_accessed_at.or(Some(ep.created_at)),
            ep.access_count,
            ep.feedback_boost,
        );
        Some(SearchResult {
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            title: ep.title.clone(),
            content: ep.content.clone(),
            score: decayed_score,
            project: ep.project_id.clone(),
            metadata: Some(serde_json::json!({
                "kind": ep.kind,
                "severity": ep.severity,
                "resolved": ep.resolved,
            })),
        })
    })
}

/// Hydrate a procedure from the lookup map.
fn hydrate_procedure(
    proc_map: &HashMap<String, alaz_core::models::Procedure>,
    entity_type: &str,
    entity_id: &str,
    score: f64,
) -> Option<SearchResult> {
    proc_map.get(entity_id).and_then(|proc| {
        if is_stale(proc.superseded_by.as_deref(), proc.valid_until) {
            debug!(entity_id, "skipping stale procedure");
            return None;
        }
        let decayed_score = decay_and_boost(
            score,
            proc.last_accessed_at.or(Some(proc.created_at)),
            proc.access_count,
            proc.feedback_boost,
        );
        Some(SearchResult {
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            title: proc.title.clone(),
            content: proc.content.clone(),
            score: decayed_score,
            project: proc.project_id.clone(),
            metadata: Some(serde_json::json!({
                "success_rate": proc.success_rate,
                "times_used": proc.times_used,
                "tags": proc.tags,
            })),
        })
    })
}

// ── JSON builders for search logging ────────────────────────────────

/// Build signal source map: `{"entity_id": ["fts", "dense"], ...}`
fn build_signal_sources_json(
    result_ids: &[String],
    signal_attribution: &HashMap<String, Vec<String>>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for id in result_ids {
        if let Some(sources) = signal_attribution.get(id) {
            map.insert(
                id.clone(),
                serde_json::Value::Array(
                    sources
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }
    }
    serde_json::Value::Object(map)
}

/// Build fusion explanations JSON for the final result set.
fn build_explanations_json(
    result_ids: &[String],
    fusion_explanations: &[FusionExplanation],
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for expl in fusion_explanations {
        if result_ids.contains(&expl.entity_id)
            && let Ok(val) = serde_json::to_value(expl)
        {
            map.insert(expl.entity_id.clone(), val);
        }
    }
    serde_json::Value::Object(map)
}

// ── Scoring helpers ─────────────────────────────────────────────────

/// Check if an entity is superseded or expired.
fn is_stale(
    superseded_by: Option<&str>,
    valid_until: Option<chrono::DateTime<chrono::Utc>>,
) -> bool {
    superseded_by.is_some() || valid_until.is_some_and(|v| chrono::Utc::now() > v)
}

/// Apply memory decay and feedback boost to a fused score.
fn decay_and_boost(
    score: f64,
    last_accessed_at: Option<chrono::DateTime<chrono::Utc>>,
    access_count: i64,
    feedback_boost: f32,
) -> f64 {
    crate::decay::apply_decay(score, last_accessed_at, access_count as i32)
        + f64::from(feedback_boost) * 0.1
}

// ── Query expansion for RAG fusion ─────────────────────────────────

/// Use the LLM to generate alternative phrasings of a search query.
///
/// Returns 3 alternative queries (plus the original) for a total of 4
/// formulations. If the LLM call fails, falls back to just the original.
async fn expand_query(llm: &LlmClient, query: &str) -> alaz_core::Result<Vec<String>> {
    let system = "You are a search query expansion assistant. Given a search query, \
        generate exactly 3 alternative phrasings that would help find relevant results. \
        Return ONLY the 3 queries, one per line, with no numbering or extra text.";
    let user = format!("Original query: {}", query);

    let mut queries = vec![query.to_string()];

    match llm.chat(system, &user, 0.3).await {
        Ok(response) => {
            for line in response.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    queries.push(line.to_string());
                }
            }
            debug!(
                original = %query,
                expanded = queries.len() - 1,
                "query expansion complete"
            );
        }
        Err(e) => {
            warn!(error = %e, "query expansion failed, using original query only");
        }
    }

    Ok(queries)
}
