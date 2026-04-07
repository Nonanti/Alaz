use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;

use alaz_core::traits::SearchQuery;
use alaz_db::repos::{KnowledgeRepo, ProjectRepo, SearchQueryRepo};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/search", get(fts_search))
        .route("/search/hybrid", get(hybrid_search))
        .route("/search/feedback", post(search_feedback))
        .with_state(state)
}

#[derive(Deserialize)]
struct FtsQuery {
    query: String,
    project: Option<String>,
    limit: Option<i64>,
}

async fn fts_search(
    State(state): State<AppState>,
    Query(q): Query<FtsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let start = std::time::Instant::now();

    let project_id = if let Some(ref name) = q.project {
        ProjectRepo::get_by_name(&state.pool, name)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    let limit = q.limit.unwrap_or(10);
    let results =
        KnowledgeRepo::fts_search(&state.pool, &q.query, project_id.as_deref(), limit).await?;

    state
        .metrics
        .record_search(start.elapsed().as_millis() as u64);

    let items: Vec<_> = results
        .into_iter()
        .map(|(id, title, rank)| {
            serde_json::json!({
                "id": id,
                "title": title,
                "rank": rank,
            })
        })
        .collect();

    let v = serde_json::to_value(items)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(Deserialize)]
struct HybridQuery {
    query: String,
    project: Option<String>,
    limit: Option<usize>,
    rerank: Option<bool>,
    hyde: Option<bool>,
    graph_expand: Option<bool>,
}

async fn hybrid_search(
    State(state): State<AppState>,
    Query(q): Query<HybridQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let search_query = SearchQuery {
        query: q.query,
        project: q.project,
        limit: q.limit,
        rerank: q.rerank,
        hyde: q.hyde,
        graph_expand: q.graph_expand,
    };

    let start = std::time::Instant::now();
    let results = state.search.hybrid_search(&search_query).await?;
    state
        .metrics
        .record_search(start.elapsed().as_millis() as u64);

    let v = serde_json::to_value(results)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(Deserialize)]
struct FeedbackBody {
    entity_id: String,
}

async fn search_feedback(
    State(state): State<AppState>,
    Json(body): Json<FeedbackBody>,
) -> Result<impl IntoResponse, ApiError> {
    SearchQueryRepo::record_click(&state.pool, &body.entity_id).await?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_minimal() {
        let json = serde_json::json!({ "query": "rust async" });
        let q: FtsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.query, "rust async");
        assert!(q.project.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn fts_query_all_fields() {
        let json = serde_json::json!({
            "query": "memory leak",
            "project": "alaz",
            "limit": 25
        });
        let q: FtsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.query, "memory leak");
        assert_eq!(q.project.as_deref(), Some("alaz"));
        assert_eq!(q.limit, Some(25));
    }

    #[test]
    fn fts_query_missing_required_field() {
        let json = serde_json::json!({ "project": "alaz" });
        let result = serde_json::from_value::<FtsQuery>(json);
        assert!(
            result.is_err(),
            "should fail without required `query` field"
        );
    }

    #[test]
    fn hybrid_query_minimal() {
        let json = serde_json::json!({ "query": "error handling" });
        let q: HybridQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.query, "error handling");
        assert!(q.project.is_none());
        assert!(q.limit.is_none());
        assert!(q.rerank.is_none());
        assert!(q.hyde.is_none());
        assert!(q.graph_expand.is_none());
    }

    #[test]
    fn hybrid_query_full() {
        let json = serde_json::json!({
            "query": "cache invalidation",
            "project": "alaz",
            "limit": 50,
            "rerank": true,
            "hyde": false,
            "graph_expand": true
        });
        let q: HybridQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.query, "cache invalidation");
        assert_eq!(q.project.as_deref(), Some("alaz"));
        assert_eq!(q.limit, Some(50));
        assert_eq!(q.rerank, Some(true));
        assert_eq!(q.hyde, Some(false));
        assert_eq!(q.graph_expand, Some(true));
    }

    #[test]
    fn hybrid_query_maps_to_search_query() {
        let json = serde_json::json!({
            "query": "vector search",
            "project": "test-proj",
            "limit": 15,
            "rerank": true,
            "hyde": true,
            "graph_expand": false
        });
        let q: HybridQuery = serde_json::from_value(json).unwrap();

        let sq = SearchQuery {
            query: q.query,
            project: q.project,
            limit: q.limit,
            rerank: q.rerank,
            hyde: q.hyde,
            graph_expand: q.graph_expand,
        };

        assert_eq!(sq.query, "vector search");
        assert_eq!(sq.project.as_deref(), Some("test-proj"));
        assert_eq!(sq.limit, Some(15));
        assert_eq!(sq.rerank, Some(true));
        assert_eq!(sq.hyde, Some(true));
        assert_eq!(sq.graph_expand, Some(false));
    }

    #[test]
    fn feedback_body_valid() {
        let json = serde_json::json!({ "entity_id": "abc123" });
        let body: FeedbackBody = serde_json::from_value(json).unwrap();
        assert_eq!(body.entity_id, "abc123");
    }
}
