use alaz_core::models::*;
use alaz_db::repos::*;

use super::super::helpers::*;
use super::super::params::*;
use crate::state::AppState;

pub(crate) async fn sessions(state: &AppState, params: SessionsParams) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let filter = ListSessionsFilter {
        project: project_id,
        status: params.status,
        limit: params.limit,
        offset: params.offset,
    };
    let sessions = SessionRepo::list(&state.pool, &filter)
        .await
        .map_err(|e| format!("sessions failed: {e}"))?;
    serde_json::to_string_pretty(&sessions).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn checkpoint_save(
    state: &AppState,
    params: CheckpointSaveParams,
) -> Result<String, String> {
    let checkpoint = SessionRepo::save_checkpoint(&state.pool, &params.session_id, &params.data)
        .await
        .map_err(|e| format!("checkpoint save failed: {e}"))?;
    serde_json::to_string_pretty(&checkpoint).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn checkpoint_list(
    state: &AppState,
    params: CheckpointGetParams,
) -> Result<String, String> {
    let checkpoints = SessionRepo::get_checkpoints(&state.pool, &params.session_id)
        .await
        .map_err(|e| format!("checkpoint list failed: {e}"))?;
    serde_json::to_string_pretty(&checkpoints).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn checkpoint_restore(
    state: &AppState,
    params: CheckpointRestoreParams,
) -> Result<String, String> {
    let checkpoint = SessionRepo::get_latest_checkpoint(&state.pool, &params.session_id)
        .await
        .map_err(|e| format!("checkpoint restore failed: {e}"))?;
    match checkpoint {
        Some(cp) => serde_json::to_string_pretty(&cp).map_err(|e| format!("json error: {e}")),
        None => Ok(r#"{"status": "no checkpoint found"}"#.to_string()),
    }
}

pub(crate) async fn compact_restore(
    state: &AppState,
    params: CompactRestoreParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let restorer = alaz_intel::CompactRestorer::new(state.pool.clone());
    let result = restorer
        .build_restore_context(
            &params.session_id,
            project_id.as_deref(),
            params.message_limit,
        )
        .await
        .map_err(|e| format!("compact restore failed: {e}"))?;

    Ok(result.formatted_output)
}
