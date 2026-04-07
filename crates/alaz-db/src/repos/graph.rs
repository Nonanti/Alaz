use alaz_core::models::{CreateRelation, GraphEdge};
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

pub struct GraphRepo;

impl GraphRepo {
    pub async fn create_edge(pool: &PgPool, input: &CreateRelation) -> Result<GraphEdge> {
        let id = cuid2::create_id();
        let weight = input.weight.unwrap_or(1.0);
        let metadata = input
            .metadata
            .as_ref()
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        let row = sqlx::query_as::<_, GraphEdge>(
            r#"
            INSERT INTO graph_edges (id, source_type, source_id, target_type, target_id,
                                     relation, weight, description, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (source_type, source_id, target_type, target_id, relation) DO UPDATE SET
                weight = GREATEST(graph_edges.weight, EXCLUDED.weight),
                usage_count = graph_edges.usage_count + 1,
                last_used_at = now(),
                description = COALESCE(EXCLUDED.description, graph_edges.description),
                metadata = COALESCE(EXCLUDED.metadata, graph_edges.metadata)
            RETURNING id, source_type, source_id, target_type, target_id, relation,
                      weight, usage_count, description, metadata, created_at, last_used_at
            "#,
        )
        .bind(&id)
        .bind(&input.source_type)
        .bind(&input.source_id)
        .bind(&input.target_type)
        .bind(&input.target_id)
        .bind(&input.relation)
        .bind(weight)
        .bind(&input.description)
        .bind(&metadata)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn delete_edge(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM graph_edges WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("graph edge {id}")));
        }
        Ok(())
    }

    /// Get edges for an entity. Direction: "outgoing", "incoming", or "both".
    pub async fn get_edges(
        pool: &PgPool,
        entity_type: &str,
        entity_id: &str,
        direction: &str,
    ) -> Result<Vec<GraphEdge>> {
        let rows = match direction {
            "outgoing" => {
                sqlx::query_as::<_, GraphEdge>(
                    r#"
                    SELECT id, source_type, source_id, target_type, target_id, relation,
                           weight, usage_count, description, metadata, created_at, last_used_at
                    FROM graph_edges
                    WHERE source_type = $1 AND source_id = $2
                    ORDER BY weight DESC
                    "#,
                )
                .bind(entity_type)
                .bind(entity_id)
                .fetch_all(pool)
                .await?
            }
            "incoming" => {
                sqlx::query_as::<_, GraphEdge>(
                    r#"
                    SELECT id, source_type, source_id, target_type, target_id, relation,
                           weight, usage_count, description, metadata, created_at, last_used_at
                    FROM graph_edges
                    WHERE target_type = $1 AND target_id = $2
                    ORDER BY weight DESC
                    "#,
                )
                .bind(entity_type)
                .bind(entity_id)
                .fetch_all(pool)
                .await?
            }
            _ => {
                // "both"
                sqlx::query_as::<_, GraphEdge>(
                    r#"
                    SELECT id, source_type, source_id, target_type, target_id, relation,
                           weight, usage_count, description, metadata, created_at, last_used_at
                    FROM graph_edges
                    WHERE (source_type = $1 AND source_id = $2)
                       OR (target_type = $1 AND target_id = $2)
                    ORDER BY weight DESC
                    "#,
                )
                .bind(entity_type)
                .bind(entity_id)
                .fetch_all(pool)
                .await?
            }
        };

        Ok(rows)
    }

    /// Apply exponential decay to all edge weights and delete edges below threshold.
    /// Returns the number of deleted edges.
    pub async fn decay_weights(pool: &PgPool) -> Result<u64> {
        // Apply exponential decay (multiply by 0.95)
        sqlx::query("UPDATE graph_edges SET weight = weight * 0.95")
            .execute(pool)
            .await?;

        // Delete edges below threshold
        let result = sqlx::query("DELETE FROM graph_edges WHERE weight < 0.05")
            .execute(pool)
            .await?;

        Ok(result.rows_affected())
    }

    pub async fn get_edge(pool: &PgPool, id: &str) -> Result<GraphEdge> {
        let row = sqlx::query_as::<_, GraphEdge>(
            r#"
            SELECT id, source_type, source_id, target_type, target_id, relation,
                   weight, usage_count, description, metadata, created_at, last_used_at
            FROM graph_edges WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("graph edge {id}")))?;

        Ok(row)
    }
}
