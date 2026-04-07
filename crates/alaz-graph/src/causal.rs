use std::collections::HashSet;

use alaz_core::Result;
use alaz_core::models::ScoredEntity;
use alaz_db::repos::GraphRepo;
use sqlx::PgPool;
use tracing::debug;

/// The set of relation types considered "causal" for chain traversal.
const CAUSAL_RELATIONS: &[&str] = &[
    "led_to",
    "caused",
    "triggered",
    "caused_by",
    "resolved_by",
    "preceded_by",
];

/// Maximum depth for causal chain traversal.
const MAX_CAUSAL_DEPTH: u32 = 5;

/// Follow a causal chain from the given entity.
///
/// Only follows edges with causal relation types (`led_to`, `caused`, `triggered`).
/// At branch points, follows the highest-weight path. Returns a linear chain.
pub async fn follow_causal_chain(
    pool: &PgPool,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<ScoredEntity>> {
    let mut chain: Vec<ScoredEntity> = Vec::new();
    let mut visited: HashSet<(String, String)> = HashSet::new();

    let mut current_type = entity_type.to_string();
    let mut current_id = entity_id.to_string();
    let mut accumulated_score = 1.0_f64;

    visited.insert((current_type.clone(), current_id.clone()));

    for depth in 1..=MAX_CAUSAL_DEPTH {
        let edges = GraphRepo::get_edges(pool, &current_type, &current_id, "outgoing").await?;

        // Filter to causal edges only and pick the highest-weight one
        let best_edge = edges
            .into_iter()
            .filter(|e| CAUSAL_RELATIONS.contains(&e.relation.as_str()))
            .filter(|e| !visited.contains(&(e.target_type.clone(), e.target_id.clone())))
            .max_by(|a, b| {
                a.weight
                    .partial_cmp(&b.weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        let Some(edge) = best_edge else {
            break; // No more causal edges to follow
        };

        accumulated_score *= edge.weight;
        visited.insert((edge.target_type.clone(), edge.target_id.clone()));

        chain.push(ScoredEntity {
            entity_type: edge.target_type.clone(),
            entity_id: edge.target_id.clone(),
            title: String::new(),
            score: accumulated_score,
            relation: edge.relation.clone(),
            depth,
        });

        current_type = edge.target_type;
        current_id = edge.target_id;
    }

    debug!(
        start_type = entity_type,
        start_id = entity_id,
        chain_length = chain.len(),
        "causal chain traversal complete"
    );

    Ok(chain)
}

/// Follow a causal chain backwards from the given entity.
///
/// Follows incoming causal edges to find what caused this entity.
/// At branch points, follows the highest-weight path. Returns a linear chain.
pub async fn follow_causal_chain_reverse(
    pool: &PgPool,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<ScoredEntity>> {
    let mut chain: Vec<ScoredEntity> = Vec::new();
    let mut visited: HashSet<(String, String)> = HashSet::new();

    let mut current_type = entity_type.to_string();
    let mut current_id = entity_id.to_string();
    let mut accumulated_score = 1.0_f64;

    visited.insert((current_type.clone(), current_id.clone()));

    for depth in 1..=MAX_CAUSAL_DEPTH {
        let edges = GraphRepo::get_edges(pool, &current_type, &current_id, "incoming").await?;

        // Filter to causal edges only and pick the highest-weight one
        let best_edge = edges
            .into_iter()
            .filter(|e| CAUSAL_RELATIONS.contains(&e.relation.as_str()))
            .filter(|e| !visited.contains(&(e.source_type.clone(), e.source_id.clone())))
            .max_by(|a, b| {
                a.weight
                    .partial_cmp(&b.weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        let Some(edge) = best_edge else {
            break;
        };

        accumulated_score *= edge.weight;
        visited.insert((edge.source_type.clone(), edge.source_id.clone()));

        chain.push(ScoredEntity {
            entity_type: edge.source_type.clone(),
            entity_id: edge.source_id.clone(),
            title: String::new(),
            score: accumulated_score,
            relation: edge.relation.clone(),
            depth,
        });

        current_type = edge.source_type;
        current_id = edge.source_id;
    }

    debug!(
        start_type = entity_type,
        start_id = entity_id,
        chain_length = chain.len(),
        "reverse causal chain traversal complete"
    );

    Ok(chain)
}
