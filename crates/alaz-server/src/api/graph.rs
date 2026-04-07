use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;

use alaz_core::models::CreateRelation;
use alaz_db::repos::GraphRepo;
use alaz_graph::explore;

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/relations", post(create_relation))
        .route("/relations/{id}", delete(delete_relation))
        .route("/relations/{id}", get(get_relations))
        .route("/graph/explore", get(graph_explore))
        .with_state(state)
}

#[derive(Deserialize)]
struct CreateRelationBody {
    source_type: String,
    source_id: String,
    target_type: String,
    target_id: String,
    relation: String,
    weight: Option<f64>,
    description: Option<String>,
    metadata: Option<serde_json::Value>,
}

async fn create_relation(
    State(state): State<AppState>,
    Json(body): Json<CreateRelationBody>,
) -> Result<impl IntoResponse, ApiError> {
    let input = CreateRelation {
        source_type: body.source_type,
        source_id: body.source_id,
        target_type: body.target_type,
        target_id: body.target_id,
        relation: body.relation,
        weight: body.weight,
        description: body.description,
        metadata: body.metadata,
    };

    let edge = GraphRepo::create_edge(&state.pool, &input).await?;
    let v = serde_json::to_value(edge)?;
    Ok((StatusCode::CREATED, Json(v)))
}

async fn delete_relation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    GraphRepo::delete_edge(&state.pool, &id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
struct RelationsQuery {
    entity_type: Option<String>,
    direction: Option<String>,
}

async fn get_relations(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<RelationsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let entity_type = q.entity_type.as_deref().unwrap_or("knowledge_item");
    let direction = q.direction.as_deref().unwrap_or("both");

    let edges = GraphRepo::get_edges(&state.pool, entity_type, &id, direction).await?;
    let v = serde_json::to_value(edges)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(Deserialize)]
struct ExploreQuery {
    entity_type: String,
    entity_id: String,
    depth: Option<u32>,
    min_weight: Option<f64>,
    relation_filter: Option<String>,
}

async fn graph_explore(
    State(state): State<AppState>,
    Query(q): Query<ExploreQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let results = explore(
        &state.pool,
        &q.entity_type,
        &q.entity_id,
        q.depth.unwrap_or(1),
        q.min_weight,
        q.relation_filter.as_deref(),
    )
    .await?;
    let v = serde_json::to_value(results)?;
    Ok((StatusCode::OK, Json(v)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_relation_body_minimal() {
        let json = json!({
            "source_type": "knowledge_item",
            "source_id": "kid_001",
            "target_type": "episode",
            "target_id": "ep_001",
            "relation": "caused_by"
        });
        let body: CreateRelationBody = serde_json::from_value(json).unwrap();
        assert_eq!(body.source_type, "knowledge_item");
        assert_eq!(body.relation, "caused_by");
        assert!(body.weight.is_none());
        assert!(body.description.is_none());
        assert!(body.metadata.is_none());
    }

    #[test]
    fn create_relation_body_full() {
        let json = json!({
            "source_type": "knowledge_item",
            "source_id": "kid_001",
            "target_type": "procedure",
            "target_id": "proc_001",
            "relation": "implements",
            "weight": 0.95,
            "description": "Pattern implements procedure",
            "metadata": { "verified": true }
        });
        let body: CreateRelationBody = serde_json::from_value(json).unwrap();
        assert!((body.weight.unwrap() - 0.95).abs() < f64::EPSILON);
        assert_eq!(
            body.description.as_deref(),
            Some("Pattern implements procedure")
        );
        assert!(body.metadata.is_some());
    }

    #[test]
    fn create_relation_body_missing_required_fails() {
        let json = json!({
            "source_type": "knowledge_item",
            "source_id": "kid_001"
        });
        let result = serde_json::from_value::<CreateRelationBody>(json);
        assert!(result.is_err());
    }

    #[test]
    fn relations_query_empty() {
        let json = json!({});
        let q: RelationsQuery = serde_json::from_value(json).unwrap();
        assert!(q.entity_type.is_none());
        assert!(q.direction.is_none());
    }

    #[test]
    fn explore_query_minimal() {
        let json = json!({
            "entity_type": "knowledge_item",
            "entity_id": "kid_001"
        });
        let q: ExploreQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.entity_type, "knowledge_item");
        assert_eq!(q.entity_id, "kid_001");
        assert!(q.depth.is_none());
        assert!(q.min_weight.is_none());
        assert!(q.relation_filter.is_none());
    }

    #[test]
    fn explore_query_full() {
        let json = json!({
            "entity_type": "episode",
            "entity_id": "ep_100",
            "depth": 3,
            "min_weight": 0.5,
            "relation_filter": "caused_by"
        });
        let q: ExploreQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.depth, Some(3));
        assert!((q.min_weight.unwrap() - 0.5).abs() < f64::EPSILON);
        assert_eq!(q.relation_filter.as_deref(), Some("caused_by"));
    }
}
