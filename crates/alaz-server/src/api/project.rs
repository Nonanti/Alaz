use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};

use alaz_db::repos::ProjectRepo;

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/projects", get(list_projects))
        .with_state(state)
}

async fn list_projects(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let projects = ProjectRepo::list(&state.pool).await?;
    let v = serde_json::to_value(projects)?;
    Ok((StatusCode::OK, Json(v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_compiles() {
        // Compile-time check: `router` function exists and accepts AppState.
        // Cannot call it without a real AppState, but the signature is verified.
        let _: fn(AppState) -> Router = router;
    }
}
