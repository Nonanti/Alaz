use alaz_core::models::{CreateProcedure, ListProceduresFilter, Procedure};
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

const PROCEDURE_COLUMNS: &str = "\
    id, title, content, steps, times_used, \
    times_success AS success, times_failure AS failure, \
    success_rate, project_id, tags, utility_score, \
    access_count, last_accessed_at, needs_embedding, feedback_boost, \
    superseded_by, valid_from, valid_until, source, source_metadata, \
    created_at, updated_at";

fn select_procedures(suffix: &str) -> String {
    format!("SELECT {PROCEDURE_COLUMNS} FROM procedures {suffix}")
}

pub struct ProcedureRepo;

impl ProcedureRepo {
    pub async fn create(
        pool: &PgPool,
        input: &CreateProcedure,
        project_id: Option<&str>,
    ) -> Result<Procedure> {
        let id = cuid2::create_id();
        let steps = input
            .steps
            .as_ref()
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));
        let tags = input.tags.as_deref().unwrap_or(&[]);

        let query = format!(
            "INSERT INTO procedures (id, title, content, steps, project_id, tags, source, source_metadata) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING {PROCEDURE_COLUMNS}"
        );

        let row = sqlx::query_as::<_, Procedure>(&query)
            .bind(&id)
            .bind(&input.title)
            .bind(&input.content)
            .bind(&steps)
            .bind(project_id)
            .bind(tags)
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

    pub async fn get(pool: &PgPool, id: &str) -> Result<Procedure> {
        let row = sqlx::query_as::<_, Procedure>(&select_procedures("WHERE id = $1"))
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AlazError::NotFound(format!("procedure {id}")))?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM graph_edges WHERE source_id = $1 OR target_id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        let result = sqlx::query("DELETE FROM procedures WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("procedure {id}")));
        }
        Ok(())
    }

    pub async fn list(pool: &PgPool, filter: &ListProceduresFilter) -> Result<Vec<Procedure>> {
        let limit = filter.limit.unwrap_or(20);
        let offset = filter.offset.unwrap_or(0);

        let rows = sqlx::query_as::<_, Procedure>(&select_procedures(
            "WHERE ($1::TEXT IS NULL OR project_id = $1) \
               AND ($2::TEXT IS NULL OR $2 = ANY(tags)) \
             ORDER BY updated_at DESC \
             LIMIT $3 OFFSET $4",
        ))
        .bind(&filter.project)
        .bind(&filter.tag)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Fetch multiple procedures by IDs in a single query.
    pub async fn get_many(pool: &PgPool, ids: &[String]) -> Result<Vec<Procedure>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let rows = sqlx::query_as::<_, Procedure>(&select_procedures("WHERE id = ANY($1)"))
            .bind(ids)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Record the outcome of a procedure execution.
    pub async fn record_outcome(pool: &PgPool, id: &str, success: bool) -> Result<()> {
        let query = if success {
            r#"
            UPDATE procedures
            SET times_used = times_used + 1,
                times_success = times_success + 1,
                updated_at = now()
            WHERE id = $1
            "#
        } else {
            r#"
            UPDATE procedures
            SET times_used = times_used + 1,
                times_failure = times_failure + 1,
                updated_at = now()
            WHERE id = $1
            "#
        };

        let result = sqlx::query(query).bind(id).execute(pool).await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("procedure {id}")));
        }
        Ok(())
    }

    /// Record an access event for a procedure (increment count, update timestamp).
    ///
    /// Returns [`AlazError::NotFound`] if the procedure does not exist.
    pub async fn record_access(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE procedures SET access_count = access_count + 1, last_accessed_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("procedure {id}")));
        }
        Ok(())
    }

    pub async fn find_needing_embedding(pool: &PgPool, limit: i64) -> Result<Vec<Procedure>> {
        let rows = sqlx::query_as::<_, Procedure>(&select_procedures(
            "WHERE needs_embedding = TRUE \
             ORDER BY created_at ASC \
             LIMIT $1",
        ))
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn mark_embedded(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("UPDATE procedures SET needs_embedding = FALSE WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Find procedures with similar titles using trigram similarity.
    pub async fn find_similar_by_title(
        pool: &PgPool,
        title: &str,
        threshold: f32,
        project_id: Option<&str>,
    ) -> Result<Vec<Procedure>> {
        let rows = sqlx::query_as::<_, Procedure>(&select_procedures(
            "WHERE similarity(title, $1) > $2 \
               AND ($3::TEXT IS NULL OR project_id = $3) \
             ORDER BY similarity(title, $1) DESC \
             LIMIT 5",
        ))
        .bind(title)
        .bind(threshold)
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Mark a procedure as superseded by a newer one.
    pub async fn supersede(pool: &PgPool, old_id: &str, new_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE procedures
            SET superseded_by = $2,
                valid_until = now(),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(old_id)
        .bind(new_id)
        .execute(pool)
        .await?;
        Ok(())
    }
}
