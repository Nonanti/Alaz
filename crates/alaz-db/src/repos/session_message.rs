use alaz_core::Result;
use sqlx::PgPool;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SessionMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tool_use: Option<serde_json::Value>,
    pub tool_result: Option<serde_json::Value>,
    pub model: Option<String>,
    pub search_text: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SessionMessageSearchResult {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub search_text: Option<String>,
    pub headline: String,
    pub rank: f32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct SessionMessageRepo;

impl SessionMessageRepo {
    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        pool: &PgPool,
        session_id: &str,
        role: &str,
        content: &str,
        tool_use: Option<&serde_json::Value>,
        tool_result: Option<&serde_json::Value>,
        model: Option<&str>,
        search_text: Option<&str>,
    ) -> Result<String> {
        let id = cuid2::create_id();
        sqlx::query(
            "INSERT INTO session_messages (id, session_id, role, content, tool_use, tool_result, model, search_text) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(tool_use)
        .bind(tool_result)
        .bind(model)
        .bind(search_text)
        .execute(pool)
        .await?;
        Ok(id)
    }

    pub async fn search(
        pool: &PgPool,
        query: &str,
        session_id: Option<&str>,
        role: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SessionMessageSearchResult>> {
        let rows = sqlx::query_as::<_, SessionMessageSearchResult>(
            r#"
            SELECT id, session_id, role, search_text,
                   ts_headline('english', COALESCE(search_text, ''), websearch_to_tsquery('english', $1),
                     'MaxWords=60, MinWords=20, StartSel=<<, StopSel=>>') as headline,
                   ts_rank(search_vector, websearch_to_tsquery('english', $1)) as rank,
                   created_at
            FROM session_messages
            WHERE search_vector @@ websearch_to_tsquery('english', $1)
              AND ($2::TEXT IS NULL OR session_id = $2)
              AND ($3::TEXT IS NULL OR role = $3)
            ORDER BY rank DESC
            LIMIT $4
            "#,
        )
        .bind(query)
        .bind(session_id)
        .bind(role)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_by_session(
        pool: &PgPool,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SessionMessage>> {
        let rows = sqlx::query_as::<_, SessionMessage>(
            "SELECT id, session_id, role, content, tool_use, tool_result, model, search_text, created_at \
             FROM session_messages WHERE session_id = $1 \
             ORDER BY created_at ASC LIMIT $2 OFFSET $3",
        )
        .bind(session_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn count_by_session(pool: &PgPool, session_id: &str) -> Result<i64> {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM session_messages WHERE session_id = $1")
                .bind(session_id)
                .fetch_one(pool)
                .await?;
        Ok(count.0)
    }
}
