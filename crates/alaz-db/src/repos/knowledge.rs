use alaz_core::models::{CreateKnowledge, KnowledgeItem, ListKnowledgeFilter, UpdateKnowledge};
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

/// Standard SELECT columns for reading a KnowledgeItem.
/// `type` is aliased to `kind` to match the Rust struct field.
const KNOWLEDGE_COLUMNS: &str = "\
    id, title, content, description, type AS kind, language, file_path, project_id, \
    tags, utility_score, access_count, last_accessed_at, needs_embedding, feedback_boost, \
    valid_from, valid_until, superseded_by, invalidation_reason, source, source_metadata, \
    times_used, times_success, pattern_score, created_at, updated_at";

/// Build a `SELECT <columns> FROM knowledge_items <suffix>` query.
fn select_knowledge(suffix: &str) -> String {
    format!("SELECT {KNOWLEDGE_COLUMNS} FROM knowledge_items {suffix}")
}

pub struct KnowledgeRepo;

impl KnowledgeRepo {
    pub async fn create(
        pool: &PgPool,
        input: &CreateKnowledge,
        project_id: Option<&str>,
    ) -> Result<KnowledgeItem> {
        let id = cuid2::create_id();
        let kind = input.kind.as_deref().unwrap_or("artifact");
        let tags = input.tags.as_deref().unwrap_or(&[]);

        let sql = format!(
            "INSERT INTO knowledge_items (id, title, content, description, type, language, file_path, project_id, tags, valid_from, valid_until, source, source_metadata) \
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
            RETURNING {KNOWLEDGE_COLUMNS}"
        );
        let row = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(&id)
            .bind(&input.title)
            .bind(&input.content)
            .bind(&input.description)
            .bind(kind)
            .bind(&input.language)
            .bind(&input.file_path)
            .bind(project_id)
            .bind(tags)
            .bind(input.valid_from)
            .bind(input.valid_until)
            .bind(input.source.as_deref().unwrap_or("pi"))
            .bind(
                input
                    .source_metadata
                    .as_ref()
                    .unwrap_or(&serde_json::json!({})),
            )
            .fetch_one(pool)
            .await?;

        Ok(row)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<KnowledgeItem> {
        let sql = format!(
            "UPDATE knowledge_items \
            SET access_count = access_count + 1, last_accessed_at = now() \
            WHERE id = $1 \
            RETURNING {KNOWLEDGE_COLUMNS}"
        );
        let row = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AlazError::NotFound(format!("knowledge item {id}")))?;

        Ok(row)
    }

