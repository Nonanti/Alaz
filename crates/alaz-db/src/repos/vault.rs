use alaz_core::models::VaultSecret;
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

pub struct VaultRepo;

impl VaultRepo {
    pub async fn store(
        pool: &PgPool,
        owner_id: &str,
        name: &str,
        encrypted_value: &[u8],
        nonce: &[u8],
        description: Option<&str>,
    ) -> Result<VaultSecret> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, VaultSecret>(
            r#"
            INSERT INTO vault_secrets (id, owner_id, name, encrypted_value, nonce, description)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (owner_id, name) DO UPDATE SET
                encrypted_value = EXCLUDED.encrypted_value,
                nonce = EXCLUDED.nonce,
                description = EXCLUDED.description,
                updated_at = now()
            RETURNING id, owner_id, name, encrypted_value, nonce, description, created_at, updated_at
            "#,
        )
        .bind(&id)
        .bind(owner_id)
        .bind(name)
        .bind(encrypted_value)
        .bind(nonce)
        .bind(description)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    pub async fn get_by_name(pool: &PgPool, owner_id: &str, name: &str) -> Result<VaultSecret> {
        let row = sqlx::query_as::<_, VaultSecret>(
            r#"
            SELECT id, owner_id, name, encrypted_value, nonce, description, created_at, updated_at
            FROM vault_secrets
            WHERE owner_id = $1 AND name = $2
            "#,
        )
        .bind(owner_id)
        .bind(name)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("vault secret '{name}'")))?;

        Ok(row)
    }

    pub async fn list(pool: &PgPool, owner_id: &str) -> Result<Vec<VaultSecret>> {
        let rows = sqlx::query_as::<_, VaultSecret>(
            r#"
            SELECT id, owner_id, name, encrypted_value, nonce, description, created_at, updated_at
            FROM vault_secrets
            WHERE owner_id = $1
            ORDER BY name
            "#,
        )
        .bind(owner_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    pub async fn delete(pool: &PgPool, owner_id: &str, name: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM vault_secrets WHERE owner_id = $1 AND name = $2")
            .bind(owner_id)
            .bind(name)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("vault secret '{name}'")));
        }
        Ok(())
    }
}
