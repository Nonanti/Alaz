use alaz_core::models::{CreateReflection, ListReflectionsFilter, Reflection};
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

/// Standard SELECT columns for reading a Reflection.
const REFLECTION_COLUMNS: &str = "\
    id, session_id, what_worked, what_failed, lessons_learned, \
    effectiveness_score, complexity_score, project_id, kind, \
    action_items, overall_score, knowledge_score, decision_score, \
    efficiency_score, evaluated_episode_ids, needs_embedding, \
    created_at, updated_at";

/// Build a `SELECT <columns> FROM reflections <suffix>` query.
fn select_reflections(suffix: &str) -> String {
    format!("SELECT {REFLECTION_COLUMNS} FROM reflections {suffix}")
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct ScoreTrend {
    pub date: chrono::NaiveDate,
    pub avg_overall: Option<f64>,
    pub avg_knowledge: Option<f64>,
    pub avg_decision: Option<f64>,
    pub avg_efficiency: Option<f64>,
    pub count: i64,
}

pub struct ReflectionRepo;

impl ReflectionRepo {
    pub async fn create(
        pool: &PgPool,
        input: &CreateReflection,
        project_id: Option<&str>,
    ) -> Result<Reflection> {
        let id = cuid2::create_id();
        let kind = input.kind.as_deref().unwrap_or("session_end");
        let action_items = input
            .action_items
            .as_ref()
            .map(|items| serde_json::to_value(items).unwrap_or_default())
            .unwrap_or(serde_json::Value::Array(vec![]));
        let evaluated_episode_ids = input.evaluated_episode_ids.as_deref().unwrap_or(&[]);

        let insert_sql = format!(
            "INSERT INTO reflections (id, session_id, what_worked, what_failed, lessons_learned, \
             effectiveness_score, complexity_score, project_id, kind, \
             action_items, overall_score, knowledge_score, decision_score, \
             efficiency_score, evaluated_episode_ids) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
             RETURNING {REFLECTION_COLUMNS}"
        );
        let row = sqlx::query_as::<_, Reflection>(&insert_sql)
            .bind(&id)
            .bind(&input.session_id)
            .bind(&input.what_worked)
            .bind(&input.what_failed)
            .bind(&input.lessons_learned)
            .bind(input.effectiveness_score)
            .bind(input.complexity_score)
            .bind(project_id)
            .bind(kind)
            .bind(&action_items)
            .bind(input.overall_score)
            .bind(input.knowledge_score)
            .bind(input.decision_score)
            .bind(input.efficiency_score)
            .bind(evaluated_episode_ids)
            .fetch_one(pool)
            .await?;

        Ok(row)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<Reflection> {
        let sql = select_reflections("WHERE id = $1");
        let row = sqlx::query_as::<_, Reflection>(&sql)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AlazError::NotFound(format!("reflection {id}")))?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM reflections WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("reflection {id}")));
        }
        Ok(())
    }

    pub async fn list(pool: &PgPool, filter: &ListReflectionsFilter) -> Result<Vec<Reflection>> {
        let limit = filter.limit.unwrap_or(20);
        let offset = filter.offset.unwrap_or(0);

        let sql = select_reflections(
            "WHERE ($1::TEXT IS NULL OR project_id = $1)
              AND ($2::TEXT IS NULL OR kind = $2)
              AND ($3::TEXT IS NULL OR session_id = $3)
            ORDER BY created_at DESC
            LIMIT $4 OFFSET $5",
        );
        let rows = sqlx::query_as::<_, Reflection>(&sql)
            .bind(&filter.project)
            .bind(&filter.kind)
            .bind(&filter.session_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Full-text search on reflections. Returns (id, lessons_learned, rank) tuples.
    pub async fn fts_search(
        pool: &PgPool,
        query: &str,
        project: Option<&str>,
        limit: i64,
    ) -> Result<Vec<(String, String, f32)>> {
        let rows = sqlx::query_as::<_, (String, String, f32)>(
            r#"
            SELECT id,
                   COALESCE(lessons_learned, '') AS lessons_learned,
                   ts_rank(search_vector, websearch_to_tsquery('simple', $1))::REAL AS rank
            FROM reflections
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

    pub async fn find_needing_embedding(pool: &PgPool, limit: i64) -> Result<Vec<Reflection>> {
        let sql =
            select_reflections("WHERE needs_embedding = TRUE ORDER BY created_at ASC LIMIT $1");
        let rows = sqlx::query_as::<_, Reflection>(&sql)
            .bind(limit)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    pub async fn mark_embedded(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("UPDATE reflections SET needs_embedding = FALSE WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Aggregate daily averages of reflection scores over a time window.
    pub async fn score_trends(
        pool: &PgPool,
        project_id: Option<&str>,
        days: i64,
    ) -> Result<Vec<ScoreTrend>> {
        let rows = sqlx::query_as::<_, ScoreTrend>(
            r#"
            SELECT created_at::date AS date,
                   AVG(overall_score) AS avg_overall,
                   AVG(knowledge_score) AS avg_knowledge,
                   AVG(decision_score) AS avg_decision,
                   AVG(efficiency_score) AS avg_efficiency,
                   COUNT(*) AS count
            FROM reflections
            WHERE created_at >= now() - make_interval(days => $1::int)
              AND ($2::TEXT IS NULL OR project_id = $2)
            GROUP BY created_at::date
            ORDER BY date ASC
            "#,
        )
        .bind(days as i32)
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}
