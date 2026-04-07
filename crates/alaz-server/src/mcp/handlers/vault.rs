use alaz_db::repos::*;

use super::super::helpers::*;
use super::super::params::*;
use crate::state::AppState;

pub(crate) async fn store(state: &AppState, params: VaultStoreParams) -> Result<String, String> {
    let crypto = state
        .vault_crypto
        .as_ref()
        .ok_or_else(|| "vault not configured: set VAULT_MASTER_KEY".to_string())?;
    let (encrypted, nonce) = crypto
        .encrypt(params.value.as_bytes())
        .map_err(|e| format!("encryption failed: {e}"))?;
    let owner_id = default_owner_id(&state.pool).await?;
    let secret = VaultRepo::store(
        &state.pool,
        &owner_id,
        &params.name,
        &encrypted,
        &nonce,
        params.description.as_deref(),
    )
    .await
    .map_err(|e| format!("vault store failed: {e}"))?;
    Ok(serde_json::json!({
        "id": secret.id,
        "name": secret.name,
        "description": secret.description,
        "created_at": secret.created_at,
    })
    .to_string())
}

pub(crate) async fn get(state: &AppState, params: VaultGetParams) -> Result<String, String> {
    let crypto = state
        .vault_crypto
        .as_ref()
        .ok_or_else(|| "vault not configured: set VAULT_MASTER_KEY".to_string())?;
    let owner_id = default_owner_id(&state.pool).await?;
    let secret = VaultRepo::get_by_name(&state.pool, &owner_id, &params.name)
        .await
        .map_err(|e| format!("vault get failed: {e}"))?;
    let plaintext = crypto
        .decrypt(&secret.encrypted_value, &secret.nonce)
        .map_err(|e| format!("decryption failed: {e}"))?;
    let value =
        String::from_utf8(plaintext).map_err(|e| format!("value is not valid UTF-8: {e}"))?;
    Ok(serde_json::json!({
        "name": secret.name,
        "value": value,
        "description": secret.description,
    })
    .to_string())
}

pub(crate) async fn list(state: &AppState, _params: VaultListParams) -> Result<String, String> {
    let owner_id = default_owner_id(&state.pool).await?;
    let secrets = VaultRepo::list(&state.pool, &owner_id)
        .await
        .map_err(|e| format!("vault list failed: {e}"))?;
    let names: Vec<_> = secrets
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
    serde_json::to_string_pretty(&names).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn delete(state: &AppState, params: VaultDeleteParams) -> Result<String, String> {
    let owner_id = default_owner_id(&state.pool).await?;
    VaultRepo::delete(&state.pool, &owner_id, &params.name)
        .await
        .map_err(|e| format!("vault delete failed: {e}"))?;
    Ok(format!("deleted vault secret '{}'", params.name))
}
