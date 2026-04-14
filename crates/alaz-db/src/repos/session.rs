use alaz_core::models::{ListSessionsFilter, SessionCheckpoint, SessionLog};
use alaz_core::{AlazError, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Result row from full-text search across session transcripts.
#[derive(Debug, sqlx::FromRow)]
pub struct SessionSearchResult {
    pub id: String,
    pub project_id: Option<String>,
    pub summary: Option<String>,
    pub headline: String,
    pub rank: f32,
    pub created_at: DateTime<Utc>,
}

pub struct SessionRepo;

impl SessionRepo {
    pub async fn create(pool: &PgPool, project_id: Option<&str>) -> Result<SessionLog> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, SessionLog>(
            r#"
            INSERT INTO session_logs (id, project_id)
            VALUES ($1, $2)
            RETURNING id, project_id, cost, input_tokens, output_tokens,
                      duration_seconds, tools_used, status, summary,
                      created_at, updated_at
            "#,
        )
        .bind(&id)
        .bind(project_id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Ensure a session exists with the given ID (e.g. Claude Code session_id).
    /// Creates it if missing, returns existing if found.
    pub async fn ensure_exists(
        pool: &PgPool,
        id: &str,
        project_id: Option<&str>,
    ) -> Result<SessionLog> {
        let row = sqlx::query_as::<_, SessionLog>(
            r#"
            INSERT INTO session_logs (id, project_id, status)
            VALUES ($1, $2, 'started')
            ON CONFLICT (id) DO UPDATE SET updated_at = now()
            RETURNING id, project_id, cost, input_tokens, output_tokens,
                      duration_seconds, tools_used, status, summary,
                      created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(project_id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<SessionLog> {
        let row = sqlx::query_as::<_, SessionLog>(
            r#"
            SELECT id, project_id, cost, input_tokens, output_tokens,
                   duration_seconds, tools_used, status, summary,
                   created_at, updated_at
            FROM session_logs WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("session {id}")))?;

        Ok(row)
    }

    pub async fn update_status(pool: &PgPool, id: &str, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE session_logs
            SET status = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_summary(
        pool: &PgPool,
        id: &str,
        summary: &str,
        cost: Option<f64>,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        duration_seconds: Option<f64>,
        tools_used: Option<&serde_json::Value>,
    ) -> Result<SessionLog> {
        let row = sqlx::query_as::<_, SessionLog>(
            r#"
            UPDATE session_logs SET
                summary = $2,
                cost = COALESCE($3, cost),
                input_tokens = COALESCE($4, input_tokens),
                output_tokens = COALESCE($5, output_tokens),
                duration_seconds = COALESCE($6, duration_seconds),
                tools_used = COALESCE($7, tools_used),
                updated_at = now()
            WHERE id = $1
            RETURNING id, project_id, cost, input_tokens, output_tokens,
                      duration_seconds, tools_used, status, summary,
                      created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(summary)
        .bind(cost)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(duration_seconds)
        .bind(tools_used)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("session {id}")))?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM session_logs WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    pub async fn list(pool: &PgPool, filter: &ListSessionsFilter) -> Result<Vec<SessionLog>> {
        let limit = filter.limit.unwrap_or(20);
        let offset = filter.offset.unwrap_or(0);

        let rows = sqlx::query_as::<_, SessionLog>(
            r#"
            SELECT id, project_id, cost, input_tokens, output_tokens,
                   duration_seconds, tools_used, status, summary,
                   created_at, updated_at
            FROM session_logs
            WHERE ($1::TEXT IS NULL OR project_id = $1)
              AND ($2::TEXT IS NULL OR status = $2)
            ORDER BY created_at DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(&filter.project)
        .bind(&filter.status)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// List sessions within a date range, ordered by created_at DESC.
    pub async fn list_in_range(
        pool: &PgPool,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        project_id: Option<&str>,
    ) -> Result<Vec<SessionLog>> {
        let rows = sqlx::query_as::<_, SessionLog>(
            r#"
            SELECT id, project_id, cost, input_tokens, output_tokens,
                   duration_seconds, tools_used, status, summary,
                   created_at, updated_at
            FROM session_logs
            WHERE created_at >= $1 AND created_at <= $2
              AND ($3::TEXT IS NULL OR project_id = $3)
            ORDER BY created_at DESC
            "#,
        )
        .bind(start)
        .bind(end)
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    // --- Checkpoint methods ---

    pub async fn save_checkpoint(
        pool: &PgPool,
        session_id: &str,
        data: &serde_json::Value,
    ) -> Result<SessionCheckpoint> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, SessionCheckpoint>(
            r#"
            INSERT INTO session_checkpoints (id, session_id, checkpoint_data)
            VALUES ($1, $2, $3)
            RETURNING id, session_id, checkpoint_data, created_at
            "#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(data)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn get_checkpoints(
        pool: &PgPool,
        session_id: &str,
    ) -> Result<Vec<SessionCheckpoint>> {
        let rows = sqlx::query_as::<_, SessionCheckpoint>(
            r#"
            SELECT id, session_id, checkpoint_data, created_at
            FROM session_checkpoints
            WHERE session_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(session_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn get_latest_checkpoint(
        pool: &PgPool,
        session_id: &str,
    ) -> Result<Option<SessionCheckpoint>> {
        let row = sqlx::query_as::<_, SessionCheckpoint>(
            r#"
            SELECT id, session_id, checkpoint_data, created_at
            FROM session_checkpoints
            WHERE session_id = $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(session_id)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn delete_checkpoint(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM session_checkpoints WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("checkpoint {id}")));
        }
        Ok(())
    }

    /// Check if a session exists in the session_logs table.
    pub async fn exists(pool: &PgPool, session_id: &str) -> Result<bool> {
        let row = sqlx::query("SELECT EXISTS(SELECT 1 FROM session_logs WHERE id = $1) AS e")
            .bind(session_id)
            .fetch_one(pool)
            .await?;
        Ok(sqlx::Row::get::<bool, _>(&row, "e"))
    }

    /// Full-text search across session transcripts.
    pub async fn search_transcripts(
        pool: &PgPool,
        query: &str,
        project: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SessionSearchResult>> {
        let rows = sqlx::query_as::<_, SessionSearchResult>(
            r#"
            SELECT id, project_id, summary,
                   ts_headline('english', COALESCE(transcript_text, ''),
                               websearch_to_tsquery('english', $1),
                               'StartSel=<<, StopSel=>>, MaxFragments=3, MaxWords=50') AS headline,
                   ts_rank(search_vector, websearch_to_tsquery('english', $1))::REAL AS rank,
                   created_at
            FROM session_logs
            WHERE search_vector @@ websearch_to_tsquery('english', $1)
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

    /// Update session with transcript text for FTS indexing.
    pub async fn update_transcript_text(
        pool: &PgPool,
        session_id: &str,
        text: &str,
        summary: Option<&str>,
        message_count: i32,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE session_logs
            SET transcript_text = $2,
                summary = COALESCE($3, summary),
                message_count = $4,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(session_id)
        .bind(text)
        .bind(summary)
        .bind(message_count)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("session {session_id}")));
        }
        Ok(())
    }

    /// Get the session's created_at as a unix epoch (for git ingestion time filtering).
    pub async fn get_session_start_epoch(pool: &PgPool, session_id: &str) -> Result<i64> {
        let ts: (DateTime<Utc>,) =
            sqlx::query_as("SELECT created_at FROM session_logs WHERE id = $1")
                .bind(session_id)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| AlazError::NotFound(format!("session {session_id}")))?;
        Ok(ts.0.timestamp())
    }

    // --- Structured Session State ---

    #[allow(clippy::too_many_arguments)]
    pub async fn update_session_state(
        pool: &PgPool,
        session_id: &str,
        goals: Option<&[String]>,
        accomplished: Option<&[String]>,
        pending: Option<&[String]>,
        handoff_summary: Option<&str>,
        current_task: Option<&str>,
        related_files: Option<&[String]>,
        working_context: Option<&serde_json::Value>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE session_logs SET
                goals = COALESCE($2, goals),
                accomplished = COALESCE($3, accomplished),
                pending = COALESCE($4, pending),
                handoff_summary = COALESCE($5, handoff_summary),
                current_task = COALESCE($6, current_task),
                related_files = COALESCE($7, related_files),
                working_context = COALESCE($8, working_context),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(session_id)
        .bind(goals)
        .bind(accomplished)
        .bind(pending)
        .bind(handoff_summary)
        .bind(current_task)
        .bind(related_files)
        .bind(working_context)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_session_state(pool: &PgPool, session_id: &str) -> Result<SessionState> {
        sqlx::query_as::<_, SessionState>(
            "SELECT id, goals, accomplished, pending, handoff_summary, \
             current_task, related_files, working_context, work_unit_id \
             FROM session_logs WHERE id = $1",
        )
        .bind(session_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("session {session_id}")))
    }
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SessionState {
    pub id: String,
    pub goals: Option<Vec<String>>,
    pub accomplished: Option<Vec<String>>,
    pub pending: Option<Vec<String>>,
    pub handoff_summary: Option<String>,
    pub current_task: Option<String>,
    pub related_files: Option<Vec<String>>,
    pub working_context: Option<serde_json::Value>,
    pub work_unit_id: Option<String>,
}
