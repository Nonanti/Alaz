use alaz_core::Result;
use alaz_core::models::Project;
use sqlx::PgPool;

pub struct ProjectRepo;

impl ProjectRepo {
    /// Get or create a project by name. If it exists, updates the path.
    pub async fn get_or_create(pool: &PgPool, name: &str, path: Option<&str>) -> Result<Project> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, Project>(
            r#"
            INSERT INTO projects (id, name, path)
            VALUES ($1, $2, $3)
            ON CONFLICT (name) DO UPDATE SET
                path = COALESCE(EXCLUDED.path, projects.path)
            RETURNING id, name, path, description, created_at
            "#,
        )
        .bind(&id)
        .bind(name)
        .bind(path)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn get_by_name(pool: &PgPool, name: &str) -> Result<Option<Project>> {
        let row = sqlx::query_as::<_, Project>(
            r#"
            SELECT id, name, path, description, created_at
            FROM projects WHERE name = $1
            "#,
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id(pool: &PgPool, id: &str) -> Result<Option<Project>> {
        let row = sqlx::query_as::<_, Project>(
            r#"
            SELECT id, name, path, description, created_at
            FROM projects WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn list(pool: &PgPool) -> Result<Vec<Project>> {
        let rows = sqlx::query_as::<_, Project>(
            r#"
            SELECT id, name, path, description, created_at
            FROM projects
            ORDER BY name ASC
            "#,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}
