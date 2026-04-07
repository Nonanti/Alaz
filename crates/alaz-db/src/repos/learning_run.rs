use alaz_core::Result;
use sqlx::PgPool;

pub struct LearningRunRepo;

/// A single learning pipeline run record.
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct LearningRun {
    pub id: String,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub transcript_size_bytes: i64,
    pub chunks_processed: i32,
    pub patterns_extracted: i32,
    pub episodes_extracted: i32,
    pub procedures_extracted: i32,
    pub memories_extracted: i32,
    pub duplicates_skipped: i32,
    pub contradictions_resolved: i32,
    pub duration_ms: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Aggregated analytics over a time window.
#[derive(Debug, serde::Serialize)]
pub struct LearningAnalytics {
    pub total_runs: i64,
    /// Average total items extracted per run.
    pub avg_extraction_rate: f64,
    /// Average ratio of duplicates to total candidates (extracted + skipped).
    pub avg_duplicate_rate: f64,
    pub total_patterns: i64,
    pub total_episodes: i64,
    pub total_procedures: i64,
    pub total_memories: i64,
    pub avg_duration_ms: f64,
    pub avg_chunks_per_run: f64,
}

impl LearningRunRepo {
    /// Record a completed learning pipeline run.
    pub async fn record(pool: &PgPool, run: &LearningRun) -> Result<()> {
        sqlx::query(
            "INSERT INTO learning_runs (
                id, session_id, project_id, transcript_size_bytes, chunks_processed,
                patterns_extracted, episodes_extracted, procedures_extracted,
                memories_extracted, duplicates_skipped, contradictions_resolved,
                duration_ms, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(&run.id)
        .bind(&run.session_id)
        .bind(&run.project_id)
        .bind(run.transcript_size_bytes)
        .bind(run.chunks_processed)
        .bind(run.patterns_extracted)
        .bind(run.episodes_extracted)
        .bind(run.procedures_extracted)
        .bind(run.memories_extracted)
        .bind(run.duplicates_skipped)
        .bind(run.contradictions_resolved)
        .bind(run.duration_ms)
        .bind(run.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Compute aggregated analytics over the last N days.
    pub async fn analytics(pool: &PgPool, days: i32) -> Result<LearningAnalytics> {
        let row = sqlx::query_as::<_, AnalyticsRow>(
            "SELECT
                COALESCE(count(*), 0) AS total_runs,
                COALESCE(avg(patterns_extracted + episodes_extracted + procedures_extracted + memories_extracted)::float8, 0) AS avg_extraction_rate,
                COALESCE(avg(
                    CASE WHEN (patterns_extracted + episodes_extracted + procedures_extracted + memories_extracted + duplicates_skipped) > 0
                    THEN duplicates_skipped::float8 / (patterns_extracted + episodes_extracted + procedures_extracted + memories_extracted + duplicates_skipped)::float8
                    ELSE 0 END
                )::float8, 0) AS avg_duplicate_rate,
                COALESCE(sum(patterns_extracted), 0) AS total_patterns,
                COALESCE(sum(episodes_extracted), 0) AS total_episodes,
                COALESCE(sum(procedures_extracted), 0) AS total_procedures,
                COALESCE(sum(memories_extracted), 0) AS total_memories,
                COALESCE(avg(duration_ms)::float8, 0) AS avg_duration_ms,
                COALESCE(avg(chunks_processed)::float8, 0) AS avg_chunks_per_run
            FROM learning_runs
            WHERE created_at >= now() - make_interval(days => $1)",
        )
        .bind(days)
        .fetch_one(pool)
        .await?;

        Ok(LearningAnalytics {
            total_runs: row.total_runs,
            avg_extraction_rate: row.avg_extraction_rate,
            avg_duplicate_rate: row.avg_duplicate_rate,
            total_patterns: row.total_patterns,
            total_episodes: row.total_episodes,
            total_procedures: row.total_procedures,
            total_memories: row.total_memories,
            avg_duration_ms: row.avg_duration_ms,
            avg_chunks_per_run: row.avg_chunks_per_run,
        })
    }

    /// Fetch the most recent learning runs.
    pub async fn recent(pool: &PgPool, limit: i64) -> Result<Vec<LearningRun>> {
        let rows = sqlx::query_as::<_, LearningRun>(
            "SELECT id, session_id, project_id, transcript_size_bytes, chunks_processed,
                    patterns_extracted, episodes_extracted, procedures_extracted,
                    memories_extracted, duplicates_skipped, contradictions_resolved,
                    duration_ms, created_at
             FROM learning_runs
             ORDER BY created_at DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}

/// Internal row type for the analytics aggregate query.
#[derive(sqlx::FromRow)]
struct AnalyticsRow {
    total_runs: i64,
    avg_extraction_rate: f64,
    avg_duplicate_rate: f64,
    total_patterns: i64,
    total_episodes: i64,
    total_procedures: i64,
    total_memories: i64,
    avg_duration_ms: f64,
    avg_chunks_per_run: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_run_serializes() {
        let run = LearningRun {
            id: "lr_test123".to_string(),
            session_id: Some("sess_001".to_string()),
            project_id: Some("proj_abc".to_string()),
            transcript_size_bytes: 48000,
            chunks_processed: 3,
            patterns_extracted: 5,
            episodes_extracted: 2,
            procedures_extracted: 1,
            memories_extracted: 3,
            duplicates_skipped: 4,
            contradictions_resolved: 1,
            duration_ms: 12500,
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_value(&run).unwrap();
        assert_eq!(json["id"], "lr_test123");
        assert_eq!(json["session_id"], "sess_001");
        assert_eq!(json["transcript_size_bytes"], 48000);
        assert_eq!(json["patterns_extracted"], 5);
        assert_eq!(json["duplicates_skipped"], 4);
        assert_eq!(json["duration_ms"], 12500);
    }

    #[test]
    fn learning_run_nullable_fields() {
        let run = LearningRun {
            id: "lr_test456".to_string(),
            session_id: None,
            project_id: None,
            transcript_size_bytes: 1024,
            chunks_processed: 1,
            patterns_extracted: 0,
            episodes_extracted: 0,
            procedures_extracted: 0,
            memories_extracted: 0,
            duplicates_skipped: 0,
            contradictions_resolved: 0,
            duration_ms: 500,
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_value(&run).unwrap();
        assert!(json["session_id"].is_null());
        assert!(json["project_id"].is_null());
        assert_eq!(json["patterns_extracted"], 0);
    }

    #[test]
    fn learning_analytics_serializes() {
        let analytics = LearningAnalytics {
            total_runs: 42,
            avg_extraction_rate: 8.5,
            avg_duplicate_rate: 0.23,
            total_patterns: 150,
            total_episodes: 80,
            total_procedures: 30,
            total_memories: 95,
            avg_duration_ms: 15000.0,
            avg_chunks_per_run: 4.2,
        };
        let json = serde_json::to_value(&analytics).unwrap();
        assert_eq!(json["total_runs"], 42);
        assert!((json["avg_extraction_rate"].as_f64().unwrap() - 8.5).abs() < f64::EPSILON);
        assert!((json["avg_duplicate_rate"].as_f64().unwrap() - 0.23).abs() < f64::EPSILON);
        assert_eq!(json["total_patterns"], 150);
        assert_eq!(json["total_episodes"], 80);
        assert_eq!(json["total_procedures"], 30);
        assert_eq!(json["total_memories"], 95);
        assert!((json["avg_duration_ms"].as_f64().unwrap() - 15000.0).abs() < f64::EPSILON);
        assert!((json["avg_chunks_per_run"].as_f64().unwrap() - 4.2).abs() < f64::EPSILON);
    }

    #[test]
    fn learning_analytics_zero_state() {
        let analytics = LearningAnalytics {
            total_runs: 0,
            avg_extraction_rate: 0.0,
            avg_duplicate_rate: 0.0,
            total_patterns: 0,
            total_episodes: 0,
            total_procedures: 0,
            total_memories: 0,
            avg_duration_ms: 0.0,
            avg_chunks_per_run: 0.0,
        };
        let json = serde_json::to_value(&analytics).unwrap();
        assert_eq!(json["total_runs"], 0);
        assert!((json["avg_extraction_rate"].as_f64().unwrap()).abs() < f64::EPSILON);
    }
}