    /// Fetch a knowledge item by ID without updating access stats.
    /// Use this for internal/read-only operations that should not affect decay scoring.
    pub async fn get_readonly(pool: &PgPool, id: &str) -> Result<KnowledgeItem> {
        let sql = select_knowledge("WHERE id = $1");
        let row = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AlazError::NotFound(format!("knowledge item {id}")))?;
        Ok(row)
    }

    pub async fn update(pool: &PgPool, id: &str, input: &UpdateKnowledge) -> Result<KnowledgeItem> {
        let sql = format!(
            "UPDATE knowledge_items SET \
                title = COALESCE($2, title), \
                content = COALESCE($3, content), \
                description = COALESCE($4, description), \
                type = COALESCE($5, type), \
                language = COALESCE($6, language), \
                file_path = COALESCE($7, file_path), \
                tags = COALESCE($8, tags), \
                valid_from = COALESCE($9, valid_from), \
                valid_until = COALESCE($10, valid_until), \
                superseded_by = COALESCE($11, superseded_by), \
                needs_embedding = TRUE, \
                updated_at = now() \
            WHERE id = $1 \
            RETURNING {KNOWLEDGE_COLUMNS}"
        );
        let row = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(id)
            .bind(&input.title)
            .bind(&input.content)
            .bind(&input.description)
            .bind(&input.kind)
            .bind(&input.language)
            .bind(&input.file_path)
            .bind(&input.tags)
            .bind(input.valid_from)
            .bind(input.valid_until)
            .bind(&input.superseded_by)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AlazError::NotFound(format!("knowledge item {id}")))?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        // Clean up dangling graph edges before deleting the entity
        sqlx::query("DELETE FROM graph_edges WHERE source_id = $1 OR target_id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        let result = sqlx::query("DELETE FROM knowledge_items WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("knowledge item {id}")));
        }
        Ok(())
    }

    /// Fetch multiple knowledge items by IDs in a single query.
    pub async fn get_many(pool: &PgPool, ids: &[String]) -> Result<Vec<KnowledgeItem>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let sql = select_knowledge("WHERE id = ANY($1)");
        let rows = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(ids)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    pub async fn list(pool: &PgPool, filter: &ListKnowledgeFilter) -> Result<Vec<KnowledgeItem>> {
        let limit = filter.limit.unwrap_or(20);
        let offset = filter.offset.unwrap_or(0);

        let sql = select_knowledge(
            "WHERE ($1::TEXT IS NULL OR project_id = $1) \
              AND ($2::TEXT IS NULL OR type = $2) \
              AND ($3::TEXT IS NULL OR language = $3) \
              AND ($4::TEXT IS NULL OR $4 = ANY(tags)) \
            ORDER BY updated_at DESC \
            LIMIT $5 OFFSET $6",
        );
        let rows = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(&filter.project)
            .bind(&filter.kind)
            .bind(&filter.language)
            .bind(&filter.tag)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Full-text search. Returns (id, title, rank) tuples.
    pub async fn fts_search(
        pool: &PgPool,
        query: &str,
        project: Option<&str>,
        limit: i64,
    ) -> Result<Vec<(String, String, f32)>> {
        let rows = sqlx::query_as::<_, (String, String, f32)>(
            r#"
            SELECT id, title, ts_rank(search_vector, websearch_to_tsquery('simple', $1))::REAL AS rank
            FROM knowledge_items
            WHERE search_vector @@ websearch_to_tsquery('simple', $1)
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY rank DESC
            LIMIT $3
            "#,
        )
        .bind(query)
        .bind(project)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn find_needing_embedding(pool: &PgPool, limit: i64) -> Result<Vec<KnowledgeItem>> {
        let sql = select_knowledge("WHERE needs_embedding = TRUE ORDER BY created_at ASC LIMIT $1");
        let rows = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(limit)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    pub async fn mark_embedded(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("UPDATE knowledge_items SET needs_embedding = FALSE WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Find knowledge items with similar titles using trigram similarity.
    ///
    /// When `project_id` is `Some`, only items in that project are considered.
    /// When `None`, all items are searched.
    pub async fn find_similar_by_title(
        pool: &PgPool,
        title: &str,
        threshold: f32,
        project_id: Option<&str>,
    ) -> Result<Vec<KnowledgeItem>> {
        let sql = select_knowledge(
            "WHERE similarity(title, $1) > $2 \
              AND ($3::TEXT IS NULL OR project_id = $3) \
            ORDER BY similarity(title, $1) DESC \
            LIMIT 5",
        );
        let rows = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(title)
            .bind(threshold)
            .bind(project_id)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// List only truly global knowledge items (project_id IS NULL) of a given type.
    pub async fn list_global(pool: &PgPool, kind: &str, limit: i64) -> Result<Vec<KnowledgeItem>> {
        let sql = select_knowledge(
            "WHERE project_id IS NULL AND type = $1 \
            ORDER BY updated_at DESC \
            LIMIT $2",
        );
        let rows = sqlx::query_as::<_, KnowledgeItem>(&sql)
            .bind(kind)
            .bind(limit)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Record an access event for a knowledge item (increment count, update timestamp).
    ///
    /// Returns [`AlazError::NotFound`] if the item does not exist.
    pub async fn record_access(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE knowledge_items SET access_count = access_count + 1, last_accessed_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("knowledge item {id}")));
        }
        Ok(())
    }

    pub async fn supersede(
        pool: &PgPool,
        old_id: &str,
        new_id: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE knowledge_items
            SET superseded_by = $2,
                invalidation_reason = $3,
                valid_until = now(),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(old_id)
        .bind(new_id)
        .bind(reason)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Bulk delete knowledge items by IDs. Returns the number of rows deleted.
    pub async fn bulk_delete(pool: &PgPool, ids: &[String]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let result = sqlx::query("DELETE FROM knowledge_items WHERE id = ANY($1)")
            .bind(ids)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Record a pattern usage event. Increments times_used, and optionally times_success.
    pub async fn record_usage(pool: &PgPool, id: &str, success: bool) -> Result<()> {
        let query = if success {
            "UPDATE knowledge_items SET times_used = times_used + 1, times_success = times_success + 1 WHERE id = $1"
        } else {
            "UPDATE knowledge_items SET times_used = times_used + 1 WHERE id = $1"
        };
        let result = sqlx::query(query).bind(id).execute(pool).await?;
        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("knowledge item {id}")));
        }
        Ok(())
    }

    /// Record explicit usage of a knowledge item with outcome tracking.
    ///
    /// `outcome` must be one of `"success"`, `"failure"`, or `"partial"`.
    /// - `"success"` increments `times_success` by 1
    /// - `"partial"` increments `times_success` by 1 (rounded from 0.5 for integer column)
    /// - `"failure"` does not increment `times_success`
    ///
    /// All outcomes increment `times_used`, update `last_accessed_at`, and
    /// nudge `utility_score` up (capped at 1.0).
    pub async fn record_usage_with_outcome(
        pool: &PgPool,
        id: &str,
        outcome: &str,
        context: Option<&str>,
    ) -> Result<()> {
        // Determine success increment: 1 for success, 0 for failure.
        // For "partial", we use a two-step approach: first record the usage,
        // then apply a fractional adjustment via float math on utility_score.
        let success_inc: i64 = match outcome {
            "success" => 1,
            _ => 0, // "failure" and "partial"
        };

        let result = sqlx::query(
            r#"
            UPDATE knowledge_items SET
                times_used = times_used + 1,
                times_success = times_success + $2,
                last_accessed_at = now(),
                utility_score = LEAST(
                    CASE
                        WHEN $3 = 'failure' THEN utility_score + 0.01
                        WHEN $3 = 'partial' THEN utility_score + 0.03
                        ELSE utility_score + 0.05
                    END,
                    1.0
                ),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(success_inc)
        .bind(outcome)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("knowledge item {id}")));
        }

        // Log context if provided (stored in source_metadata for audit trail)
        if let Some(ctx) = context {
            sqlx::query(
                r#"
                UPDATE knowledge_items
                SET source_metadata = jsonb_set(
                    COALESCE(source_metadata, '{}'::jsonb),
                    '{last_usage_context}',
                    to_jsonb($2::TEXT)
                )
                WHERE id = $1
                "#,
            )
            .bind(id)
            .bind(ctx)
            .execute(pool)
            .await?;
        }

        Ok(())
    }
}
