use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;

use alaz_db::repos::{OwnerRepo, VaultRepo};

use crate::error::ApiError;
use crate::state::AppState;

async fn get_owner_id(pool: &sqlx::PgPool) -> Result<String, ApiError> {
    OwnerRepo::list(pool)
        .await
        .ok()
        .and_then(|owners| owners.into_iter().next().map(|o| o.id))
        .ok_or_else(|| ApiError::Internal("no owner found".into()))
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/vault", get(vault_list).post(vault_store))
        .route("/vault/{name}", get(vault_get).delete(vault_delete))
        .with_state(state)
}

async fn vault_list(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let owner_id = get_owner_id(&state.pool).await?;
    let secrets = VaultRepo::list(&state.pool, &owner_id).await?;
    let names: Vec<serde_json::Value> = secrets
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            })
        })
        .collect();
    let v = serde_json::to_value(names)?;
    Ok((StatusCode::OK, Json(v)))
}

async fn vault_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id = get_owner_id(&state.pool).await?;
    let crypto = state
        .vault_crypto
        .as_ref()
        .ok_or_else(|| ApiError::Internal("vault not configured".into()))?;

    let secret = VaultRepo::get_by_name(&state.pool, &owner_id, &name).await?;
    let plaintext = crypto
        .decrypt(&secret.encrypted_value, &secret.nonce)
        .map_err(|e| ApiError::Internal(format!("decrypt failed: {e}")))?;

    let value = String::from_utf8_lossy(&plaintext).into_owned();
    let result = serde_json::json!({
        "name": secret.name,
        "value": value,
        "description": secret.description,
    });
    Ok((StatusCode::OK, Json(result)))
}

#[derive(Deserialize)]
struct VaultStoreBody {
    name: String,
    value: String,
    description: Option<String>,
}

async fn vault_store(
    State(state): State<AppState>,
    Json(body): Json<VaultStoreBody>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id = get_owner_id(&state.pool).await?;
    let crypto = state
        .vault_crypto
        .as_ref()
        .ok_or_else(|| ApiError::Internal("vault not configured".into()))?;

    let (encrypted, nonce) = crypto
        .encrypt(body.value.as_bytes())
        .map_err(|e| ApiError::Internal(format!("encrypt failed: {e}")))?;

    VaultRepo::store(
        &state.pool,
        &owner_id,
        &body.name,
        &encrypted,
        &nonce,
        body.description.as_deref(),
    )
    .await?;

    Ok((StatusCode::OK, "stored"))
}

async fn vault_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let owner_id = get_owner_id(&state.pool).await?;
    VaultRepo::delete(&state.pool, &owner_id, &name).await?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vault_store_body_minimal() {
        let body: VaultStoreBody = serde_json::from_value(json!({
            "name": "API_KEY",
            "value": "sk-secret-123"
        }))
        .unwrap();

        assert_eq!(body.name, "API_KEY");
        assert_eq!(body.value, "sk-secret-123");
        assert!(body.description.is_none());
    }

    #[test]
    fn vault_store_body_full() {
        let body: VaultStoreBody = serde_json::from_value(json!({
            "name": "DB_PASSWORD",
            "value": "super-secret",
            "description": "Production database password"
        }))
        .unwrap();

        assert_eq!(body.name, "DB_PASSWORD");
        assert_eq!(body.value, "super-secret");
        assert_eq!(
            body.description.as_deref(),
            Some("Production database password")
        );
    }

    #[test]
    fn vault_store_body_missing_required_field() {
        let result = serde_json::from_value::<VaultStoreBody>(json!({
            "name": "ONLY_NAME"
        }));
        assert!(result.is_err(), "should fail without 'value' field");

        let result = serde_json::from_value::<VaultStoreBody>(json!({
            "value": "only-value"
        }));
        assert!(result.is_err(), "should fail without 'name' field");

        let result = serde_json::from_value::<VaultStoreBody>(json!({}));
        assert!(result.is_err(), "should fail with empty object");
    }

    #[test]
    fn vault_store_body_empty_strings_valid() {
        let body: VaultStoreBody = serde_json::from_value(json!({
            "name": "",
            "value": ""
        }))
        .unwrap();

        assert_eq!(body.name, "");
        assert_eq!(body.value, "");
        assert!(body.description.is_none());
    }
}
