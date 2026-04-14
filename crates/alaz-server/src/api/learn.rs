use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
};
use serde::Deserialize;

use alaz_core::models::CreateReflection;
use alaz_db::repos::{LearningQueueRepo, ProjectRepo, ReflectionRepo, SessionRepo};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/sessions/{id}/learn", post(trigger_learn))
        .route("/sessions/{id}/checkpoint", post(save_checkpoint))
        .route(
            "/sessions/{id}/checkpoints",
            axum::routing::get(list_checkpoints),
        )
        .route("/reflections", post(create_reflection))
        .route("/raptor/rebuild", post(raptor_rebuild))
        .route("/raptor/status", axum::routing::get(raptor_status))
        .with_state(state)
}

#[derive(Deserialize)]
struct LearnBody {
    transcript: String,
    project: Option<String>,
}

/// Trigger the learning pipeline for a given session.
async fn trigger_learn(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<LearnBody>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_or_create(&state.pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    // Ensure session exists before saving transcript
    let _ = SessionRepo::ensure_exists(&state.pool, &session_id, project_id.as_deref()).await;

    // Save transcript text for FTS session search immediately
    let message_count = body.transcript.matches("[USER]:").count()
        + body.transcript.matches("[ASSISTANT]:").count();
    let _ = SessionRepo::update_transcript_text(
        &state.pool,
        &session_id,
        &body.transcript,
        None,
        message_count as i32,
    )
    .await;

    // Enqueue for debounced learning instead of running pipeline directly.
    // If the session is reopened and closed again, the newer transcript
    // replaces this one (old request gets cancelled automatically).
    let queue_id = LearningQueueRepo::enqueue(
        &state.pool,
        &session_id,
        project_id.as_deref(),
        &body.transcript,
        message_count as i32,
    )
    .await?;

    let response = serde_json::json!({
        "session_id": session_id,
        "queue_id": queue_id,
        "status": "queued",
        "message": "Learning queued. Will process after 3-minute cooldown if session stays closed.",
    });
    Ok((StatusCode::ACCEPTED, Json(response)))
}

#[derive(Deserialize)]
struct RaptorRebuildBody {
    project: Option<String>,
}

/// Trigger a RAPTOR tree rebuild.
async fn raptor_rebuild(
    State(state): State<AppState>,
    Json(body): Json<RaptorRebuildBody>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_or_create(&state.pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    let builder = alaz_intel::RaptorBuilder::new(
        state.pool.clone(),
        state.llm.clone(),
        state.embedding.clone(),
        state.qdrant.clone(),
    );

    let tree = builder.rebuild_tree(project_id.as_deref()).await?;
    let v = serde_json::to_value(tree)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(Deserialize)]
struct RaptorStatusQuery {
    project: Option<String>,
}

/// Get RAPTOR tree status.
async fn raptor_status(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<RaptorStatusQuery>,
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

    match alaz_db::repos::RaptorRepo::get_tree(&state.pool, project_id.as_deref()).await? {
        Some(tree) => {
            let v = serde_json::to_value(tree)?;
            Ok((StatusCode::OK, Json(v)))
        }
        None => {
            let response = serde_json::json!({"status": "no tree found"});
            Ok((StatusCode::OK, Json(response)))
        }
    }
}

// ---------------------------------------------------------------------------
// Session Checkpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CheckpointBody {
    summary: Option<String>,
    active_files: Option<Vec<String>>,
    current_task: Option<String>,
    blockers: Option<Vec<String>>,
}

async fn save_checkpoint(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<CheckpointBody>,
) -> Result<impl IntoResponse, ApiError> {
    SessionRepo::ensure_exists(&state.pool, &session_id, None).await?;

    let data = serde_json::json!({
        "summary": body.summary,
        "active_files": body.active_files,
        "current_task": body.current_task,
        "blockers": body.blockers,
    });

    let cp = SessionRepo::save_checkpoint(&state.pool, &session_id, &data).await?;
    let v = serde_json::to_value(cp)?;
    Ok((StatusCode::CREATED, Json(v)))
}

async fn list_checkpoints(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let cps = SessionRepo::get_checkpoints(&state.pool, &session_id).await?;
    let v = serde_json::to_value(cps)?;
    Ok((StatusCode::OK, Json(v)))
}

// ---------------------------------------------------------------------------
// Reflections
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateReflectionBody {
    session_id: String,
    what_worked: Option<String>,
    what_failed: Option<String>,
    lessons_learned: Option<String>,
    effectiveness_score: Option<f64>,
    project: Option<String>,
}

async fn create_reflection(
    State(state): State<AppState>,
    Json(body): Json<CreateReflectionBody>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_or_create(&state.pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    let input = CreateReflection {
        session_id: body.session_id,
        what_worked: body.what_worked,
        what_failed: body.what_failed,
        lessons_learned: body.lessons_learned,
        effectiveness_score: body.effectiveness_score,
        complexity_score: None,
        kind: Some("prompted".to_string()),
        action_items: None,
        overall_score: None,
        knowledge_score: None,
        decision_score: None,
        efficiency_score: None,
        evaluated_episode_ids: None,
        project: None,
    };

    let r = ReflectionRepo::create(&state.pool, &input, project_id.as_deref()).await?;
    let v = serde_json::to_value(r)?;
    Ok((StatusCode::CREATED, Json(v)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn learn_body_minimal() {
        let json = json!({ "transcript": "user asked about Rust" });
        let body: LearnBody = serde_json::from_value(json).unwrap();
        assert_eq!(body.transcript, "user asked about Rust");
        assert!(body.project.is_none());
    }

    #[test]
    fn learn_body_with_project() {
        let json = json!({ "transcript": "session log", "project": "alaz" });
        let body: LearnBody = serde_json::from_value(json).unwrap();
        assert_eq!(body.project.as_deref(), Some("alaz"));
    }

    #[test]
    fn raptor_rebuild_body_empty() {
        let json = json!({});
        let body: RaptorRebuildBody = serde_json::from_value(json).unwrap();
        assert!(body.project.is_none());
    }

    #[test]
    fn raptor_status_query_empty() {
        let json = json!({});
        let q: RaptorStatusQuery = serde_json::from_value(json).unwrap();
        assert!(q.project.is_none());
    }

    #[test]
    fn checkpoint_body_empty() {
        let json = json!({});
        let body: CheckpointBody = serde_json::from_value(json).unwrap();
        assert!(body.summary.is_none());
        assert!(body.active_files.is_none());
        assert!(body.current_task.is_none());
        assert!(body.blockers.is_none());
    }

    #[test]
    fn checkpoint_body_full() {
        let json = json!({
            "summary": "Working on tests",
            "active_files": ["src/main.rs", "src/lib.rs"],
            "current_task": "Add unit tests",
            "blockers": ["CI pipeline broken"]
        });
        let body: CheckpointBody = serde_json::from_value(json).unwrap();
        assert_eq!(body.summary.as_deref(), Some("Working on tests"));
        assert_eq!(body.active_files.as_ref().unwrap().len(), 2);
        assert_eq!(body.current_task.as_deref(), Some("Add unit tests"));
        assert_eq!(body.blockers.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn create_reflection_body_minimal() {
        let json = json!({ "session_id": "sess_001" });
        let body: CreateReflectionBody = serde_json::from_value(json).unwrap();
        assert_eq!(body.session_id, "sess_001");
        assert!(body.what_worked.is_none());
        assert!(body.what_failed.is_none());
        assert!(body.lessons_learned.is_none());
        assert!(body.effectiveness_score.is_none());
        assert!(body.project.is_none());
    }

    #[test]
    fn create_reflection_body_full() {
        let json = json!({
            "session_id": "sess_002",
            "what_worked": "Testing approach was efficient",
            "what_failed": "Missed edge case",
            "lessons_learned": "Always test boundaries",
            "effectiveness_score": 0.85,
            "project": "alaz"
        });
        let body: CreateReflectionBody = serde_json::from_value(json).unwrap();
        assert_eq!(body.session_id, "sess_002");
        assert_eq!(
            body.what_worked.as_deref(),
            Some("Testing approach was efficient")
        );
        assert!((body.effectiveness_score.unwrap() - 0.85).abs() < f64::EPSILON);
        assert_eq!(body.project.as_deref(), Some("alaz"));
    }
}
