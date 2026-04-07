use alaz_core::models::ApiKey;
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

pub struct ApiKeyRepo;

impl ApiKeyRepo {
    /// Create a new API key record (key_hash is pre-computed by the caller).
    pub async fn create(
        pool: &PgPool,
        owner_id: &str,
        key_hash: &str,
        name: Option<&str>,
    ) -> Result<ApiKey> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, ApiKey>(
            r#"
            INSERT INTO api_keys (id, owner_id, key_hash, name)
            VALUES ($1, $2, $3, $4)
            RETURNING id, owner_id, key_hash, name, last_used_at, created_at
            "#,
        )
        .bind(&id)
        .bind(owner_id)
        .bind(key_hash)
        .bind(name)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Verify an API key by hash, update last_used_at, return owner_id.
    pub async fn verify(pool: &PgPool, key_hash: &str) -> Result<String> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"
            UPDATE api_keys
            SET last_used_at = now()
            WHERE key_hash = $1
            RETURNING owner_id
            "#,
        )
        .bind(key_hash)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::Auth("invalid API key".to_string()))?;

        Ok(row.0)
    }

    /// List API keys for an owner.
    pub async fn list(pool: &PgPool, owner_id: Option<&str>) -> Result<Vec<ApiKey>> {
        let rows = if let Some(oid) = owner_id {
            sqlx::query_as::<_, ApiKey>(
                r#"
                SELECT id, owner_id, key_hash, name, last_used_at, created_at
                FROM api_keys WHERE owner_id = $1
                ORDER BY created_at DESC
                "#,
            )
            .bind(oid)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, ApiKey>(
                r#"
                SELECT id, owner_id, key_hash, name, last_used_at, created_at
                FROM api_keys
                ORDER BY created_at DESC
                "#,
            )
            .fetch_all(pool)
            .await?
        };

        Ok(rows)
    }

    /// Revoke (delete) an API key.
    pub async fn revoke(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM api_keys WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("api_key {id}")));
        }

        Ok(())
    }
}
