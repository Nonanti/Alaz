use alaz_core::{AlazError, Result};
use sqlx::PgPool;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct WorkUnit {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub goal: Option<String>,
    pub project_id: Option<String>,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct WorkUnitRepo;

impl WorkUnitRepo {
    pub async fn create(
        pool: &PgPool,
        name: &str,
        description: Option<&str>,
        goal: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<WorkUnit> {
        let id = cuid2::create_id();
        let row = sqlx::query_as::<_, WorkUnit>(
            "INSERT INTO work_units (id, name, description, goal, project_id) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING *",
        )
        .bind(&id)
        .bind(name)
        .bind(description)
        .bind(goal)
        .bind(project_id)
        .fetch_one(pool)
        .await?;
        Ok(row)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<WorkUnit> {
        sqlx::query_as::<_, WorkUnit>("SELECT * FROM work_units WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AlazError::NotFound(format!("work unit {id}")))
    }

    pub async fn list(
        pool: &PgPool,
        project_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<WorkUnit>> {
        let rows = sqlx::query_as::<_, WorkUnit>(
            "SELECT * FROM work_units \
             WHERE ($1::TEXT IS NULL OR project_id = $1) \
             AND ($2::TEXT IS NULL OR status = $2) \
             ORDER BY updated_at DESC LIMIT 50",
        )
        .bind(project_id)
        .bind(status)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn update_status(pool: &PgPool, id: &str, status: &str) -> Result<()> {
        let completed_at = if status == "completed" {
            Some(chrono::Utc::now())
        } else {
            None
        };
        let result = sqlx::query(
            "UPDATE work_units SET status = $2, updated_at = now(), \
             completed_at = COALESCE($3, completed_at) WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(completed_at)
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("work unit {id}")));
        }
        Ok(())
    }

    pub async fn link_session(pool: &PgPool, session_id: &str, work_unit_id: &str) -> Result<()> {
        sqlx::query("UPDATE session_logs SET work_unit_id = $2 WHERE id = $1")
            .bind(session_id)
            .bind(work_unit_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
