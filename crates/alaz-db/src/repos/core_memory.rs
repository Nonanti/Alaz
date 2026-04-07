use alaz_core::models::{CoreMemory, ListCoreMemoryFilter, UpsertCoreMemory};
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

pub struct CoreMemoryRepo;

impl CoreMemoryRepo {
    pub async fn upsert(
        pool: &PgPool,
        input: &UpsertCoreMemory,
        project_id: Option<&str>,
    ) -> Result<CoreMemory> {
        let id = cuid2::create_id();
        let confidence = input.confidence.unwrap_or(1.0);

        // PostgreSQL UNIQUE constraints treat NULLs as distinct, so we need
        // different ON CONFLICT targets depending on whether project_id is NULL.
        let row = if project_id.is_some() {
            sqlx::query_as::<_, CoreMemory>(
                r#"
                INSERT INTO core_memories (id, category, key, value, confidence, project_id)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (category, key, project_id) DO UPDATE SET
                    value = EXCLUDED.value,
                    confidence = EXCLUDED.confidence,
                    confirmations = core_memories.confirmations + 1,
                    updated_at = now()
                RETURNING id, category, key, value, confidence, confirmations, contradictions,
                          project_id, needs_embedding, created_at, updated_at
                "#,
            )
            .bind(&id)
            .bind(&input.category)
            .bind(&input.key)
            .bind(&input.value)
            .bind(confidence)
            .bind(project_id)
            .fetch_one(pool)
            .await?
        } else {
            sqlx::query_as::<_, CoreMemory>(
                r#"
                INSERT INTO core_memories (id, category, key, value, confidence, project_id)
                VALUES ($1, $2, $3, $4, $5, NULL)
                ON CONFLICT (category, key) WHERE project_id IS NULL DO UPDATE SET
                    value = EXCLUDED.value,
                    confidence = EXCLUDED.confidence,
                    confirmations = core_memories.confirmations + 1,
                    updated_at = now()
                RETURNING id, category, key, value, confidence, confirmations, contradictions,
                          project_id, needs_embedding, created_at, updated_at
                "#,
            )
            .bind(&id)
            .bind(&input.category)
            .bind(&input.key)
            .bind(&input.value)
            .bind(confidence)
            .fetch_one(pool)
            .await?
        };

        Ok(row)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<CoreMemory> {
        let row = sqlx::query_as::<_, CoreMemory>(
            r#"
            SELECT id, category, key, value, confidence, confirmations, contradictions,
                   project_id, needs_embedding, created_at, updated_at
            FROM core_memories WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("core memory {id}")))?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM core_memories WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("core memory {id}")));
        }
        Ok(())
    }

    pub async fn list(pool: &PgPool, filter: &ListCoreMemoryFilter) -> Result<Vec<CoreMemory>> {
        let limit = filter.limit.unwrap_or(50);
        let offset = filter.offset.unwrap_or(0);

        let rows = sqlx::query_as::<_, CoreMemory>(
            r#"
            SELECT id, category, key, value, confidence, confirmations, contradictions,
                   project_id, needs_embedding, created_at, updated_at
            FROM core_memories
            WHERE ($1::TEXT IS NULL OR project_id = $1)
              AND ($2::TEXT IS NULL OR category = $2)
            ORDER BY updated_at DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(&filter.project)
        .bind(&filter.category)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// List only truly global core memories (project_id IS NULL).
    pub async fn list_global(pool: &PgPool, limit: i64) -> Result<Vec<CoreMemory>> {
        let rows = sqlx::query_as::<_, CoreMemory>(
            r#"
            SELECT id, category, key, value, confidence, confirmations, contradictions,
                   project_id, needs_embedding, created_at, updated_at
            FROM core_memories
            WHERE project_id IS NULL
            ORDER BY updated_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Find core memories with similar keys using trigram similarity.
    /// Scoped to same category and project.
    pub async fn find_similar_by_key(
        pool: &PgPool,
        category: &str,
        key: &str,
        threshold: f32,
        project_id: Option<&str>,
    ) -> Result<Vec<CoreMemory>> {
        let rows = if project_id.is_some() {
            sqlx::query_as::<_, CoreMemory>(
                r#"
                SELECT id, category, key, value, confidence, confirmations, contradictions,
                       project_id, needs_embedding, created_at, updated_at
                FROM core_memories
                WHERE category = $1
                  AND similarity(key, $2) > $3
                  AND project_id = $4
                ORDER BY similarity(key, $2) DESC
                LIMIT 5
                "#,
            )
            .bind(category)
            .bind(key)
            .bind(threshold)
            .bind(project_id)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, CoreMemory>(
                r#"
                SELECT id, category, key, value, confidence, confirmations, contradictions,
                       project_id, needs_embedding, created_at, updated_at
                FROM core_memories
                WHERE category = $1
                  AND similarity(key, $2) > $3
                  AND project_id IS NULL
                ORDER BY similarity(key, $2) DESC
                LIMIT 5
                "#,
            )
            .bind(category)
            .bind(key)
            .bind(threshold)
            .fetch_all(pool)
            .await?
        };

        Ok(rows)
    }

    pub async fn find_needing_embedding(pool: &PgPool, limit: i64) -> Result<Vec<CoreMemory>> {
        let rows = sqlx::query_as::<_, CoreMemory>(
            r#"
            SELECT id, category, key, value, confidence, confirmations, contradictions,
                   project_id, needs_embedding, created_at, updated_at
            FROM core_memories
            WHERE needs_embedding = TRUE
            ORDER BY created_at ASC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn mark_embedded(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("UPDATE core_memories SET needs_embedding = FALSE WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn record_contradiction(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE core_memories
            SET contradictions = contradictions + 1,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }
}
