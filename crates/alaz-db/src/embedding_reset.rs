use alaz_core::Result;
use sqlx::PgPool;
use tracing::info;

/// Reset embedding flags for all items across all entity tables.
///
/// This sets `needs_embedding = TRUE` for every row in knowledge_items,
/// episodes, procedures, and core_memories. Used when the embedding model
/// or dimension changes and all vectors need to be regenerated.
pub async fn reset_all_embeddings(pool: &PgPool) -> Result<()> {
    let ki = sqlx::query("UPDATE knowledge_items SET needs_embedding = TRUE")
        .execute(pool)
        .await?;
    let ep = sqlx::query("UPDATE episodes SET needs_embedding = TRUE")
        .execute(pool)
        .await?;
    let pr = sqlx::query("UPDATE procedures SET needs_embedding = TRUE")
        .execute(pool)
        .await?;
    let cm = sqlx::query("UPDATE core_memories SET needs_embedding = TRUE")
        .execute(pool)
        .await?;

    info!(
        knowledge_items = ki.rows_affected(),
        episodes = ep.rows_affected(),
        procedures = pr.rows_affected(),
        core_memories = cm.rows_affected(),
        "reset all embeddings — items will be re-embedded by the backfill job"
    );

    Ok(())
}
