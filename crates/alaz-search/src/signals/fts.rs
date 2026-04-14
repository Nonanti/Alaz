//! Full-text search signal.
//!
//! Searches knowledge_items, episodes, and procedures via their tsvector columns
//! using `websearch_to_tsquery('english', $1)`.

use alaz_core::Result;
use alaz_core::traits::SignalResult;
use sqlx::PgPool;
use tracing::debug;

/// Execute FTS across all three entity tables and return merged results.
///
/// Uses a single UNION ALL query for efficiency instead of 3 separate queries.
pub async fn execute(
    pool: &PgPool,
    query: &str,
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<SignalResult>> {
    let sql_limit = limit as i64;

    let rows = sqlx::query_as::<_, (String, String, f32)>(
        r#"
        (
            SELECT 'knowledge_item'::TEXT AS entity_type, id,
                   ts_rank(search_vector, websearch_to_tsquery('english', $1))::REAL AS rank
            FROM knowledge_items
            WHERE search_vector @@ websearch_to_tsquery('english', $1)
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY rank DESC
            LIMIT $3
        )
        UNION ALL
        (
            SELECT 'episode'::TEXT AS entity_type, id,
                   ts_rank(search_vector, websearch_to_tsquery('english', $1))::REAL AS rank
            FROM episodes
            WHERE search_vector @@ websearch_to_tsquery('english', $1)
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY rank DESC
            LIMIT $3
        )
        UNION ALL
        (
            SELECT 'procedure'::TEXT AS entity_type, id,
                   ts_rank(search_vector, websearch_to_tsquery('english', $1))::REAL AS rank
            FROM procedures
            WHERE search_vector @@ websearch_to_tsquery('english', $1)
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY rank DESC
            LIMIT $3
        )
        ORDER BY rank DESC
        LIMIT $3
        "#,
    )
    .bind(query)
    .bind(project)
    .bind(sql_limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let results: Vec<SignalResult> = rows
        .into_iter()
        .enumerate()
        .map(|(rank, (entity_type, entity_id, _score))| SignalResult {
            entity_type,
            entity_id,
            rank,
        })
        .collect();

    debug!(
        query = %query,
        count = results.len(),
        "FTS signal complete"
    );

    Ok(results)
}
