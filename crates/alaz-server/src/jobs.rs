//! Background jobs for the Alaz server.
//!
//! Periodic jobs run alongside the HTTP server:
//! - **embedding_backfill_job**: every 5 minutes, embeds entities that need it
//! - **graph_decay_job**: every 6 hours, decays graph edge weights
//! - **memory_decay_job**: every 6 hours, decays/boosts/prunes entity utility scores
//! - **feedback_aggregation_job**: every 12 hours, aggregates search feedback

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::metrics::SharedMetrics;
use alaz_core::{Embeddable, Result};
use alaz_db::repos::{
    CoreMemoryRepo, EpisodeRepo, GraphRepo, KnowledgeRepo, ProcedureRepo, ReflectionRepo,
    SearchQueryRepo,
};
use alaz_intel::{ColbertService, EmbeddingService};
use alaz_vector::{ColbertOps, DenseVectorOps, QdrantManager};
use sqlx::PgPool;
use tracing::{debug, error, info, warn};

/// Maximum number of entities to process per embedding backfill cycle.
const BACKFILL_BATCH_SIZE: i64 = 50;

/// Interval between embedding backfill cycles: 5 minutes.
const BACKFILL_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Interval between graph decay cycles: 6 hours.
const DECAY_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Interval between memory decay cycles: 6 hours.
const MEMORY_DECAY_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Interval between feedback aggregation cycles: 12 hours.
const FEEDBACK_AGGREGATION_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

/// Interval between signal weight learning cycles: 7 days.
const WEIGHT_LEARNING_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Interval between knowledge consolidation cycles: 7 days.
const CONSOLIDATION_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Decay factor per cycle: items not accessed in 7 days lose 5% of their utility score.
const MEMORY_DECAY_FACTOR: f64 = 0.95;

/// Items below this utility score are candidates for pruning.
const MEMORY_PRUNE_THRESHOLD: f64 = 0.1;

/// Minimum age before an item can be pruned (30 days in seconds).
const MIN_AGE_FOR_PRUNING_SECS: i64 = 30 * 24 * 60 * 60;

// ---------------------------------------------------------------------------
// Generic embedding helpers
// ---------------------------------------------------------------------------

/// Embed and upsert a single entity's vectors to Qdrant (dense text + optional ColBERT).
///
/// `content` is passed explicitly to avoid a second `embed_content()` call
/// (which allocates for CoreMemory/Reflection via `format!`).
async fn upsert_vectors<E: Embeddable>(
    item: &E,
    text_vec: Vec<f32>,
    content: &str,
    qdrant: &QdrantManager,
    colbert: &ColbertService,
) -> Result<()> {
    let etype = item.entity_type_name();
    let eid = item.entity_id();
    let pid = item.project_id();

    DenseVectorOps::upsert_text(qdrant.client(), etype, eid, pid, text_vec).await?;

    if item.needs_colbert() {
        let tokens = colbert.embed_document(content).await?;
        if !tokens.is_empty() {
            ColbertOps::upsert_colbert(qdrant.client(), etype, eid, pid, tokens).await?;
        }
    }

    Ok(())
}

/// Type alias for the `mark_embedded` callback.
///
/// Uses higher-ranked trait bounds (`for<'a>`) so the returned future borrows
/// from the pool and id with the correct lifetime.
type MarkEmbeddedFn =
    for<'a> fn(
        &'a PgPool,
        &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>>;

