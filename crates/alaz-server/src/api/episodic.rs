use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
};
use serde::Deserialize;

use alaz_core::models::*;
use alaz_db::repos::{CoreMemoryRepo, EpisodeRepo, ProcedureRepo, ProjectRepo};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/episodes", get(list_episodes).post(create_episode))
        .route("/episodes/{id}/resolve", post(resolve_episode))
        .route("/procedures", get(list_procedures).post(create_procedure))
        .route("/procedures/{id}/outcome", post(record_procedure_outcome))
        .route(
            "/core-memory",
            get(list_core_memory).post(create_core_memory),
        )
        .route(
            "/core-memory/{id}",
            patch(update_core_memory).delete(delete_core_memory),
        )
        .route("/cue-search", post(cue_search))
        .route("/episodes/by-files", post(episodes_by_files))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Episodes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct EpisodesQuery {
    #[serde(rename = "type")]
    kind: Option<String>,
    project: Option<String>,
    resolved: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_episodes(
    State(state): State<AppState>,
    Query(q): Query<EpisodesQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = q.project {
        ProjectRepo::get_by_name(&state.pool, name)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    let filter = ListEpisodesFilter {
        project: project_id,
        kind: q.kind,
        resolved: q.resolved,
        limit: q.limit,
        offset: q.offset,
    };

    let episodes = EpisodeRepo::list(&state.pool, &filter).await?;
    let v = serde_json::to_value(episodes)?;
    Ok((StatusCode::OK, Json(v)))
}

// ---------------------------------------------------------------------------
// Episode Create
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateEpisodeBody {
    title: String,
    content: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    severity: Option<String>,
    resolved: Option<bool>,
    who_cues: Option<Vec<String>>,
    what_cues: Option<Vec<String>>,
    where_cues: Option<Vec<String>>,
    when_cues: Option<Vec<String>>,
    why_cues: Option<Vec<String>>,
    project: Option<String>,
    action: Option<String>,
    outcome: Option<String>,
    outcome_score: Option<f64>,
    related_files: Option<Vec<String>>,
}

async fn create_episode(
    State(state): State<AppState>,
    Json(body): Json<CreateEpisodeBody>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_or_create(&state.pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    let input = CreateEpisode {
        title: body.title,
        content: body.content,
        kind: body.kind,
        severity: body.severity,
        resolved: body.resolved,
        who_cues: body.who_cues,
        what_cues: body.what_cues,
        where_cues: body.where_cues,
        when_cues: body.when_cues,
        why_cues: body.why_cues,
        project: None,
        source: Some("pi-extension".to_string()),
        source_metadata: None,
        action: body.action,
        outcome: body.outcome,
        outcome_score: body.outcome_score.map(|s| s.clamp(-1.0, 1.0)),
        related_files: body.related_files,
    };

    let episode = EpisodeRepo::create(&state.pool, &input, project_id.as_deref()).await?;
    let v = serde_json::to_value(episode)?;
    Ok((StatusCode::CREATED, Json(v)))
}

async fn resolve_episode(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let r = sqlx::query("UPDATE episodes SET resolved = true, updated_at = now() WHERE id = $1")
        .bind(&id)
        .execute(&state.pool)
        .await?;

    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound("episode not found".into()));
    }
    Ok(StatusCode::OK)
}

// ---------------------------------------------------------------------------
// Procedures
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ProceduresQuery {
    project: Option<String>,
    tag: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Procedure Create
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateProcedureBody {
    title: String,
    content: String,
    steps: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    project: Option<String>,
}

async fn create_procedure(
    State(state): State<AppState>,
    Json(body): Json<CreateProcedureBody>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_or_create(&state.pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    let steps_json = body
        .steps
        .map(|s| serde_json::to_value(s).unwrap_or_default());

    let input = CreateProcedure {
        title: body.title,
        content: body.content,
        steps: steps_json,
        project: None,
        tags: body.tags,
        source: Some("pi-extension".to_string()),
        source_metadata: None,
    };

    let proc_ = ProcedureRepo::create(&state.pool, &input, project_id.as_deref()).await?;
    let v = serde_json::to_value(proc_)?;
    Ok((StatusCode::CREATED, Json(v)))
}

async fn list_procedures(
    State(state): State<AppState>,
    Query(q): Query<ProceduresQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = q.project {
        ProjectRepo::get_by_name(&state.pool, name)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    let filter = ListProceduresFilter {
        project: project_id,
        tag: q.tag,
        limit: q.limit,
        offset: q.offset,
    };

    let procedures = ProcedureRepo::list(&state.pool, &filter).await?;
    let v = serde_json::to_value(procedures)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(Deserialize)]
struct OutcomeBody {
    success: bool,
}

async fn record_procedure_outcome(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<OutcomeBody>,
) -> Result<impl IntoResponse, ApiError> {
    ProcedureRepo::record_outcome(&state.pool, &id, body.success).await?;
    let proc_ = ProcedureRepo::get(&state.pool, &id).await?;
    let v = serde_json::to_value(proc_)?;
    Ok((StatusCode::OK, Json(v)))
}

// ---------------------------------------------------------------------------
// Core Memory
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CoreMemoryQuery {
    category: Option<String>,
    project: Option<String>,
    limit: Option<i64>,
}

async fn list_core_memory(
    State(state): State<AppState>,
    Query(q): Query<CoreMemoryQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = q.project {
        ProjectRepo::get_by_name(&state.pool, name)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    let filter = ListCoreMemoryFilter {
        project: project_id,
        category: q.category,
        limit: q.limit,
        offset: None,
    };

    let memories = CoreMemoryRepo::list(&state.pool, &filter).await?;
    let v = serde_json::to_value(memories)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(Deserialize)]
struct CreateCoreMemoryBody {
    category: String,
    key: String,
    value: String,
    confidence: Option<f64>,
    project_id: Option<String>,
}

async fn create_core_memory(
    State(state): State<AppState>,
    Json(body): Json<CreateCoreMemoryBody>,
) -> Result<impl IntoResponse, ApiError> {
    // Resolve project_id: if the caller passed a name, look it up
    let project_id = if let Some(ref proj) = body.project_id {
        // Try to resolve as project name first; if that fails, use as-is (assume it's an ID)
        match ProjectRepo::get_by_name(&state.pool, proj).await {
            Ok(Some(p)) => Some(p.id),
            _ => Some(proj.clone()),
        }
    } else {
        None
    };

    let input = UpsertCoreMemory {
        category: body.category,
        key: body.key,
        value: body.value,
        confidence: body.confidence,
        project: None, // not used by repo; project_id is passed separately
    };

    let memory = CoreMemoryRepo::upsert(&state.pool, &input, project_id.as_deref()).await?;
    let v = serde_json::to_value(memory)?;
    Ok((StatusCode::CREATED, Json(v)))
}

#[derive(Deserialize)]
struct UpdateCoreMemoryBody {
    value: Option<String>,
    confidence: Option<f64>,
}

async fn update_core_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateCoreMemoryBody>,
) -> Result<impl IntoResponse, ApiError> {
    // Fetch existing memory
    let existing = CoreMemoryRepo::get(&state.pool, &id).await?;

    // Merge with provided fields
    let input = UpsertCoreMemory {
        category: existing.category,
        key: existing.key,
        value: body.value.unwrap_or(existing.value),
        confidence: Some(body.confidence.unwrap_or(existing.confidence)),
        project: None,
    };

    let memory =
        CoreMemoryRepo::upsert(&state.pool, &input, existing.project_id.as_deref()).await?;
    let v = serde_json::to_value(memory)?;
    Ok((StatusCode::OK, Json(v)))
}

async fn delete_core_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    CoreMemoryRepo::delete(&state.pool, &id).await?;
    Ok(StatusCode::OK)
}

// ---------------------------------------------------------------------------
// Cue Search
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CueSearchBody {
    who: Option<Vec<String>>,
    what: Option<Vec<String>>,
    #[serde(rename = "where")]
    where_cues: Option<Vec<String>>,
    #[serde(rename = "when")]
    when_cues: Option<Vec<String>>,
    why: Option<Vec<String>>,
    project: Option<String>,
    limit: Option<i64>,
}

async fn cue_search(
    State(state): State<AppState>,
    Json(body): Json<CueSearchBody>,
) -> Result<impl IntoResponse, ApiError> {
    let episodes = EpisodeRepo::cue_search(
        &state.pool,
        body.who.as_deref(),
        body.what.as_deref(),
        body.where_cues.as_deref(),
        body.when_cues.as_deref(),
        body.why.as_deref(),
        body.project.as_deref(),
        body.limit.map(|l| l.min(100)),
    )
    .await?;
    let v = serde_json::to_value(episodes)?;
    Ok((StatusCode::OK, Json(v)))
}

// ---------------------------------------------------------------------------
// Episodes by Files
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct EpisodesByFilesBody {
    file_paths: Vec<String>,
    limit: Option<i64>,
}

async fn episodes_by_files(
    State(state): State<AppState>,
    Json(body): Json<EpisodesByFilesBody>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = body.limit.unwrap_or(20);
    let refs: Vec<&str> = body.file_paths.iter().map(|s| s.as_str()).collect();

    let episodes = EpisodeRepo::find_by_files(&state.pool, &refs, limit).await?;
    let v = serde_json::to_value(episodes)?;
    Ok((StatusCode::OK, Json(v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episodes_query_defaults() {
        let q: EpisodesQuery = serde_json::from_str("{}").unwrap();
        assert!(q.kind.is_none());
        assert!(q.project.is_none());
        assert!(q.resolved.is_none());
        assert!(q.limit.is_none());
        assert!(q.offset.is_none());
    }

    #[test]
    fn episodes_query_all_fields() {
        let q: EpisodesQuery = serde_json::from_str(
            r#"{"type":"error","project":"alaz","resolved":true,"limit":10,"offset":5}"#,
        )
        .unwrap();
        assert_eq!(q.kind.as_deref(), Some("error"));
        assert_eq!(q.project.as_deref(), Some("alaz"));
        assert_eq!(q.resolved, Some(true));
        assert_eq!(q.limit, Some(10));
        assert_eq!(q.offset, Some(5));
    }

    #[test]
    fn create_episode_body_minimal() {
        let b: CreateEpisodeBody = serde_json::from_str(r#"{"title":"t","content":"c"}"#).unwrap();
        assert_eq!(b.title, "t");
        assert_eq!(b.content, "c");
        assert!(b.kind.is_none());
        assert!(b.severity.is_none());
        assert!(b.resolved.is_none());
        assert!(b.who_cues.is_none());
        assert!(b.what_cues.is_none());
        assert!(b.where_cues.is_none());
        assert!(b.when_cues.is_none());
        assert!(b.why_cues.is_none());
        assert!(b.project.is_none());
        assert!(b.action.is_none());
        assert!(b.outcome.is_none());
        assert!(b.outcome_score.is_none());
        assert!(b.related_files.is_none());
    }

    #[test]
    fn create_episode_body_full() {
        let b: CreateEpisodeBody = serde_json::from_str(
            r#"{
                "title": "Bug found",
                "content": "Deadlock in cache",
                "type": "error",
                "severity": "high",
                "resolved": false,
                "who_cues": ["alice"],
                "what_cues": ["deadlock"],
                "where_cues": ["cache.rs"],
                "when_cues": ["2026-04-01"],
                "why_cues": ["held lock across await"],
                "project": "alaz",
                "action": "fix mutex",
                "outcome": "resolved",
                "outcome_score": 0.9,
                "related_files": ["src/cache.rs", "src/pipeline.rs"]
            }"#,
        )
        .unwrap();
        assert_eq!(b.title, "Bug found");
        assert_eq!(b.kind.as_deref(), Some("error"));
        assert_eq!(b.severity.as_deref(), Some("high"));
        assert_eq!(b.resolved, Some(false));
        assert_eq!(b.who_cues.as_ref().unwrap().len(), 1);
        assert_eq!(b.what_cues.as_ref().unwrap()[0], "deadlock");
        assert_eq!(b.where_cues.as_ref().unwrap()[0], "cache.rs");
        assert_eq!(b.when_cues.as_ref().unwrap()[0], "2026-04-01");
        assert_eq!(b.why_cues.as_ref().unwrap()[0], "held lock across await");
        assert_eq!(b.project.as_deref(), Some("alaz"));
        assert_eq!(b.action.as_deref(), Some("fix mutex"));
        assert_eq!(b.outcome.as_deref(), Some("resolved"));
        assert_eq!(b.outcome_score, Some(0.9));
        assert_eq!(b.related_files.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn create_procedure_body_minimal() {
        let b: CreateProcedureBody =
            serde_json::from_str(r#"{"title":"deploy","content":"run deploy.sh"}"#).unwrap();
        assert_eq!(b.title, "deploy");
        assert_eq!(b.content, "run deploy.sh");
        assert!(b.steps.is_none());
        assert!(b.tags.is_none());
        assert!(b.project.is_none());
    }

    #[test]
    fn procedures_query_defaults() {
        let q: ProceduresQuery = serde_json::from_str("{}").unwrap();
        assert!(q.project.is_none());
        assert!(q.tag.is_none());
        assert!(q.limit.is_none());
        assert!(q.offset.is_none());
    }

    #[test]
    fn outcome_body_valid() {
        let b: OutcomeBody = serde_json::from_str(r#"{"success":true}"#).unwrap();
        assert!(b.success);

        let b: OutcomeBody = serde_json::from_str(r#"{"success":false}"#).unwrap();
        assert!(!b.success);
    }

    #[test]
    fn outcome_body_missing_field() {
        let r = serde_json::from_str::<OutcomeBody>("{}");
        assert!(r.is_err());
    }

    #[test]
    fn cue_search_body_empty() {
        let b: CueSearchBody = serde_json::from_str("{}").unwrap();
        assert!(b.who.is_none());
        assert!(b.what.is_none());
        assert!(b.where_cues.is_none());
        assert!(b.when_cues.is_none());
        assert!(b.why.is_none());
        assert!(b.project.is_none());
        assert!(b.limit.is_none());
    }

    #[test]
    fn cue_search_body_with_renamed_fields() {
        let b: CueSearchBody = serde_json::from_str(
            r#"{
                "who": ["alice"],
                "what": ["deadlock"],
                "where": ["cache.rs"],
                "when": ["yesterday"],
                "why": ["debugging"],
                "project": "alaz",
                "limit": 5
            }"#,
        )
        .unwrap();
        assert_eq!(b.who.as_ref().unwrap(), &["alice"]);
        assert_eq!(b.what.as_ref().unwrap(), &["deadlock"]);
        assert_eq!(b.where_cues.as_ref().unwrap(), &["cache.rs"]);
        assert_eq!(b.when_cues.as_ref().unwrap(), &["yesterday"]);
        assert_eq!(b.why.as_ref().unwrap(), &["debugging"]);
        assert_eq!(b.project.as_deref(), Some("alaz"));
        assert_eq!(b.limit, Some(5));
    }

    #[test]
    fn episodes_by_files_body_valid() {
        let b: EpisodesByFilesBody =
            serde_json::from_str(r#"{"file_paths":["src/main.rs","src/lib.rs"],"limit":10}"#)
                .unwrap();
        assert_eq!(b.file_paths.len(), 2);
        assert_eq!(b.file_paths[0], "src/main.rs");
        assert_eq!(b.limit, Some(10));
    }

    #[test]
    fn episodes_by_files_body_no_limit() {
        let b: EpisodesByFilesBody = serde_json::from_str(r#"{"file_paths":["a.rs"]}"#).unwrap();
        assert_eq!(b.file_paths.len(), 1);
        assert!(b.limit.is_none());
    }

    #[test]
    fn episodes_by_files_body_missing_paths() {
        let r = serde_json::from_str::<EpisodesByFilesBody>("{}");
        assert!(r.is_err());
    }

    #[test]
    fn create_core_memory_body_minimal() {
        let b: CreateCoreMemoryBody =
            serde_json::from_str(r#"{"category":"fact","key":"db_port","value":"5434"}"#).unwrap();
        assert_eq!(b.category, "fact");
        assert_eq!(b.key, "db_port");
        assert_eq!(b.value, "5434");
        assert!(b.confidence.is_none());
        assert!(b.project_id.is_none());
    }

    #[test]
    fn create_core_memory_body_full() {
        let b: CreateCoreMemoryBody = serde_json::from_str(
            r#"{"category":"preference","key":"lang","value":"tr","confidence":0.95,"project_id":"proj1"}"#,
        )
        .unwrap();
        assert_eq!(b.category, "preference");
        assert_eq!(b.confidence, Some(0.95));
        assert_eq!(b.project_id.as_deref(), Some("proj1"));
    }

    #[test]
    fn update_core_memory_body_empty() {
        let b: UpdateCoreMemoryBody = serde_json::from_str("{}").unwrap();
        assert!(b.value.is_none());
        assert!(b.confidence.is_none());
    }

    #[test]
    fn update_core_memory_body_partial() {
        let b: UpdateCoreMemoryBody = serde_json::from_str(r#"{"confidence":0.8}"#).unwrap();
        assert!(b.value.is_none());
        assert_eq!(b.confidence, Some(0.8));
    }

    #[test]
    fn core_memory_query_defaults() {
        let q: CoreMemoryQuery = serde_json::from_str("{}").unwrap();
        assert!(q.category.is_none());
        assert!(q.project.is_none());
        assert!(q.limit.is_none());
    }
}
