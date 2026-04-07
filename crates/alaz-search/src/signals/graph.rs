//! Graph expansion signal.
//!
//! Takes top-10 candidates from other signals as seeds and performs 1-hop BFS
//! expansion from each seed via `alaz_graph::traversal::explore`. Collects
//! unique discovered entities not already in the seed set.

use std::collections::HashSet;

use alaz_core::Result;
use alaz_core::traits::SignalResult;
use sqlx::PgPool;
use tracing::debug;

/// Execute graph expansion signal.
///
/// `seed_results` are `(entity_type, entity_id)` pairs from prior signals.
pub async fn execute(
    pool: &PgPool,
    seed_results: &[(String, String)],
    limit: usize,
) -> Result<Vec<SignalResult>> {
    if seed_results.is_empty() {
        return Ok(vec![]);
    }

    // Track already-seen entities (seeds should not appear in expansion results)
    let seed_set: HashSet<(&str, &str)> = seed_results
        .iter()
        .map(|(t, i)| (t.as_str(), i.as_str()))
        .collect();

    let mut discovered: Vec<(String, String, f64)> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    // For each seed, do 1-hop BFS
    for (entity_type, entity_id) in seed_results.iter().take(10) {
        let neighbors = alaz_graph::explore(
            pool,
            entity_type,
            entity_id,
            1, // 1-hop
            None,
            None,
        )
        .await
        .unwrap_or_default();

        for neighbor in neighbors {
            let key = (neighbor.entity_type.clone(), neighbor.entity_id.clone());

            // Skip if already in seeds or already discovered
            if seed_set.contains(&(key.0.as_str(), key.1.as_str())) {
                continue;
            }
            if seen.contains(&key) {
                continue;
            }

            seen.insert(key);
            discovered.push((neighbor.entity_type, neighbor.entity_id, neighbor.score));
        }
    }

    // Sort by accumulated graph score descending
    discovered.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    discovered.truncate(limit);

    let results: Vec<SignalResult> = discovered
        .into_iter()
        .enumerate()
        .map(|(rank, (entity_type, entity_id, _score))| SignalResult {
            entity_type,
            entity_id,
            rank,
        })
        .collect();

    debug!(
        seeds = seed_results.len(),
        discovered = results.len(),
        "graph expansion signal complete"
    );

    Ok(results)
}