/// Process a batch of embeddable entities: embed text, upsert vectors, mark as embedded.
///
/// Generic over any `Embeddable` type. The `mark_embedded` callback is a function
/// pointer to the appropriate repo's `mark_embedded` method.
async fn embed_entity_batch<E: Embeddable>(
    items: Vec<E>,
    pool: &PgPool,
    qdrant: &QdrantManager,
    embedding: &EmbeddingService,
    colbert: &ColbertService,
    mark_embedded: MarkEmbeddedFn,
    label: &str,
) -> (u32, u32) {
    if items.is_empty() {
        return (0, 0);
    }

    let mut processed = 0u32;
    let mut errors = 0u32;

    debug!(count = items.len(), label, "backfill: processing batch");

    let texts: Vec<String> = items.iter().map(|e| e.embed_content()).collect();
    let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

    match embedding.embed_text(&text_refs).await {
        Ok(vectors) => {
            // Phase 1: Upsert dense vectors sequentially (fast Qdrant RPCs)
            let mut dense_ok = vec![false; items.len()];
            for (i, (item, text_vec)) in items.iter().zip(vectors.into_iter()).enumerate() {
                match DenseVectorOps::upsert_text(
                    qdrant.client(),
                    item.entity_type_name(),
                    item.entity_id(),
                    item.project_id(),
                    text_vec,
                )
                .await
                {
                    Ok(()) => dense_ok[i] = true,
                    Err(e) => {
                        error!(id = %item.entity_id(), label, error = %e, "failed to upsert dense vector");
                        errors += 1;
                    }
                }
            }

            // Phase 2: ColBERT embeddings in parallel (chunks of 4 concurrent)
            let colbert_indices: Vec<usize> = (0..items.len())
                .filter(|&i| dense_ok[i] && items[i].needs_colbert())
                .collect();

            let mut colbert_ok = vec![true; items.len()];

            for chunk in colbert_indices.chunks(4) {
                let futs: Vec<_> = chunk
                    .iter()
                    .map(|&i| {
                        let content = texts[i].as_str();
                        let item = &items[i];
                        let colbert_ref = colbert;
                        let qdrant_ref = qdrant;
                        async move {
                            let tokens = colbert_ref.embed_document(content).await?;
                            if !tokens.is_empty() {
                                ColbertOps::upsert_colbert(
                                    qdrant_ref.client(),
                                    item.entity_type_name(),
                                    item.entity_id(),
                                    item.project_id(),
                                    tokens,
                                )
                                .await?;
                            }
                            Ok::<(), alaz_core::AlazError>(())
                        }
                    })
                    .collect();

                let results = futures::future::join_all(futs).await;
                for (&idx, result) in chunk.iter().zip(results) {
                    if let Err(e) = result {
                        error!(id = %items[idx].entity_id(), label, error = %e, "ColBERT embedding failed");
                        colbert_ok[idx] = false;
                        errors += 1;
                    }
                }
            }

            // Phase 3: Mark all successfully processed items as embedded
            for (i, item) in items.iter().enumerate() {
                if dense_ok[i] && colbert_ok[i] {
                    if let Err(e) = mark_embedded(pool, item.entity_id()).await {
                        error!(id = %item.entity_id(), label, error = %e, "failed to mark embedded");
                        errors += 1;
                    } else {
                        processed += 1;
                    }
                }
            }
        }
        Err(batch_err) => {
            warn!(label, error = %batch_err, "batch embed failed, falling back to one-at-a-time");
            for (item, text) in items.iter().zip(texts.iter()) {
                let vec = match embedding.embed_text(&[text.as_str()]).await {
                    Ok(vecs) => vecs.into_iter().next(),
                    Err(e) => {
                        error!(id = %item.entity_id(), label, error = %e, "failed to embed single");
                        errors += 1;
                        continue;
                    }
                };
                if let Some(vec) = vec {
                    match upsert_vectors(item, vec, text, qdrant, colbert).await {
                        Ok(()) => {
                            if let Err(e) = mark_embedded(pool, item.entity_id()).await {
                                error!(id = %item.entity_id(), label, error = %e, "failed to mark embedded");
                                errors += 1;
                            } else {
                                processed += 1;
                            }
                        }
                        Err(e) => {
                            error!(id = %item.entity_id(), label, error = %e, "failed to embed");
                            errors += 1;
                        }
                    }
                }
            }
        }
    }

    (processed, errors)
}

