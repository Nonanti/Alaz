//! ColBERT multi-vector search signal.
//!
//! If the ColBERT service returns empty embeddings (stub), returns empty results
//! gracefully. Otherwise: embeds query tokens, searches `alaz_colbert`, and
//! applies MaxSim scoring via `ColbertOps::search_colbert`.

use alaz_core::Result;
use alaz_core::traits::SignalResult;
use alaz_intel::ColbertService;
use alaz_vector::{ColbertOps, QdrantManager};
use tracing::debug;

/// Execute ColBERT search signal.
pub async fn execute(
    qdrant: &QdrantManager,
    colbert: &ColbertService,
    query: &str,
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<SignalResult>> {
    // Embed query tokens via the ColBERT service
    let query_tokens = colbert.embed_query(query).await?;

    // If stub returns empty, degrade gracefully
    if query_tokens.is_empty() {
        debug!("ColBERT service returned empty tokens, skipping signal");
        return Ok(vec![]);
    }

    let results = ColbertOps::search_colbert(qdrant.client(), query_tokens, project, limit).await?;

    let signal_results: Vec<SignalResult> = results
        .into_iter()
        .enumerate()
        .map(|(rank, (entity_type, entity_id, _score))| SignalResult {
            entity_type,
            entity_id,
            rank,
        })
        .collect();

    debug!(count = signal_results.len(), "ColBERT signal complete");

    Ok(signal_results)
}
