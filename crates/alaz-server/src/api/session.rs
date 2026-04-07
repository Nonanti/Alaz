use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;

use alaz_core::models::ListSessionsFilter;
use alaz_db::repos::{ProjectRepo, SessionRepo};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/sessions", get(list_sessions))
        .with_state(state)
}

#[derive(Deserialize)]
struct SessionsQuery {
    project: Option<String>,
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_sessions(
    State(state): State<AppState>,
    Query(q): Query<SessionsQuery>,
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

    let filter = ListSessionsFilter {
        project: project_id,
        status: q.status,
        limit: q.limit,
        offset: q.offset,
    };

    let sessions = SessionRepo::list(&state.pool, &filter).await?;
    let v = serde_json::to_value(sessions)?;
    Ok((StatusCode::OK, Json(v)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sessions_query_empty() {
        let json = json!({});
        let q: SessionsQuery = serde_json::from_value(json).unwrap();
        assert!(q.project.is_none());
        assert!(q.status.is_none());
        assert!(q.limit.is_none());
        assert!(q.offset.is_none());
    }

    #[test]
    fn sessions_query_full() {
        let json = json!({
            "project": "alaz",
            "status": "active",
            "limit": 10,
            "offset": 20
        });
        let q: SessionsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.project.as_deref(), Some("alaz"));
        assert_eq!(q.status.as_deref(), Some("active"));
        assert_eq!(q.limit, Some(10));
        assert_eq!(q.offset, Some(20));
    }

    #[test]
    fn sessions_query_partial() {
        let json = json!({ "limit": 5 });
        let q: SessionsQuery = serde_json::from_value(json).unwrap();
        assert!(q.project.is_none());
        assert_eq!(q.limit, Some(5));
    }
}