// Wrapper functions that return pinned futures for each repo's mark_embedded.
// Wrapper functions returning pinned futures for each repo's `mark_embedded`.
// Explicit `'a` lifetime required to match the `MarkEmbeddedFn` HRTB signature.
fn mark_knowledge<'a>(
    pool: &'a PgPool,
    id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(KnowledgeRepo::mark_embedded(pool, id))
}
fn mark_episode<'a>(
    pool: &'a PgPool,
    id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(EpisodeRepo::mark_embedded(pool, id))
}
fn mark_procedure<'a>(
    pool: &'a PgPool,
    id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(ProcedureRepo::mark_embedded(pool, id))
}
fn mark_core_memory<'a>(
    pool: &'a PgPool,
    id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(CoreMemoryRepo::mark_embedded(pool, id))
}
fn mark_reflection<'a>(
    pool: &'a PgPool,
    id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(ReflectionRepo::mark_embedded(pool, id))
}

// ---------------------------------------------------------------------------
// Embedding backfill job
// ---------------------------------------------------------------------------

/// Background job: every 5 minutes, finds entities with `needs_embedding = true`
/// across all entity types, embeds them, upserts to Qdrant, and marks as embedded.
pub async fn embedding_backfill_job(
    pool: PgPool,
    qdrant: Arc<QdrantManager>,
    embedding: Arc<EmbeddingService>,
    colbert: Arc<ColbertService>,
    metrics: SharedMetrics,
) {
    let mut interval = tokio::time::interval(BACKFILL_INTERVAL);

    loop {
        interval.tick().await;
        info!("embedding backfill: starting cycle");

        let mut total_processed = 0u32;
        let mut total_errors = 0u32;

        // Process each entity type through the generic embedding pipeline.
        if let Ok(items) = KnowledgeRepo::find_needing_embedding(&pool, BACKFILL_BATCH_SIZE).await {
            let (p, e) = embed_entity_batch(
                items,
                &pool,
                &qdrant,
                &embedding,
                &colbert,
                mark_knowledge,
                "knowledge",
            )
            .await;
            total_processed += p;
            total_errors += e;
        }
        if let Ok(items) = EpisodeRepo::find_needing_embedding(&pool, BACKFILL_BATCH_SIZE).await {
            let (p, e) = embed_entity_batch(
                items,
                &pool,
                &qdrant,
                &embedding,
                &colbert,
                mark_episode,
                "episode",
            )
            .await;
            total_processed += p;
            total_errors += e;
        }
        if let Ok(items) = ProcedureRepo::find_needing_embedding(&pool, BACKFILL_BATCH_SIZE).await {
            let (p, e) = embed_entity_batch(
                items,
                &pool,
                &qdrant,
                &embedding,
                &colbert,
                mark_procedure,
                "procedure",
            )
            .await;
            total_processed += p;
            total_errors += e;
        }
        if let Ok(items) = CoreMemoryRepo::find_needing_embedding(&pool, BACKFILL_BATCH_SIZE).await
        {
            let (p, e) = embed_entity_batch(
                items,
                &pool,
                &qdrant,
                &embedding,
                &colbert,
                mark_core_memory,
                "core_memory",
            )
            .await;
            total_processed += p;
            total_errors += e;
        }
        if let Ok(items) = ReflectionRepo::find_needing_embedding(&pool, BACKFILL_BATCH_SIZE).await
        {
            let (p, e) = embed_entity_batch(
                items,
                &pool,
                &qdrant,
                &embedding,
                &colbert,
                mark_reflection,
                "reflection",
            )
            .await;
            total_processed += p;
            total_errors += e;
        }

        if total_processed > 0 || total_errors > 0 {
            metrics
                .backfill_processed
                .fetch_add(total_processed as u64, Ordering::Relaxed);
            metrics
                .embedding_count
                .fetch_add(total_processed as u64, Ordering::Relaxed);
            info!(
                processed = total_processed,
                errors = total_errors,
                "embedding backfill: cycle complete"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Graph decay job
// ---------------------------------------------------------------------------

/// Background job: every 6 hours, applies exponential decay to graph edge weights
/// and removes edges that fall below the threshold.
pub async fn graph_decay_job(pool: PgPool) {
    let mut interval = tokio::time::interval(DECAY_INTERVAL);

    loop {
        interval.tick().await;
        info!("graph decay: starting cycle");

        match GraphRepo::decay_weights(&pool).await {
            Ok(deleted) => {
                info!(deleted_edges = deleted, "graph decay: cycle complete");
            }
            Err(e) => {
                error!(error = %e, "graph decay: failed to decay weights");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Memory decay job
// ---------------------------------------------------------------------------

/// Entity tables that support utility decay, boost, and pruning.
///
/// Each variant maps to a table name and Qdrant entity type via methods,
/// ensuring SQL identifiers are always compile-time constants.
#[derive(Debug, Clone, Copy)]
enum DecayableEntity {
    Knowledge,
    Episode,
    Procedure,
}

impl DecayableEntity {
    /// All decayable entity types.
    const ALL: &[Self] = &[Self::Knowledge, Self::Episode, Self::Procedure];

    /// SQL table name (compile-time safe).
    const fn table(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge_items",
            Self::Episode => "episodes",
            Self::Procedure => "procedures",
        }
    }

    /// Qdrant entity type identifier.
    const fn entity_type(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge_item",
            Self::Episode => "episode",
            Self::Procedure => "procedure",
        }
    }

    /// Decay stale items: not accessed in 7 days or never accessed and older than 7 days.
    async fn decay(self, pool: &PgPool) -> u64 {
        let stale = self.execute_update(
            pool,
            "SET utility_score = utility_score * $1 WHERE last_accessed_at < now() - interval '7 days' AND utility_score > $2",
            Some(MEMORY_DECAY_FACTOR),
            Some(MEMORY_PRUNE_THRESHOLD),
        ).await;

        let never_accessed = self.execute_update(
            pool,
            "SET utility_score = utility_score * $1 WHERE last_accessed_at IS NULL AND created_at < now() - interval '7 days' AND utility_score > $2",
            Some(MEMORY_DECAY_FACTOR),
            Some(MEMORY_PRUNE_THRESHOLD),
        ).await;

        stale + never_accessed
    }

    /// Boost recently accessed items.
    async fn boost(self, pool: &PgPool) -> u64 {
        self.execute_update(
            pool,
            "SET utility_score = LEAST(utility_score * 1.1, 1.0) WHERE last_accessed_at >= now() - interval '7 days' AND utility_score < 1.0",
            None,
            None,
        ).await
    }

    /// Prune old low-utility items and their graph edges + Qdrant vectors.
    async fn prune(self, pool: &PgPool, qdrant: &QdrantManager, min_age: &str) -> u64 {
        // Match on self to use compile-time SQL per variant
        let ids: Vec<String> = match self {
            Self::Knowledge => {
                sqlx::query_scalar("SELECT id FROM knowledge_items WHERE utility_score < $1 AND created_at < now() - $2::interval")
                    .bind(MEMORY_PRUNE_THRESHOLD).bind(min_age).fetch_all(pool).await
            }
            Self::Episode => {
                sqlx::query_scalar("SELECT id FROM episodes WHERE utility_score < $1 AND created_at < now() - $2::interval")
                    .bind(MEMORY_PRUNE_THRESHOLD).bind(min_age).fetch_all(pool).await
            }
            Self::Procedure => {
                sqlx::query_scalar("SELECT id FROM procedures WHERE utility_score < $1 AND created_at < now() - $2::interval")
                    .bind(MEMORY_PRUNE_THRESHOLD).bind(min_age).fetch_all(pool).await
            }
        }.unwrap_or_else(|e| {
            error!(table = self.table(), error = %e, "memory decay: failed to query for pruning");
            Vec::new()
        });

        if ids.is_empty() {
            return 0;
        }

        // Clean up graph edges and Qdrant vectors
        for id in &ids {
            if let Err(e) =
                sqlx::query("DELETE FROM graph_edges WHERE source_id = $1 OR target_id = $1")
                    .bind(id)
                    .execute(pool)
                    .await
            {
                warn!(id, error = %e, "memory decay: failed to delete graph edges");
            }
            for collection in ["alaz_text", "alaz_colbert"] {
                if let Err(e) = DenseVectorOps::delete_point(
                    qdrant.client(),
                    collection,
                    self.entity_type(),
                    id,
                )
                .await
                {
                    warn!(id, collection, error = %e, "memory decay: failed to delete vector");
                }
            }
        }

        // Bulk delete from DB
        let deleted = match self {
            Self::Knowledge => {
                sqlx::query("DELETE FROM knowledge_items WHERE utility_score < $1 AND created_at < now() - $2::interval")
                    .bind(MEMORY_PRUNE_THRESHOLD).bind(min_age).execute(pool).await
            }
            Self::Episode => {
                sqlx::query("DELETE FROM episodes WHERE utility_score < $1 AND created_at < now() - $2::interval")
                    .bind(MEMORY_PRUNE_THRESHOLD).bind(min_age).execute(pool).await
            }
            Self::Procedure => {
                sqlx::query("DELETE FROM procedures WHERE utility_score < $1 AND created_at < now() - $2::interval")
                    .bind(MEMORY_PRUNE_THRESHOLD).bind(min_age).execute(pool).await
            }
        };

        match deleted {
            Ok(r) => r.rows_affected(),
            Err(e) => {
                error!(table = self.table(), error = %e, "memory decay: failed to prune");
                0
            }
        }
    }

    /// Execute an UPDATE statement with optional bind parameters.
    ///
    /// The `clause` is a compile-time SQL fragment (e.g., `SET ... WHERE ...`)
    /// appended to `UPDATE <table>`. Table name comes from `self.table()` which
    /// is always a compile-time constant.
    async fn execute_update(
        self,
        pool: &PgPool,
        clause: &str,
        bind1: Option<f64>,
        bind2: Option<f64>,
    ) -> u64 {
        // We match on self to produce compile-time SQL strings per variant.
        // This avoids `format!()` interpolation entirely for the table name.
        let result = match (self, bind1, bind2) {
            (Self::Knowledge, Some(b1), Some(b2)) => {
                let sql = concat_update("knowledge_items", clause);
                sqlx::query(&sql).bind(b1).bind(b2).execute(pool).await
            }
            (Self::Episode, Some(b1), Some(b2)) => {
                let sql = concat_update("episodes", clause);
                sqlx::query(&sql).bind(b1).bind(b2).execute(pool).await
            }
            (Self::Procedure, Some(b1), Some(b2)) => {
                let sql = concat_update("procedures", clause);
                sqlx::query(&sql).bind(b1).bind(b2).execute(pool).await
            }
            (Self::Knowledge, None, None) => {
                let sql = concat_update("knowledge_items", clause);
                sqlx::query(&sql).execute(pool).await
            }
            (Self::Episode, None, None) => {
                let sql = concat_update("episodes", clause);
                sqlx::query(&sql).execute(pool).await
            }
            (Self::Procedure, None, None) => {
                let sql = concat_update("procedures", clause);
                sqlx::query(&sql).execute(pool).await
            }
            _ => return 0,
        };

        match result {
            Ok(r) => r.rows_affected(),
            Err(e) => {
                error!(table = self.table(), error = %e, "memory decay: update failed");
                0
            }
        }
    }
}

/// Build an `UPDATE <table> <clause>` SQL string.
///
/// `table` MUST be a compile-time constant from [`DecayableEntity::table`].
fn concat_update(table: &str, clause: &str) -> String {
    format!("UPDATE {table} {clause}")
}

/// Background job: every 6 hours, applies time-based utility decay to entities.
///
/// Items not accessed in 7 days lose utility; recently accessed items get a boost.
/// Old items below the prune threshold are deleted with their graph edges and vectors.
pub async fn memory_decay_job(pool: PgPool, qdrant: Arc<QdrantManager>, metrics: SharedMetrics) {
    let mut interval = tokio::time::interval(MEMORY_DECAY_INTERVAL);

    loop {
        interval.tick().await;
        info!("memory decay: starting cycle");

        let mut decayed = 0u64;
        let mut boosted = 0u64;
        let mut pruned = 0u64;

        let min_age = format!("{MIN_AGE_FOR_PRUNING_SECS} seconds");

        for &entity in DecayableEntity::ALL {
            decayed += entity.decay(&pool).await;
            boosted += entity.boost(&pool).await;
            pruned += entity.prune(&pool, &qdrant, &min_age).await;
        }

        metrics.decay_pruned.fetch_add(pruned, Ordering::Relaxed);
        info!(decayed, boosted, pruned, "memory decay: cycle complete");
    }
}

// ---------------------------------------------------------------------------
// Feedback aggregation job
// ---------------------------------------------------------------------------

/// Background job: every 7 days, learns optimal signal weights from click-through
/// data and updates the `signal_weights` table.
///
/// Each query type gets its own set of weights. Weights are smoothed with EMA
/// to prevent sudden shifts from noisy data.
pub async fn weight_learning_job(pool: PgPool) {
    let mut interval = tokio::time::interval(WEIGHT_LEARNING_INTERVAL);

    loop {
        interval.tick().await;
        info!("weight learning: starting cycle");

        match alaz_search::weight_learning::learn_weights(&pool).await {
            Ok(updated) => {
                info!(updated, "weight learning: cycle complete");
            }
            Err(e) => {
                error!(error = %e, "weight learning: failed");
            }
        }
    }
}

/// Background job: every 7 days, merges clusters of similar knowledge items
/// to prevent noise accumulation.
///
/// Groups items by vector similarity, generates LLM-powered merged summaries,
/// and supersedes the originals with a single consolidated item.
pub async fn consolidation_job(
    pool: PgPool,
    llm: Arc<alaz_intel::LlmClient>,
    embedding: Arc<alaz_intel::EmbeddingService>,
    qdrant: Arc<QdrantManager>,
    metrics: SharedMetrics,
) {
    let mut interval = tokio::time::interval(CONSOLIDATION_INTERVAL);

    loop {
        interval.tick().await;
        info!("consolidation: starting cycle");

        let pipeline = alaz_intel::ConsolidationPipeline::new(
            pool.clone(),
            llm.clone(),
            embedding.clone(),
            qdrant.clone(),
        );

        match pipeline.run().await {
            Ok(report) => {
                metrics
                    .consolidation_merged
                    .fetch_add(report.clusters_merged as u64, Ordering::Relaxed);
                info!(
                    clusters_found = report.clusters_found,
                    clusters_merged = report.clusters_merged,
                    items_superseded = report.items_superseded,
                    clusters_skipped = report.clusters_skipped,
                    "consolidation: cycle complete"
                );
            }
            Err(e) => {
                error!(error = %e, "consolidation: failed");
            }
        }
    }
}

/// Background job: every 12 hours, aggregates search feedback (click-through rates)
/// and updates feedback_boost on entities.
pub async fn feedback_aggregation_job(pool: PgPool) {
    let mut interval = tokio::time::interval(FEEDBACK_AGGREGATION_INTERVAL);

    loop {
        interval.tick().await;
        info!("feedback aggregation: starting cycle");

        match SearchQueryRepo::aggregate_feedback(&pool).await {
            Ok(updated) => {
                info!(updated, "feedback aggregation: cycle complete");
            }
            Err(e) => {
                error!(error = %e, "feedback aggregation: failed");
            }
        }
    }
}
