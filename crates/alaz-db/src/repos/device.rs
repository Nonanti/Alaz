use alaz_core::models::Device;
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

pub struct DeviceRepo;

impl DeviceRepo {
    /// Register a new device. Returns the device (trusted=false by default).
    pub async fn register(
        pool: &PgPool,
        owner_id: &str,
        fingerprint: &str,
        name: Option<&str>,
    ) -> Result<Device> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, Device>(
            r#"
            INSERT INTO devices (id, owner_id, fingerprint, name)
            VALUES ($1, $2, $3, $4)
            RETURNING id, owner_id, fingerprint, name, trusted, last_seen_at, created_at
            "#,
        )
        .bind(&id)
        .bind(owner_id)
        .bind(fingerprint)
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.constraint() == Some("devices_fingerprint_key") => {
                AlazError::Validation("device fingerprint already registered".to_string())
            }
            _ => e.into(),
        })?;

        Ok(row)
    }

    /// Get a device by its fingerprint.
    pub async fn get_by_fingerprint(pool: &PgPool, fingerprint: &str) -> Result<Option<Device>> {
        let row = sqlx::query_as::<_, Device>(
            r#"
            SELECT id, owner_id, fingerprint, name, trusted, last_seen_at, created_at
            FROM devices WHERE fingerprint = $1
            "#,
        )
        .bind(fingerprint)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Update last_seen_at for a device.
    pub async fn touch(pool: &PgPool, fingerprint: &str) -> Result<()> {
        sqlx::query("UPDATE devices SET last_seen_at = now() WHERE fingerprint = $1")
            .bind(fingerprint)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Mark a device as trusted.
    pub async fn approve(pool: &PgPool, id: &str) -> Result<Device> {
        let row = sqlx::query_as::<_, Device>(
            r#"
            UPDATE devices SET trusted = TRUE
            WHERE id = $1
            RETURNING id, owner_id, fingerprint, name, trusted, last_seen_at, created_at
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("device {id}")))?;

        Ok(row)
    }

    /// Revoke trust from a device.
    pub async fn revoke(pool: &PgPool, id: &str) -> Result<Device> {
        let row = sqlx::query_as::<_, Device>(
            r#"
            UPDATE devices SET trusted = FALSE
            WHERE id = $1
            RETURNING id, owner_id, fingerprint, name, trusted, last_seen_at, created_at
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("device {id}")))?;

        Ok(row)
    }

    /// Delete a device.
    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM devices WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("device {id}")));
        }

        Ok(())
    }

    /// List all devices, optionally filtered by owner.
    pub async fn list(pool: &PgPool, owner_id: Option<&str>) -> Result<Vec<Device>> {
        let rows = if let Some(oid) = owner_id {
            sqlx::query_as::<_, Device>(
                r#"
                SELECT id, owner_id, fingerprint, name, trusted, last_seen_at, created_at
                FROM devices WHERE owner_id = $1
                ORDER BY created_at DESC
                "#,
            )
            .bind(oid)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, Device>(
                r#"
                SELECT id, owner_id, fingerprint, name, trusted, last_seen_at, created_at
                FROM devices
                ORDER BY created_at DESC
                "#,
            )
            .fetch_all(pool)
            .await?
        };

        Ok(rows)
    }
}
