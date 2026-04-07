//! Label propagation clustering for the knowledge graph.
//!
//! Groups knowledge items into clusters based on their graph edge relationships
//! using a weighted label propagation algorithm.

use std::collections::{HashMap, HashSet};

use alaz_core::Result;
use sqlx::PgPool;

/// Run label propagation on an arbitrary weighted edge list.
///
/// Each node starts with its own unique label. On each iteration, every node
/// adopts the label with the highest cumulative weight among its neighbours.
/// Iteration stops when labels stabilise or after 20 rounds.
///
/// Deterministic: nodes are processed in sorted order and ties are broken by
/// the smallest label value.
///
/// Returns a map from node ID to cluster label (`u32`).
pub fn label_propagation(edges: &[(String, String, f32)]) -> HashMap<String, u32> {
    if edges.is_empty() {
        return HashMap::new();
    }

    // Build undirected adjacency list
    let mut adjacency: HashMap<String, Vec<(String, f32)>> = HashMap::new();
    let mut all_nodes: HashSet<String> = HashSet::new();

    for (source, target, weight) in edges {
        adjacency
            .entry(source.clone())
            .or_default()
            .push((target.clone(), *weight));
        adjacency
            .entry(target.clone())
            .or_default()
            .push((source.clone(), *weight));
        all_nodes.insert(source.clone());
        all_nodes.insert(target.clone());
    }

    // Sorted for deterministic iteration
    let mut sorted_nodes: Vec<String> = all_nodes.into_iter().collect();
    sorted_nodes.sort();

    // Each node starts with a unique label
    let mut labels: HashMap<String, u32> = HashMap::new();
    for (i, node) in sorted_nodes.iter().enumerate() {
        labels.insert(node.clone(), i as u32);
    }

    let max_iterations = 20;
    for _ in 0..max_iterations {
        let mut changed = false;

        for node in &sorted_nodes {
            if let Some(neighbors) = adjacency.get(node) {
                // Accumulate weighted votes per label
                let mut label_scores: HashMap<u32, f32> = HashMap::new();
                for (neighbor, weight) in neighbors {
                    if let Some(&neighbor_label) = labels.get(neighbor) {
                        *label_scores.entry(neighbor_label).or_default() += weight;
                    }
                }

                // Pick the label with the highest score.
                // On ties, pick the smallest label for determinism.
                if let Some((&best_label, _)) = label_scores.iter().max_by(|a, b| {
                    a.1.partial_cmp(b.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| b.0.cmp(a.0)) // smaller label wins on tie
                }) {
                    let current = labels.get(node).copied().unwrap_or(0);
                    if best_label != current {
                        labels.insert(node.clone(), best_label);
                        changed = true;
                    }
                }
            }
        }

        if !changed {
            break;
        }
    }

    labels
}

/// Load edges from the `graph_edges` table and cluster them via label propagation.
///
/// Optionally filters by `project_id` (matched against source/target metadata in
/// `knowledge_items`) and a minimum edge weight.
///
/// Returns a map from entity ID to cluster label.
pub async fn cluster_knowledge(
    pool: &PgPool,
    project_id: Option<&str>,
    min_weight: f32,
) -> Result<HashMap<String, u32>> {
    let edges: Vec<(String, String, f32)> = match project_id {
        Some(pid) => {
            sqlx::query_as::<_, (String, String, f64)>(
                r#"
                SELECT ge.source_id, ge.target_id, ge.weight
                FROM graph_edges ge
                JOIN knowledge_items ki ON ki.id = ge.source_id
                WHERE ge.weight >= $1
                  AND ki.project_id = $2
                "#,
            )
            .bind(min_weight as f64)
            .bind(pid)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, (String, String, f64)>(
                r#"
                SELECT source_id, target_id, weight
                FROM graph_edges
                WHERE weight >= $1
                "#,
            )
            .bind(min_weight as f64)
            .fetch_all(pool)
            .await?
        }
    }
    .into_iter()
    .map(|(s, t, w)| (s, t, w as f32))
    .collect();

    Ok(label_propagation(&edges))
}

