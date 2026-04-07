use alaz_core::Result;
use alaz_core::models::AuditLog;
use sqlx::PgPool;

pub struct AuditRepo;

impl AuditRepo {
    /// Log an audit event.
    pub async fn log(
        pool: &PgPool,
        owner_id: Option<&str>,
        event: &str,
        details: serde_json::Value,
        ip_address: Option<&str>,
    ) -> Result<AuditLog> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, AuditLog>(
            r#"
            INSERT INTO audit_logs (id, owner_id, event, details, ip_address)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, owner_id, event, details, ip_address, created_at
            "#,
        )
        .bind(&id)
        .bind(owner_id)
        .bind(event)
        .bind(&details)
        .bind(ip_address)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// List recent audit logs, optionally filtered by owner or event.
    pub async fn list(
        pool: &PgPool,
        owner_id: Option<&str>,
        event: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AuditLog>> {
        let rows = sqlx::query_as::<_, AuditLog>(
            r#"
            SELECT id, owner_id, event, details, ip_address, created_at
            FROM audit_logs
            WHERE ($1::TEXT IS NULL OR owner_id = $1)
              AND ($2::TEXT IS NULL OR event = $2)
            ORDER BY created_at DESC
            LIMIT $3
            "#,
        )
        .bind(owner_id)
        .bind(event)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}
