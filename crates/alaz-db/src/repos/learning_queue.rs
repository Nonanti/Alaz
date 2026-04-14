use alaz_core::Result;
use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
pub struct LearningQueueItem {
    pub id: String,
    pub session_id: String,
    pub project_id: Option<String>,
    pub transcript: String,
    pub message_count: i32,
    pub retry_count: i32,
    pub queued_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
}

/// Maximum retries before marking as permanently failed.
pub const MAX_RETRIES: i32 = 3;

pub struct LearningQueueRepo;

impl LearningQueueRepo {
    /// Enqueue a learn request. If a pending request already exists for the same
    /// session, cancel the old one and insert the new one (latest transcript wins).
    pub async fn enqueue(
        pool: &PgPool,
        session_id: &str,
        project_id: Option<&str>,
        transcript: &str,
        message_count: i32,
    ) -> Result<String> {
        // Cancel any pending requests for the same session
        sqlx::query(
            "UPDATE learning_queue SET status = 'cancelled' \
             WHERE session_id = $1 AND status = 'pending'",
        )
        .bind(session_id)
        .execute(pool)
        .await?;

        // Insert new request
        let id = cuid2::create_id();
        sqlx::query(
            "INSERT INTO learning_queue (id, session_id, project_id, transcript, message_count) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(project_id)
        .bind(transcript)
        .bind(message_count)
        .execute(pool)
        .await?;

        Ok(id)
    }

    /// Fetch the next ready-to-process item: pending for at least `cooldown_secs`
    /// and no newer pending request for the same session.
    ///
    /// Atomically marks it as 'processing' to prevent double-pickup.
    pub async fn dequeue(pool: &PgPool, cooldown_secs: i64) -> Result<Option<LearningQueueItem>> {
        let item = sqlx::query_as::<_, LearningQueueItem>(
            r#"
            UPDATE learning_queue SET status = 'processing', started_at = now()
            WHERE id = (
                SELECT lq.id FROM learning_queue lq
                WHERE lq.status = 'pending'
                  AND lq.queued_at < now() - make_interval(secs => $1::DOUBLE PRECISION)
                  AND NOT EXISTS (
                      SELECT 1 FROM learning_queue newer
                      WHERE newer.session_id = lq.session_id
                        AND newer.status = 'pending'
                        AND newer.queued_at > lq.queued_at
                  )
                ORDER BY lq.queued_at ASC
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id, session_id, project_id, transcript, message_count, retry_count, queued_at, status
            "#,
        )
        .bind(cooldown_secs as f64)
        .fetch_optional(pool)
        .await?;

        Ok(item)
    }

    /// Mark a queue item as completed.
    pub async fn mark_completed(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE learning_queue SET status = 'completed', completed_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Mark a queue item as failed. Increments retry_count and returns to pending.
    /// After MAX_RETRIES, marks as 'failed_permanent' (dead letter).
    pub async fn mark_failed(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE learning_queue SET \
                retry_count = retry_count + 1, \
                status = CASE WHEN retry_count + 1 >= $2 THEN 'failed_permanent' ELSE 'pending' END, \
                started_at = NULL \
             WHERE id = $1",
        )
        .bind(id)
        .bind(MAX_RETRIES)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Clean up old completed/cancelled entries (older than 7 days).
    pub async fn cleanup(pool: &PgPool) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM learning_queue \
             WHERE status IN ('completed', 'cancelled') \
             AND queued_at < now() - interval '7 days'",
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}
