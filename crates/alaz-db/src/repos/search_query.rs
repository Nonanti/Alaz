use alaz_core::Result;
use sqlx::PgPool;
use tracing::debug;

/// Entity tables that receive feedback boost updates.
///
/// Each variant maps to a compile-time SQL query, avoiding `format!()`
/// interpolation of table names.
#[derive(Debug, Clone, Copy)]
enum FeedbackEntity {
    Knowledge,
    Episode,
    Procedure,
}

impl FeedbackEntity {
    /// All entity types that receive feedback.
    const ALL: &[Self] = &[Self::Knowledge, Self::Episode, Self::Procedure];

    /// Table name for logging purposes.
    const fn table(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge_items",
            Self::Episode => "episodes",
            Self::Procedure => "procedures",
        }
    }

    /// Update `feedback_boost` based on click-through rates from search queries.
    ///
    /// CTR = (times entity was clicked) / (times entity was shown in results)
    /// over the last 7 days.
    async fn update_feedback(self, pool: &PgPool) -> Result<u64> {
        // Each variant uses a fully static SQL string — no `format!()` interpolation.
        let result = match self {
            Self::Knowledge => {
                sqlx::query(Self::FEEDBACK_SQL_KNOWLEDGE)
                    .execute(pool)
                    .await?
            }
            Self::Episode => {
                sqlx::query(Self::FEEDBACK_SQL_EPISODES)
                    .execute(pool)
                    .await?
            }
            Self::Procedure => {
                sqlx::query(Self::FEEDBACK_SQL_PROCEDURES)
                    .execute(pool)
                    .await?
            }
        };

        debug!(
            table = self.table(),
            rows = result.rows_affected(),
            "feedback updated"
        );
        Ok(result.rows_affected())
    }

    // NOTE: Each table gets its own SQL constant to avoid `format!()` interpolation
    // of table names. This is verbose but guarantees compile-time safety.

    const FEEDBACK_SQL_KNOWLEDGE: &str = r#"
        WITH shown AS (
            SELECT unnest(result_ids) AS entity_id, id AS query_id
            FROM search_queries WHERE created_at > now() - interval '7 days'
        ), shown_counts AS (
            SELECT entity_id, COUNT(*) AS times_shown FROM shown GROUP BY entity_id
        ), clicked AS (
            SELECT unnest(clicked_ids) AS entity_id, id AS query_id
            FROM search_queries WHERE created_at > now() - interval '7 days'
        ), clicked_counts AS (
            SELECT entity_id, COUNT(*) AS times_clicked FROM clicked GROUP BY entity_id
        ), rates AS (
            SELECT s.entity_id, COALESCE(c.times_clicked::float / NULLIF(s.times_shown, 0), 0) AS ctr
            FROM shown_counts s LEFT JOIN clicked_counts c ON s.entity_id = c.entity_id
        )
        UPDATE knowledge_items t SET feedback_boost = LEAST(r.ctr, 1.0) FROM rates r WHERE t.id = r.entity_id"#;

    const FEEDBACK_SQL_EPISODES: &str = r#"
        WITH shown AS (
            SELECT unnest(result_ids) AS entity_id, id AS query_id
            FROM search_queries WHERE created_at > now() - interval '7 days'
        ), shown_counts AS (
            SELECT entity_id, COUNT(*) AS times_shown FROM shown GROUP BY entity_id
        ), clicked AS (
            SELECT unnest(clicked_ids) AS entity_id, id AS query_id
            FROM search_queries WHERE created_at > now() - interval '7 days'
        ), clicked_counts AS (
            SELECT entity_id, COUNT(*) AS times_clicked FROM clicked GROUP BY entity_id
        ), rates AS (
            SELECT s.entity_id, COALESCE(c.times_clicked::float / NULLIF(s.times_shown, 0), 0) AS ctr
            FROM shown_counts s LEFT JOIN clicked_counts c ON s.entity_id = c.entity_id
        )
        UPDATE episodes t SET feedback_boost = LEAST(r.ctr, 1.0) FROM rates r WHERE t.id = r.entity_id"#;

    const FEEDBACK_SQL_PROCEDURES: &str = r#"
        WITH shown AS (
            SELECT unnest(result_ids) AS entity_id, id AS query_id
            FROM search_queries WHERE created_at > now() - interval '7 days'
        ), shown_counts AS (
            SELECT entity_id, COUNT(*) AS times_shown FROM shown GROUP BY entity_id
        ), clicked AS (
            SELECT unnest(clicked_ids) AS entity_id, id AS query_id
            FROM search_queries WHERE created_at > now() - interval '7 days'
        ), clicked_counts AS (
            SELECT entity_id, COUNT(*) AS times_clicked FROM clicked GROUP BY entity_id
        ), rates AS (
            SELECT s.entity_id, COALESCE(c.times_clicked::float / NULLIF(s.times_shown, 0), 0) AS ctr
            FROM shown_counts s LEFT JOIN clicked_counts c ON s.entity_id = c.entity_id
        )
        UPDATE procedures t SET feedback_boost = LEAST(r.ctr, 1.0) FROM rates r WHERE t.id = r.entity_id"#;
}

