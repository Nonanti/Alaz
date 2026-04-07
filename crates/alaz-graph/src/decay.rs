use alaz_core::Result;
use alaz_db::repos::GraphRepo;
use sqlx::PgPool;
use tracing::info;

/// Run the background decay job on graph edge weights.
///
/// Applies exponential decay (multiply by 0.95) to all edge weights,
/// then deletes edges that have decayed below 0.05.
///
/// Returns the number of deleted edges.
pub async fn run_decay(pool: &PgPool) -> Result<u64> {
    let deleted = GraphRepo::decay_weights(pool).await?;
    info!(deleted, "decay job completed");
    Ok(deleted)
}
