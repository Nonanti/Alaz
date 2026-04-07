use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;

use alaz_intel::ContextInjector;

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/context", get(get_context))
        .with_state(state)
}

#[derive(Deserialize)]
struct ContextQuery {
    /// Project path or name for context injection.
    path: String,
}

/// Build priority-based context for a given project path.
/// Used by the session start hook to inject context into Claude.
async fn get_context(
    State(state): State<AppState>,
    Query(q): Query<ContextQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let injector = ContextInjector::new(state.pool.clone());
    let context = injector.build_context(&q.path).await?;
    let response = serde_json::json!({ "context": context });
    Ok((StatusCode::OK, Json(response)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn context_query_valid() {
        let json = json!({ "path": "/home/user/project" });
        let q: ContextQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.path, "/home/user/project");
    }

    #[test]
    fn context_query_missing_path_fails() {
        let json = json!({});
        let result = serde_json::from_value::<ContextQuery>(json);
        assert!(result.is_err(), "ContextQuery should require 'path' field");
    }
}
