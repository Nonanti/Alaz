use alaz_core::{AlazError, Result};
use sqlx::PgPool;

// ============================================================================
// Structured Logs
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct StructuredLog {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: Option<serde_json::Value>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct LogStats {
    pub level: String,
    pub count: i64,
}

pub struct StructuredLogRepo;

impl StructuredLogRepo {
    /// Bulk insert logs. Called by the tracing layer's background flush task.
    pub async fn insert_batch(pool: &PgPool, logs: &[NewLog]) -> Result<usize> {
        if logs.is_empty() {
            return Ok(0);
        }
        let mut count = 0;
        for log in logs {
            sqlx::query(
                "INSERT INTO structured_logs (level, target, message, fields, fingerprint) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&log.level)
            .bind(&log.target)
            .bind(&log.message)
            .bind(&log.fields)
            .bind(&log.fingerprint)
            .execute(pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    /// Query logs with filters.
    pub async fn query(
        pool: &PgPool,
        level: Option<&str>,
        target: Option<&str>,
        search: Option<&str>,
        since_secs: Option<i64>,
        limit: i64,
    ) -> Result<Vec<StructuredLog>> {
        let since = since_secs.map(|s| chrono::Utc::now() - chrono::Duration::seconds(s));

        let rows = sqlx::query_as::<_, StructuredLog>(
            r#"
            SELECT id, timestamp, level, target, message, fields, fingerprint
            FROM structured_logs
            WHERE ($1::TEXT IS NULL OR level = $1)
              AND ($2::TEXT IS NULL OR target LIKE $2 || '%')
              AND ($3::TEXT IS NULL OR search_vector @@ websearch_to_tsquery('english', $3))
              AND ($4::TIMESTAMPTZ IS NULL OR timestamp >= $4)
            ORDER BY timestamp DESC
            LIMIT $5
            "#,
        )
        .bind(level)
        .bind(target)
        .bind(search)
        .bind(since)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Get log counts by level for a time window.
    pub async fn stats_by_level(pool: &PgPool, since_secs: i64) -> Result<Vec<LogStats>> {
        let since = chrono::Utc::now() - chrono::Duration::seconds(since_secs);
        let rows = sqlx::query_as::<_, LogStats>(
            "SELECT level, COUNT(*)::BIGINT as count FROM structured_logs \
             WHERE timestamp >= $1 GROUP BY level ORDER BY count DESC",
        )
        .bind(since)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Count recent logs matching a filter (for alert evaluation).
    pub async fn count_matching(
        pool: &PgPool,
        level: Option<&str>,
        target: Option<&str>,
        pattern: Option<&str>,
        since_secs: i64,
    ) -> Result<i64> {
        let since = chrono::Utc::now() - chrono::Duration::seconds(since_secs);
        let count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)::BIGINT FROM structured_logs
            WHERE timestamp >= $1
              AND ($2::TEXT IS NULL OR level = $2)
              AND ($3::TEXT IS NULL OR target LIKE $3 || '%')
              AND ($4::TEXT IS NULL OR message ~ $4)
            "#,
        )
        .bind(since)
        .bind(level)
        .bind(target)
        .bind(pattern)
        .fetch_one(pool)
        .await?;
        Ok(count.0)
    }

    /// Delete old logs beyond retention period.
    pub async fn cleanup(pool: &PgPool, retention_days: i64) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM structured_logs WHERE timestamp < now() - make_interval(days => $1::INT)",
        )
        .bind(retention_days as i32)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}

/// Input for inserting a new log entry.
#[derive(Debug, Clone)]
pub struct NewLog {
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: Option<serde_json::Value>,
    pub fingerprint: Option<String>,
}

// ============================================================================
// Error Groups
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct ErrorGroup {
    pub id: String,
    pub fingerprint: String,
    pub title: String,
    pub target: String,
    pub first_seen: chrono::DateTime<chrono::Utc>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub event_count: i64,
    pub status: String,
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub resolution_notes: Option<String>,
}

pub struct ErrorGroupRepo;

impl ErrorGroupRepo {
    /// Upsert an error group: create or update event_count + last_seen.
    pub async fn upsert(pool: &PgPool, fingerprint: &str, title: &str, target: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO error_groups (fingerprint, title, target)
            VALUES ($1, $2, $3)
            ON CONFLICT (fingerprint) DO UPDATE SET
                event_count = error_groups.event_count + 1,
                last_seen = now(),
                status = CASE WHEN error_groups.status = 'resolved' THEN 'unresolved' ELSE error_groups.status END
            "#,
        )
        .bind(fingerprint)
        .bind(title)
        .bind(target)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Process new error logs into groups.
    pub async fn aggregate_new(pool: &PgPool, since_secs: i64) -> Result<u64> {
        let since = chrono::Utc::now() - chrono::Duration::seconds(since_secs);
        // Find all error-level logs with fingerprints that don't have a group yet
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            r#"
            SELECT DISTINCT fingerprint, message, target
            FROM structured_logs
            WHERE level IN ('error', 'warn')
              AND fingerprint IS NOT NULL
              AND timestamp >= $1
            "#,
        )
        .bind(since)
        .fetch_all(pool)
        .await?;

        let count = rows.len();
        for (fp, msg, target) in rows {
            let title = msg.chars().take(200).collect::<String>();
            Self::upsert(pool, &fp, &title, &target).await?;
        }
        Ok(count as u64)
    }

    /// List error groups with optional filters.
    pub async fn list(pool: &PgPool, status: Option<&str>, limit: i64) -> Result<Vec<ErrorGroup>> {
        let rows = sqlx::query_as::<_, ErrorGroup>(
            "SELECT * FROM error_groups \
             WHERE ($1::TEXT IS NULL OR status = $1) \
             ORDER BY last_seen DESC LIMIT $2",
        )
        .bind(status)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<ErrorGroup> {
        sqlx::query_as::<_, ErrorGroup>("SELECT * FROM error_groups WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AlazError::NotFound(format!("error group {id}")))
    }

    pub async fn resolve(pool: &PgPool, id: &str, notes: Option<&str>) -> Result<()> {
        sqlx::query(
            "UPDATE error_groups SET status = 'resolved', resolved_at = now(), resolution_notes = $2 \
             WHERE id = $1",
        )
        .bind(id)
        .bind(notes)
        .execute(pool)
        .await?;
        Ok(())
    }
}

// ============================================================================
// Alert Rules
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub condition_type: String,
    pub threshold: i32,
    pub window_secs: i32,
    pub filter_level: Option<String>,
    pub filter_target: Option<String>,
    pub filter_pattern: Option<String>,
    pub enabled: bool,
    pub last_triggered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub trigger_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct AlertRuleRepo;

impl AlertRuleRepo {
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
        condition_type: &str,
        threshold: i32,
        window_secs: i32,
        filter_level: Option<&str>,
        filter_target: Option<&str>,
        filter_pattern: Option<&str>,
    ) -> Result<AlertRule> {
        sqlx::query_as::<_, AlertRule>(
            "INSERT INTO alert_rules (name, description, condition_type, threshold, window_secs, \
             filter_level, filter_target, filter_pattern) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING *",
        )
        .bind(name)
        .bind(description)
        .bind(condition_type)
        .bind(threshold)
        .bind(window_secs)
        .bind(filter_level)
        .bind(filter_target)
        .bind(filter_pattern)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn list_enabled(pool: &PgPool) -> Result<Vec<AlertRule>> {
        let rows = sqlx::query_as::<_, AlertRule>(
            "SELECT * FROM alert_rules WHERE enabled = TRUE ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_all(pool: &PgPool) -> Result<Vec<AlertRule>> {
        let rows =
            sqlx::query_as::<_, AlertRule>("SELECT * FROM alert_rules ORDER BY created_at DESC")
                .fetch_all(pool)
                .await?;
        Ok(rows)
    }

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM alert_rules WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("alert rule {id}")));
        }
        Ok(())
    }

    pub async fn record_trigger(
        pool: &PgPool,
        rule_id: &str,
        matched_count: i32,
        details: Option<&serde_json::Value>,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "UPDATE alert_rules SET last_triggered_at = now(), trigger_count = trigger_count + 1 \
             WHERE id = $1",
        )
        .bind(rule_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO alert_history (alert_rule_id, matched_count, details) VALUES ($1, $2, $3)",
        )
        .bind(rule_id)
        .bind(matched_count)
        .bind(details)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AlertHistoryEntry {
    pub id: String,
    pub alert_rule_id: String,
    pub triggered_at: chrono::DateTime<chrono::Utc>,
    pub matched_count: i32,
    pub details: Option<serde_json::Value>,
}

pub struct AlertHistoryRepo;

impl AlertHistoryRepo {
    pub async fn recent(pool: &PgPool, limit: i64) -> Result<Vec<AlertHistoryEntry>> {
        let rows = sqlx::query_as::<_, AlertHistoryEntry>(
            "SELECT * FROM alert_history ORDER BY triggered_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}