/// Group clustered nodes into a `Vec` of clusters, each containing its member IDs.
///
/// Clusters are sorted largest-first; members within each cluster are sorted
/// alphabetically.
pub fn group_clusters(labels: &HashMap<String, u32>) -> Vec<Vec<String>> {
    let mut clusters_map: HashMap<u32, Vec<String>> = HashMap::new();
    for (node, label) in labels {
        clusters_map.entry(*label).or_default().push(node.clone());
    }

    let mut groups: Vec<Vec<String>> = clusters_map.into_values().collect();

    // Sort members within each cluster for determinism
    for group in &mut groups {
        group.sort();
    }

    // Largest clusters first, break ties by first member
    groups.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_edges() {
        let labels = label_propagation(&[]);
        assert!(labels.is_empty());
    }

    #[test]
    fn single_edge() {
        let edges = vec![("a".into(), "b".into(), 1.0)];
        let labels = label_propagation(&edges);
        assert_eq!(labels.len(), 2);
        // Both nodes should converge to the same cluster
        assert_eq!(labels["a"], labels["b"]);
    }

    #[test]
    fn triangle_forms_single_cluster() {
        let edges = vec![
            ("a".into(), "b".into(), 1.0),
            ("b".into(), "c".into(), 1.0),
            ("a".into(), "c".into(), 1.0),
        ];
        let labels = label_propagation(&edges);
        assert_eq!(labels.len(), 3);
        assert_eq!(labels["a"], labels["b"]);
        assert_eq!(labels["b"], labels["c"]);
    }

    #[test]
    fn disconnected_components() {
        let edges = vec![("a".into(), "b".into(), 1.0), ("c".into(), "d".into(), 1.0)];
        let labels = label_propagation(&edges);
        assert_eq!(labels.len(), 4);
        // {a,b} and {c,d} should be in different clusters
        assert_eq!(labels["a"], labels["b"]);
        assert_eq!(labels["c"], labels["d"]);
        assert_ne!(labels["a"], labels["c"]);
    }

    #[test]
    fn convergence_within_max_iterations() {
        // Chain: a-b-c-d-e — should converge quickly
        let edges = vec![
            ("a".into(), "b".into(), 1.0),
            ("b".into(), "c".into(), 1.0),
            ("c".into(), "d".into(), 1.0),
            ("d".into(), "e".into(), 1.0),
        ];
        let labels = label_propagation(&edges);
        // All connected → single cluster
        let cluster = labels["a"];
        assert!(labels.values().all(|&l| l == cluster));
    }

    #[test]
    fn determinism_same_input_same_output() {
        let edges = vec![
            ("x".into(), "y".into(), 0.8),
            ("y".into(), "z".into(), 0.9),
            ("z".into(), "x".into(), 0.7),
        ];
        let labels1 = label_propagation(&edges);
        let labels2 = label_propagation(&edges);
        assert_eq!(labels1, labels2, "same input must produce identical labels");
    }

    #[test]
    fn weighted_edges_preference() {
        // Node b is connected to both {a} and {c,d} clusters.
        // The stronger weight should pull b towards {c,d}.
        let edges = vec![
            ("a".into(), "b".into(), 0.1), // weak link to a
            ("b".into(), "c".into(), 0.9), // strong link to c
            ("c".into(), "d".into(), 1.0), // c-d cluster
        ];
        let labels = label_propagation(&edges);
        // b should be in the same cluster as c and d
        assert_eq!(labels["b"], labels["c"]);
        assert_eq!(labels["c"], labels["d"]);
    }

    #[test]
    fn large_graph_star_topology() {
        // Hub connected to 50 spokes
        let edges: Vec<(String, String, f32)> = (0..50)
            .map(|i| ("hub".into(), format!("spoke_{i}"), 1.0))
            .collect();
        let labels = label_propagation(&edges);
        assert_eq!(labels.len(), 51); // hub + 50 spokes
        // All in the same cluster
        let hub_label = labels["hub"];
        assert!(
            labels.values().all(|&l| l == hub_label),
            "star topology should form a single cluster"
        );
    }

    #[test]
    fn group_clusters_basic() {
        let mut labels = HashMap::new();
        labels.insert("a".into(), 0);
        labels.insert("b".into(), 0);
        labels.insert("c".into(), 1);
        labels.insert("d".into(), 1);
        labels.insert("e".into(), 1);

        let groups = group_clusters(&labels);
        assert_eq!(groups.len(), 2);
        // Largest cluster first
        assert_eq!(groups[0].len(), 3);
        assert_eq!(groups[1].len(), 2);
    }

    #[test]
    fn group_clusters_empty() {
        let labels: HashMap<String, u32> = HashMap::new();
        let groups = group_clusters(&labels);
        assert!(groups.is_empty());
    }
}
