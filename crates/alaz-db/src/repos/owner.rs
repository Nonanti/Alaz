use alaz_core::models::Owner;
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

pub struct OwnerRepo;

impl OwnerRepo {
    /// Create a new owner with a pre-hashed password.
    pub async fn create(pool: &PgPool, username: &str, password_hash: &str) -> Result<Owner> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, Owner>(
            r#"
            INSERT INTO owners (id, username, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id, username, password_hash, created_at
            "#,
        )
        .bind(&id)
        .bind(username)
        .bind(password_hash)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.constraint() == Some("owners_username_key") => {
                AlazError::Validation(format!("username '{username}' already exists"))
            }
            _ => e.into(),
        })?;

        Ok(row)
    }

    /// Get an owner by username.
    pub async fn get_by_username(pool: &PgPool, username: &str) -> Result<Option<Owner>> {
        let row = sqlx::query_as::<_, Owner>(
            r#"
            SELECT id, username, password_hash, created_at
            FROM owners WHERE username = $1
            "#,
        )
        .bind(username)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Get an owner by ID.
    pub async fn get(pool: &PgPool, id: &str) -> Result<Owner> {
        let row = sqlx::query_as::<_, Owner>(
            r#"
            SELECT id, username, password_hash, created_at
            FROM owners WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("owner {id}")))?;

        Ok(row)
    }

    /// List all owners.
    pub async fn list(pool: &PgPool) -> Result<Vec<Owner>> {
        let rows = sqlx::query_as::<_, Owner>(
            r#"
            SELECT id, username, password_hash, created_at
            FROM owners ORDER BY created_at ASC
            "#,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}
