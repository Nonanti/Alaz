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

pub(crate) async fn search_transcripts(
    state: &AppState,
    params: SessionSearchParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let limit = params.limit.unwrap_or(10);

    let results =
        SessionRepo::search_transcripts(&state.pool, &params.query, project_id.as_deref(), limit)
            .await
            .map_err(|e| format!("session search failed: {e}"))?;

    if results.is_empty() {
        return Ok(format!("No sessions found matching \"{}\"", params.query));
    }

    let mut output = format!(
        "## Session Search: \"{}\" ({} results)\n\n",
        params.query,
        results.len()
    );

    for r in &results {
        let summary = r.summary.as_deref().unwrap_or("No summary");
        output.push_str(&format!(
            "### {} (rank: {:.2})\n**Project**: {} | **Date**: {}\n{}\n> {}\n\n",
            r.id,
            r.rank,
            r.project_id.as_deref().unwrap_or("-"),
            r.created_at.format("%Y-%m-%d %H:%M"),
            summary,
            r.headline,
        ));
    }

    Ok(output)
}

// --- Session State ---

pub(crate) async fn update_session_state(
    state: &AppState,
    params: UpdateSessionStateParams,
) -> Result<String, String> {
    SessionRepo::update_session_state(
        &state.pool,
        &params.session_id,
        params.goals.as_deref(),
        params.accomplished.as_deref(),
        params.pending.as_deref(),
        params.handoff_summary.as_deref(),
        params.current_task.as_deref(),
        params.related_files.as_deref(),
        None, // working_context
    )
    .await
    .map_err(|e| format!("update session state failed: {e}"))?;

    Ok(format!(
        "Session state updated for `{}`.",
        params.session_id
    ))
}

pub(crate) async fn get_session_state(
    state: &AppState,
    params: GetSessionStateParams,
) -> Result<String, String> {
    let ss = SessionRepo::get_session_state(&state.pool, &params.session_id)
        .await
        .map_err(|e| format!("get session state failed: {e}"))?;

    serde_json::to_string_pretty(&ss).map_err(|e| format!("json error: {e}"))
}

// --- Work Units ---

pub(crate) async fn create_work_unit(
    state: &AppState,
    params: CreateWorkUnitParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;

    let work_unit = WorkUnitRepo::create(
        &state.pool,
        &params.name,
        params.description.as_deref(),
        params.goal.as_deref(),
        project_id.as_deref(),
    )
    .await
    .map_err(|e| format!("create work unit failed: {e}"))?;

    serde_json::to_string_pretty(&work_unit).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn list_work_units(
    state: &AppState,
    params: ListWorkUnitsParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;

    let units = WorkUnitRepo::list(&state.pool, project_id.as_deref(), params.status.as_deref())
        .await
        .map_err(|e| format!("list work units failed: {e}"))?;

    if units.is_empty() {
        return Ok("No work units found.".to_string());
    }

    let mut output = format!("## Work Units ({} total)\n\n", units.len());
    output.push_str("| ID | Name | Status | Goal |\n");
    output.push_str("|----|------|--------|------|\n");

    for u in &units {
        let goal = u.goal.as_deref().unwrap_or("-");
        let goal_truncated = truncate_content(goal, 60);
        output.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            &u.id[..8.min(u.id.len())],
            u.name,
            u.status,
            goal_truncated,
        ));
    }

    Ok(output)
}

pub(crate) async fn update_work_unit(
    state: &AppState,
    params: UpdateWorkUnitParams,
) -> Result<String, String> {
    WorkUnitRepo::update_status(&state.pool, &params.id, &params.status)
        .await
        .map_err(|e| format!("update work unit failed: {e}"))?;

    Ok(format!(
        "Work unit `{}` status updated to **{}**.",
        params.id, params.status
    ))
}

pub(crate) async fn link_session_work_unit(
    state: &AppState,
    params: LinkSessionWorkUnitParams,
) -> Result<String, String> {
    WorkUnitRepo::link_session(&state.pool, &params.session_id, &params.work_unit_id)
        .await
        .map_err(|e| format!("link session to work unit failed: {e}"))?;

    Ok(format!(
        "Session `{}` linked to work unit `{}`.",
        params.session_id, params.work_unit_id
    ))
}

// --- Session Messages ---

pub(crate) async fn search_messages(
    state: &AppState,
    params: SearchMessagesParams,
) -> Result<String, String> {
    let limit = params.limit.unwrap_or(20);

    let results = SessionMessageRepo::search(
        &state.pool,
        &params.query,
        params.session_id.as_deref(),
        params.role.as_deref(),
        limit,
    )
    .await
    .map_err(|e| format!("search messages failed: {e}"))?;

    if results.is_empty() {
        return Ok(format!("No messages found matching \"{}\"", params.query));
    }

    let mut output = format!(
        "## Message Search: \"{}\" ({} results)\n\n",
        params.query,
        results.len()
    );

    for r in &results {
        output.push_str(&format!(
            "### {} | **{}** | {} (rank: {:.2})\n> {}\n\n",
            r.session_id,
            r.role,
            r.created_at.format("%Y-%m-%d %H:%M"),
            r.rank,
            r.headline,
        ));
    }

    Ok(output)
}

pub(crate) async fn get_messages(
    state: &AppState,
    params: GetMessagesParams,
) -> Result<String, String> {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    let messages =
        SessionMessageRepo::list_by_session(&state.pool, &params.session_id, limit, offset)
            .await
            .map_err(|e| format!("get messages failed: {e}"))?;

    // Apply role filter in-memory if specified
    let messages: Vec<_> = if let Some(ref role) = params.role {
        messages.into_iter().filter(|m| m.role == *role).collect()
    } else {
        messages
    };

    if messages.is_empty() {
        return Ok(format!(
            "No messages found for session `{}`.",
            params.session_id
        ));
    }

    let mut output = format!(
        "## Messages for session `{}` ({} messages)\n\n",
        params.session_id,
        messages.len()
    );

    for m in &messages {
        let content_preview = truncate_content(&m.content, 300);
        output.push_str(&format!(
            "**[{}]** {} — {}\n\n",
            m.role,
            m.created_at.format("%Y-%m-%d %H:%M:%S"),
            content_preview,
        ));
    }

    Ok(output)
}
