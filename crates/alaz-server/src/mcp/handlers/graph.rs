use alaz_core::models::*;
use alaz_db::repos::*;
use alaz_graph::explore;

use super::super::helpers::*;
use super::super::params::*;
use crate::state::AppState;

pub(crate) async fn relate(state: &AppState, params: RelateParams) -> Result<String, String> {
    let source_type = detect_entity_type(&state.pool, &params.source_id).await;
    let target_type = detect_entity_type(&state.pool, &params.target_id).await;
    let input = CreateRelation {
        source_type,
        source_id: params.source_id,
        target_type,
        target_id: params.target_id,
        relation: params.relation,
        weight: Some(1.0),
        description: params.description,
        metadata: params.metadata,
    };
    let edge = GraphRepo::create_edge(&state.pool, &input)
        .await
        .map_err(|e| format!("relate failed: {e}"))?;
    serde_json::to_string_pretty(&edge).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn unrelate(state: &AppState, params: UnrelateParams) -> Result<String, String> {
    GraphRepo::delete_edge(&state.pool, &params.id)
        .await
        .map_err(|e| format!("unrelate failed: {e}"))?;
    Ok(format!("deleted edge {}", params.id))
}

pub(crate) async fn graph_explore(
    state: &AppState,
    params: GraphExploreInput,
) -> Result<String, String> {
    let results = explore(
        &state.pool,
        &params.entity_type,
        &params.entity_id,
        params.depth.unwrap_or(1),
        params.min_weight,
        params.relation_filter.as_deref(),
    )
    .await
    .map_err(|e| format!("explore failed: {e}"))?;
    serde_json::to_string_pretty(&results).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn relations(state: &AppState, params: RelationsParams) -> Result<String, String> {
    let entity_type = detect_entity_type(&state.pool, &params.item_id).await;
    let direction = params.direction.as_deref().unwrap_or("both");
    let depth = params.depth.unwrap_or(1);
    if depth > 1 {
        let results = explore(
            &state.pool,
            &entity_type,
            &params.item_id,
            depth,
            None,
            params.relation_type.as_deref(),
        )
        .await
        .map_err(|e| format!("relations explore failed: {e}"))?;
        serde_json::to_string_pretty(&results).map_err(|e| format!("json error: {e}"))
    } else {
        let edges = GraphRepo::get_edges(&state.pool, &entity_type, &params.item_id, direction)
            .await
            .map_err(|e| format!("relations failed: {e}"))?;
        let edges: Vec<_> = if let Some(ref rt) = params.relation_type {
            edges.into_iter().filter(|e| &e.relation == rt).collect()
        } else {
            edges
        };
        serde_json::to_string_pretty(&edges).map_err(|e| format!("json error: {e}"))
    }
}
