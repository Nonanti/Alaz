use alaz_core::Result;
use sqlx::PgPool;

/// Tracks which entities were injected into session context
/// and which were later referenced (searched/accessed).
pub struct ContextTrackingRepo;

/// Aggregated usage statistics for context injections.
#[derive(Debug, serde::Serialize)]
pub struct ContextUsageStats {
    /// Total number of context injection events.
    pub total_injections: i64,
    /// Average number of entities injected per session.
    pub avg_entities_injected: f64,
    /// Average number of entities referenced per session.
    pub avg_entities_referenced: f64,
    /// Overall usage rate: referenced / injected (0.0–1.0).
    pub usage_rate: f64,
    /// Top entities that get referenced most often: (entity_id, count).
    pub most_referenced_entities: Vec<(String, i64)>,
    /// Sections that were injected but never had their entities referenced: (section, count).
    pub never_referenced_sections: Vec<(String, i64)>,
}

impl ContextTrackingRepo {
    /// Record a context injection event, returning the generated ID.
    pub async fn record_injection(
        pool: &PgPool,
        session_id: Option<&str>,
        project_id: Option<&str>,
        entity_ids: &[String],
        sections: &[String],
        tokens_used: u64,
    ) -> Result<String> {
        let id = cuid2::create_id();

        sqlx::query(
            r#"
            INSERT INTO context_injections
                (id, session_id, project_id, injected_entity_ids, injected_sections, tokens_used)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(project_id)
        .bind(entity_ids)
        .bind(sections)
        .bind(tokens_used as i64)
        .execute(pool)
        .await?;

        Ok(id)
    }

    /// Record that an entity was referenced (searched/accessed) during a session.
    /// Appends the entity_id to referenced_entity_ids and increments reference_count.
    pub async fn record_reference(pool: &PgPool, session_id: &str, entity_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE context_injections
            SET referenced_entity_ids = array_append(referenced_entity_ids, $2),
                reference_count = reference_count + 1
            WHERE session_id = $1
              AND $2 = ANY(injected_entity_ids)
              AND NOT ($2 = ANY(referenced_entity_ids))
            "#,
        )
        .bind(session_id)
        .bind(entity_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Compute aggregated usage statistics over the given number of days.
    pub async fn usage_stats(pool: &PgPool, days: i32) -> Result<ContextUsageStats> {
        // Aggregates: totals, averages
        let row = sqlx::query_as::<_, (i64, Option<f64>, Option<f64>)>(
            r#"
            SELECT
                count(*) AS total,
                avg(array_length(injected_entity_ids, 1))  AS avg_injected,
                avg(array_length(referenced_entity_ids, 1)) AS avg_referenced
            FROM context_injections
            WHERE created_at >= now() - make_interval(days => $1)
            "#,
        )
        .bind(days)
        .fetch_one(pool)
        .await?;

        let total_injections = row.0;
        let avg_entities_injected = row.1.unwrap_or(0.0);
        let avg_entities_referenced = row.2.unwrap_or(0.0);
        let usage_rate = if avg_entities_injected > 0.0 {
            avg_entities_referenced / avg_entities_injected
        } else {
            0.0
        };

        // Most referenced entities: unnest referenced_entity_ids and count
        let most_referenced: Vec<(String, i64)> = sqlx::query_as(
            r#"
            SELECT entity_id, count(*) AS cnt
            FROM context_injections,
                 unnest(referenced_entity_ids) AS entity_id
            WHERE created_at >= now() - make_interval(days => $1)
            GROUP BY entity_id
            ORDER BY cnt DESC
            LIMIT 10
            "#,
        )
        .bind(days)
        .fetch_all(pool)
        .await?;

        // Sections that were injected but never had entities referenced.
        // A "never referenced" injection is one where referenced_entity_ids is empty.
        let never_referenced: Vec<(String, i64)> = sqlx::query_as(
            r#"
            SELECT section, count(*) AS cnt
            FROM context_injections,
                 unnest(injected_sections) AS section
            WHERE created_at >= now() - make_interval(days => $1)
              AND array_length(referenced_entity_ids, 1) IS NULL
            GROUP BY section
            ORDER BY cnt DESC
            LIMIT 10
            "#,
        )
        .bind(days)
        .fetch_all(pool)
        .await?;

        Ok(ContextUsageStats {
            total_injections,
            avg_entities_injected,
            avg_entities_referenced,
            usage_rate,
            most_referenced_entities: most_referenced,
            never_referenced_sections: never_referenced,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_usage_stats_serializes() {
        let stats = ContextUsageStats {
            total_injections: 42,
            avg_entities_injected: 15.3,
            avg_entities_referenced: 3.7,
            usage_rate: 0.242,
            most_referenced_entities: vec![("ent_abc".to_string(), 12), ("ent_def".to_string(), 7)],
            never_referenced_sections: vec![
                ("global_patterns".to_string(), 30),
                ("reflections".to_string(), 25),
            ],
        };

        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_injections"], 42);
        assert_eq!(json["usage_rate"], 0.242);
        assert!(json["most_referenced_entities"].is_array());
        assert_eq!(json["most_referenced_entities"][0][0], "ent_abc");
        assert_eq!(json["most_referenced_entities"][0][1], 12);
        assert!(json["never_referenced_sections"].is_array());
        assert_eq!(json["never_referenced_sections"][0][0], "global_patterns");
    }

    #[test]
    fn context_usage_stats_zero_division_safe() {
        let stats = ContextUsageStats {
            total_injections: 0,
            avg_entities_injected: 0.0,
            avg_entities_referenced: 0.0,
            usage_rate: 0.0,
            most_referenced_entities: vec![],
            never_referenced_sections: vec![],
        };

        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["usage_rate"], 0.0);
        assert_eq!(json["total_injections"], 0);
    }

    #[test]
    fn context_usage_stats_empty_collections() {
        let stats = ContextUsageStats {
            total_injections: 5,
            avg_entities_injected: 10.0,
            avg_entities_referenced: 0.0,
            usage_rate: 0.0,
            most_referenced_entities: vec![],
            never_referenced_sections: vec![],
        };

        let json = serde_json::to_value(&stats).unwrap();
        assert!(
            json["most_referenced_entities"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            json["never_referenced_sections"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