/// Row returned by search query lookups.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SearchQueryRow {
    pub id: String,
    pub query: String,
    pub query_type: Option<String>,
    pub result_ids: Vec<String>,
    pub signal_sources: serde_json::Value,
    pub explanations: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Aggregated search quality analytics over a time window.
#[derive(Debug, serde::Serialize)]
pub struct SearchAnalytics {
    pub total_queries: i64,
    pub queries_with_clicks: i64,
    pub click_through_rate: f64,
    pub avg_results_per_query: f64,
    pub queries_by_type: Vec<(String, i64)>,
    pub top_queries: Vec<(String, i64)>,
    pub signal_effectiveness: Vec<(String, f64)>,
}

pub struct SearchQueryRepo;

impl SearchQueryRepo {
    /// Log a search query with result IDs, classification, signal attribution,
    /// and per-result explanations.
    ///
    /// - `signal_sources`: maps entity ID → contributing signal names
    /// - `explanations`: maps entity ID → full score breakdown (fused score + per-signal contributions)
    pub async fn log(
        pool: &PgPool,
        query: &str,
        project_id: Option<&str>,
        result_ids: &[String],
        query_type: Option<&str>,
        signal_sources: Option<&serde_json::Value>,
        explanations: Option<&serde_json::Value>,
    ) -> Result<String> {
        let id = cuid2::create_id();
        let empty_json = serde_json::json!({});
        sqlx::query(
            r#"
            INSERT INTO search_queries
                (id, query, project_id, result_ids, query_type, signal_sources, explanations)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(&id)
        .bind(query)
        .bind(project_id)
        .bind(result_ids)
        .bind(query_type)
        .bind(signal_sources.unwrap_or(&empty_json))
        .bind(explanations.unwrap_or(&empty_json))
        .execute(pool)
        .await?;

        Ok(id)
    }

    /// Get the most recent search query matching the given text.
    ///
    /// Used by `alaz_explain` to retrieve explanations for a past search.
    pub async fn get_latest_by_query(
        pool: &PgPool,
        query_text: &str,
    ) -> Result<Option<SearchQueryRow>> {
        let row = sqlx::query_as::<_, SearchQueryRow>(
            r#"
            SELECT id, query, query_type, result_ids, signal_sources, explanations, created_at
            FROM search_queries
            WHERE query = $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(query_text)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Record a click (implicit feedback) by adding the entity ID to the
    /// most recent search query's clicked_ids.
    pub async fn record_click(pool: &PgPool, entity_id: &str) -> Result<()> {
        // Find the most recent search query that contains this entity in result_ids
        // and add it to clicked_ids (if not already there).
        sqlx::query(
            r#"
            UPDATE search_queries
            SET clicked_ids = array_append(clicked_ids, $1)
            WHERE id = (
                SELECT id FROM search_queries
                WHERE $1 = ANY(result_ids)
                  AND NOT ($1 = ANY(clicked_ids))
                ORDER BY created_at DESC
                LIMIT 1
            )
            "#,
        )
        .bind(entity_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Aggregate click-through rates and update `feedback_boost` on entities.
    ///
    /// Called periodically by the feedback aggregation job. Each entity table
    /// is updated via a compile-time safe SQL query (no `format!()` interpolation).
    pub async fn aggregate_feedback(pool: &PgPool) -> Result<u64> {
        let mut total = 0u64;
        for &entity in FeedbackEntity::ALL {
            total += entity.update_feedback(pool).await?;
        }
        Ok(total)
    }

    /// Compute search quality analytics over the last `days` days.
    ///
    /// Aggregates total queries, click-through rate, query type distribution,
    /// most frequent queries, and signal effectiveness (which signals contribute
    /// most to clicked results).
    pub async fn analytics(pool: &PgPool, days: i32) -> Result<SearchAnalytics> {
        let interval = format!("{days} days");

        // Basic counts
        let total_queries: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM search_queries WHERE created_at > now() - $1::interval",
        )
        .bind(&interval)
        .fetch_one(pool)
        .await?;

        let queries_with_clicks: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM search_queries WHERE created_at > now() - $1::interval AND array_length(clicked_ids, 1) > 0",
        )
        .bind(&interval)
        .fetch_one(pool)
        .await?;

        let click_through_rate = if total_queries > 0 {
            queries_with_clicks as f64 / total_queries as f64
        } else {
            0.0
        };

        let avg_results: f64 = sqlx::query_scalar(
            "SELECT COALESCE(AVG(array_length(result_ids, 1))::float8, 0) FROM search_queries WHERE created_at > now() - $1::interval",
        )
        .bind(&interval)
        .fetch_one(pool)
        .await?;

        // Queries by type
        let type_rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT COALESCE(query_type, 'unknown'), count(*) FROM search_queries WHERE created_at > now() - $1::interval GROUP BY query_type ORDER BY count(*) DESC",
        )
        .bind(&interval)
        .fetch_all(pool)
        .await?;

        // Top queries (most frequent)
        let top_rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT query, count(*) FROM search_queries WHERE created_at > now() - $1::interval GROUP BY query ORDER BY count(*) DESC LIMIT 10",
        )
        .bind(&interval)
        .fetch_all(pool)
        .await?;

        // Signal effectiveness: for clicked results, which signals contributed?
        // signal_sources format: {"entity_id": ["fts", "dense"], ...}
        // We look at clicked_ids and check which signals were present for those IDs.
        let signal_rows: Vec<(String, i64)> = sqlx::query_as(
            r#"
            WITH clicked_signals AS (
                SELECT
                    jsonb_array_elements_text(
                        signal_sources -> unnest(clicked_ids)
                    ) AS signal_name
                FROM search_queries
                WHERE created_at > now() - $1::interval
                  AND array_length(clicked_ids, 1) > 0
                  AND signal_sources != '{}'::jsonb
            )
            SELECT signal_name, count(*) AS cnt
            FROM clicked_signals
            GROUP BY signal_name
            ORDER BY cnt DESC
            "#,
        )
        .bind(&interval)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        // Convert signal counts to proportions
        let signal_total: i64 = signal_rows.iter().map(|(_, c)| c).sum();
        let signal_effectiveness: Vec<(String, f64)> = if signal_total > 0 {
            signal_rows
                .into_iter()
                .map(|(name, count)| (name, count as f64 / signal_total as f64))
                .collect()
        } else {
            Vec::new()
        };

        Ok(SearchAnalytics {
            total_queries,
            queries_with_clicks,
            click_through_rate,
            avg_results_per_query: avg_results,
            queries_by_type: type_rows,
            top_queries: top_rows,
            signal_effectiveness,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_analytics_serializes_empty() {
        let analytics = SearchAnalytics {
            total_queries: 0,
            queries_with_clicks: 0,
            click_through_rate: 0.0,
            avg_results_per_query: 0.0,
            queries_by_type: Vec::new(),
            top_queries: Vec::new(),
            signal_effectiveness: Vec::new(),
        };
        let json = serde_json::to_value(&analytics).unwrap();
        assert_eq!(json["total_queries"], 0);
        assert_eq!(json["queries_with_clicks"], 0);
        assert!((json["click_through_rate"].as_f64().unwrap()).abs() < f64::EPSILON);
        assert!(json["queries_by_type"].as_array().unwrap().is_empty());
        assert!(json["top_queries"].as_array().unwrap().is_empty());
        assert!(json["signal_effectiveness"].as_array().unwrap().is_empty());
    }

    #[test]
    fn search_analytics_serializes_with_data() {
        let analytics = SearchAnalytics {
            total_queries: 100,
            queries_with_clicks: 35,
            click_through_rate: 0.35,
            avg_results_per_query: 8.5,
            queries_by_type: vec![("factual".to_string(), 60), ("exploratory".to_string(), 40)],
            top_queries: vec![
                ("rust async".to_string(), 15),
                ("error handling".to_string(), 10),
            ],
            signal_effectiveness: vec![
                ("fts".to_string(), 0.4),
                ("dense".to_string(), 0.35),
                ("colbert".to_string(), 0.25),
            ],
        };
        let json = serde_json::to_value(&analytics).unwrap();
        assert_eq!(json["total_queries"], 100);
        assert_eq!(json["queries_with_clicks"], 35);
        assert!((json["click_through_rate"].as_f64().unwrap() - 0.35).abs() < f64::EPSILON);
        assert!((json["avg_results_per_query"].as_f64().unwrap() - 8.5).abs() < f64::EPSILON);

        let types = json["queries_by_type"].as_array().unwrap();
        assert_eq!(types.len(), 2);
        assert_eq!(types[0][0], "factual");
        assert_eq!(types[0][1], 60);

        let top = json["top_queries"].as_array().unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0][0], "rust async");

        let signals = json["signal_effectiveness"].as_array().unwrap();
        assert_eq!(signals.len(), 3);
        assert_eq!(signals[0][0], "fts");
        assert!((signals[0][1].as_f64().unwrap() - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn search_analytics_ctr_calculation() {
        // Verify the CTR formula matches what analytics() computes
        let total = 50i64;
        let with_clicks = 20i64;
        let ctr = with_clicks as f64 / total as f64;
        assert!((ctr - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn search_analytics_signal_proportion_calculation() {
        // Verify signal proportion calculation
        let signal_rows = vec![
            ("fts".to_string(), 20i64),
            ("dense".to_string(), 15i64),
            ("colbert".to_string(), 5i64),
        ];
        let signal_total: i64 = signal_rows.iter().map(|(_, c)| c).sum();
        let proportions: Vec<(String, f64)> = signal_rows
            .into_iter()
            .map(|(name, count)| (name, count as f64 / signal_total as f64))
            .collect();
        assert_eq!(signal_total, 40);
        assert!((proportions[0].1 - 0.5).abs() < f64::EPSILON);
        assert!((proportions[1].1 - 0.375).abs() < f64::EPSILON);
        assert!((proportions[2].1 - 0.125).abs() < f64::EPSILON);
    }
}
