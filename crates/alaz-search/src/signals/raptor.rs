//! RAPTOR hierarchical search signal.
//!
//! Gets the RAPTOR tree for a project, retrieves all nodes, and searches by
//! embedding similarity. For level-0 nodes, returns the mapped entity directly.
//! For higher-level nodes, expands to leaf descendants via parent_id traversal.

use std::collections::HashMap;

use alaz_core::Result;
use alaz_core::models::RaptorNode;
use alaz_core::traits::SignalResult;
use alaz_db::repos::RaptorRepo;
use alaz_vector::{DenseVectorOps, QdrantManager};
use sqlx::PgPool;
use tracing::debug;

/// Execute RAPTOR search signal.
pub async fn execute(
    pool: &PgPool,
    qdrant: &QdrantManager,
    text_embedding: &[f32],
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<SignalResult>> {
    // Get the RAPTOR tree for this project (or global if no project)
    let tree = match RaptorRepo::get_tree(pool, project).await? {
        Some(t) if t.status == "ready" => t,
        _ => {
            debug!(project = ?project, "RAPTOR tree not available, skipping signal");
            return Ok(vec![]);
        }
    };

    // Get all nodes in the tree
    let nodes = RaptorRepo::get_collapsed_tree(pool, &tree.id).await?;

    if nodes.is_empty() {
        return Ok(vec![]);
    }

    // For RAPTOR search, we use the text embedding to search the vector store.
    // RAPTOR node summaries are embedded in the text collection with entity_type "raptor_node".
    // We search the text collection for raptor_node entities to find the most relevant nodes.
    let raptor_results = DenseVectorOps::search_text(
        qdrant.client(),
        text_embedding.to_vec(),
        project,
        (limit * 3) as u64,
    )
    .await
    .unwrap_or_default();

    // Filter to only raptor_node results
    let raptor_node_results: Vec<_> = raptor_results
        .into_iter()
        .filter(|(entity_type, _id, _score)| entity_type == "raptor_node")
        .collect();

    if raptor_node_results.is_empty() {
        // Fallback: if no raptor_node vectors found, try direct text search
        // against all entities in the tree
        let leaf_entities: Vec<SignalResult> = nodes
            .iter()
            .filter(|n| n.level == 0)
            .take(limit)
            .enumerate()
            .map(|(rank, node)| SignalResult {
                entity_type: node.entity_type.clone(),
                entity_id: node.entity_id.clone(),
                rank,
            })
            .collect();

        debug!(
            count = leaf_entities.len(),
            "RAPTOR signal fallback to leaf nodes"
        );
        return Ok(leaf_entities);
    }

    // Build a map from node_id -> node for quick lookup
    let node_map: HashMap<&str, &RaptorNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // For each matched raptor node, resolve to leaf entities
    let mut results: Vec<SignalResult> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (_, node_id, _score) in &raptor_node_results {
        if let Some(node) = node_map.get(node_id.as_str()) {
            if node.level == 0 {
                // Level-0 node: return the mapped entity directly
                let key = (node.entity_type.clone(), node.entity_id.clone());
                if seen.insert(key) {
                    results.push(SignalResult {
                        entity_type: node.entity_type.clone(),
                        entity_id: node.entity_id.clone(),
                        rank: results.len(),
                    });
                }
            } else {
                // Higher-level node: expand to leaf descendants via parent_id traversal
                let leaves = find_leaf_descendants(node_id, &nodes);
                for leaf in leaves {
                    let key = (leaf.entity_type.clone(), leaf.entity_id.clone());
                    if seen.insert(key) {
                        results.push(SignalResult {
                            entity_type: leaf.entity_type.clone(),
                            entity_id: leaf.entity_id.clone(),
                            rank: results.len(),
                        });
                    }
                }
            }
        }

        if results.len() >= limit {
            break;
        }
    }

    results.truncate(limit);

    debug!(count = results.len(), "RAPTOR signal complete");

    Ok(results)
}

/// Find all leaf (level-0) descendants of a given node by traversing parent_id links.
fn find_leaf_descendants<'a>(
    parent_node_id: &str,
    all_nodes: &'a [RaptorNode],
) -> Vec<&'a RaptorNode> {
    // Collect direct children
    let children: Vec<&RaptorNode> = all_nodes
        .iter()
        .filter(|n| n.parent_id.as_deref() == Some(parent_node_id))
        .collect();

    let mut leaves = Vec::new();

    for child in children {
        if child.level == 0 {
            leaves.push(child);
        } else {
            // Recurse into non-leaf children
            leaves.extend(find_leaf_descendants(&child.id, all_nodes));
        }
    }

    leaves
}
