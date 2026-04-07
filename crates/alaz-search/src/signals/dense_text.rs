//! Dense text vector search signal.
//!
//! Searches the `alaz_text` collection via `DenseVectorOps::search_text`
//! and maps results to `SignalResult` with rank by score order.

use alaz_core::Result;
use alaz_core::traits::SignalResult;
use alaz_vector::{DenseVectorOps, QdrantManager};
use tracing::debug;

/// Execute dense text vector search.
pub async fn execute(
    qdrant: &QdrantManager,
    text_embedding: &[f32],
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<SignalResult>> {
    let results = DenseVectorOps::search_text(
        qdrant.client(),
        text_embedding.to_vec(),
        project,
        limit as u64,
    )
    .await?;

    let signal_results: Vec<SignalResult> = results
        .into_iter()
        .enumerate()
        .map(|(rank, (entity_type, entity_id, _score))| SignalResult {
            entity_type,
            entity_id,
            rank,
        })
        .collect();

    debug!(count = signal_results.len(), "dense text signal complete");

    Ok(signal_results)
}
