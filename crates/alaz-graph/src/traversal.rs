use std::collections::{HashSet, VecDeque};

use alaz_core::Result;
use alaz_core::models::ScoredEntity;
use alaz_db::repos::GraphRepo;
use sqlx::PgPool;
use tracing::debug;

/// Performs a BFS multi-hop scored traversal starting from the given entity.
///
/// The accumulated score along a path is the product of edge weights.
/// Nodes are visited at most once. Results are sorted by score descending.
pub async fn explore(
    pool: &PgPool,
    entity_type: &str,
    entity_id: &str,
    max_depth: u32,
    min_weight: Option<f64>,
    relation_filter: Option<&str>,
) -> Result<Vec<ScoredEntity>> {
    let min_w = min_weight.unwrap_or(0.0);
    let max_depth = max_depth.min(10); // Safety cap

    // (entity_type, entity_id, accumulated_score, depth)
    let mut queue: VecDeque<(String, String, f64, u32)> = VecDeque::new();
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut results: Vec<ScoredEntity> = Vec::new();

    // Seed the BFS from the starting node
    queue.push_back((entity_type.to_string(), entity_id.to_string(), 1.0, 0));
    visited.insert((entity_type.to_string(), entity_id.to_string()));

    while let Some((current_type, current_id, score, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        // Get outgoing edges from the current node
        let edges = GraphRepo::get_edges(pool, &current_type, &current_id, "outgoing").await?;

        for edge in edges {
            // Apply weight threshold
            if edge.weight < min_w {
                continue;
            }

            // Apply relation filter
            if let Some(filter) = relation_filter
                && edge.relation != filter
            {
                continue;
            }

            let target_key = (edge.target_type.clone(), edge.target_id.clone());
            if visited.contains(&target_key) {
                continue;
            }
            visited.insert(target_key);

            let accumulated = score * edge.weight;
            let next_depth = depth + 1;

            results.push(ScoredEntity {
                entity_type: edge.target_type.clone(),
                entity_id: edge.target_id.clone(),
                title: String::new(), // Title can be resolved by the caller
                score: accumulated,
                relation: edge.relation.clone(),
                depth: next_depth,
            });

            // Continue traversal from this node
            queue.push_back((edge.target_type, edge.target_id, accumulated, next_depth));
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    debug!(
        start_type = entity_type,
        start_id = entity_id,
        found = results.len(),
        "BFS traversal complete"
    );

    Ok(results)
}
