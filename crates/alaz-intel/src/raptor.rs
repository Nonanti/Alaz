use std::sync::Arc;

use alaz_core::Result;
use alaz_core::models::{
    ListEpisodesFilter, ListKnowledgeFilter, ListProceduresFilter, RaptorTree,
};
use alaz_db::repos::{EpisodeRepo, KnowledgeRepo, ProcedureRepo, RaptorRepo};
use alaz_vector::{COLLECTION_TEXT, DenseVectorOps, QdrantManager};
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::embeddings::EmbeddingService;
use crate::llm::LlmClient;

/// Builds RAPTOR hierarchical clustering trees for conceptual search.
///
/// RAPTOR (Recursive Abstractive Processing for Tree-Organized Retrieval)
/// groups similar items into clusters, generates LLM summaries, and builds
/// a tree of embeddings for multi-level search.
pub struct RaptorBuilder {
    pool: PgPool,
    llm: Arc<LlmClient>,
    embedding: Arc<EmbeddingService>,
    qdrant: Arc<QdrantManager>,
}

/// An item with its text content for clustering.
struct ClusterItem {
    entity_type: String,
    entity_id: String,
    text: String,
}

/// Intermediate result from the tree preparation phase.
enum PrepareResult {
    /// Too few items — tree was created as flat (leaf-only) and is ready.
    FlatTree(RaptorTree),
    /// Enough items for clustering — proceed to embedding + clustering.
    Ready {
        tree: RaptorTree,
        items: Vec<ClusterItem>,
    },
}

const SUMMARY_SYSTEM_PROMPT: &str = r#"You are a knowledge summarizer. Given a cluster of related knowledge items, write a concise summary that captures the key themes and information.

Keep the summary under 300 words. Focus on the shared themes and important details."#;

/// Minimum number of items required to perform clustering.
const MIN_ITEMS_FOR_CLUSTERING: usize = 10;

impl RaptorBuilder {
    /// Create a new RAPTOR builder.
    pub fn new(
        pool: PgPool,
        llm: Arc<LlmClient>,
        embedding: Arc<EmbeddingService>,
        qdrant: Arc<QdrantManager>,
    ) -> Self {
        Self {
            pool,
            llm,
            embedding,
            qdrant,
        }
    }

    /// Rebuild the RAPTOR tree for a project.
    ///
    /// Orchestrates the full pipeline: fetch → clean → cluster → summarize → store.
    pub async fn rebuild_tree(&self, project_id: Option<&str>) -> Result<RaptorTree> {
        let project_label = project_id.unwrap_or("global");
        info!(project = %project_label, "starting RAPTOR tree rebuild");

        // Phases 1-3: Fetch items, create tree, clean old nodes, handle flat tree
        let (tree, items) = match self.fetch_and_prepare_tree(project_id).await? {
            PrepareResult::FlatTree(tree) => return Ok(tree),
            PrepareResult::Ready { tree, items } => (tree, items),
        };

        // Phase 4-5: Compute embeddings + K-Means++ with silhouette selection
        let (best_k, assignments) = match self.find_optimal_clusters(&items, &tree.id).await? {
            Some(result) => result,
            None => return RaptorRepo::get_tree_by_id(&self.pool, &tree.id).await,
        };

        // Phase 6: Create cluster nodes with LLM summaries
        let total_nodes = self
            .build_cluster_nodes(&tree.id, project_id, &items, best_k, &assignments)
            .await?;

        // Phase 7: Update tree stats
        RaptorRepo::update_tree_stats(&self.pool, &tree.id, total_nodes, 1, "ready").await?;

        info!(
            project = %project_label,
            clusters = best_k,
            total_nodes,
            "RAPTOR tree rebuild completed"
        );

        RaptorRepo::get_tree_by_id(&self.pool, &tree.id).await
    }

    /// Fetch items, create/clean tree record, and handle the flat-tree early return.
    ///
    /// Returns `PrepareResult::FlatTree` if there are too few items for clustering,
    /// or `PrepareResult::Ready` with the tree and items for the clustering pipeline.
    async fn fetch_and_prepare_tree(&self, project_id: Option<&str>) -> Result<PrepareResult> {
        let project_label = project_id.unwrap_or("global");

        // Fetch all items
        let items = self.fetch_all_items(project_id).await?;

        info!(
            project = %project_label,
            item_count = items.len(),
            "fetched items for RAPTOR"
        );

        // Create or update the tree record
        let tree = RaptorRepo::upsert_tree(&self.pool, project_id).await?;

        // Clear existing nodes and their vectors from Qdrant
        let old_nodes = RaptorRepo::get_collapsed_tree(&self.pool, &tree.id).await?;
        for node in &old_nodes {
            if let Err(e) = DenseVectorOps::delete_point(
                self.qdrant.client(),
                COLLECTION_TEXT,
                "raptor_node",
                &node.id,
            )
            .await
            {
                warn!(node_id = %node.id, error = %e, "failed to delete raptor node vector from Qdrant");
            }
        }
        RaptorRepo::delete_tree_nodes(&self.pool, &tree.id).await?;

        // Check minimum item count
        if items.len() < MIN_ITEMS_FOR_CLUSTERING {
            info!(
                project = %project_label,
                item_count = items.len(),
                "too few items for clustering, creating flat tree"
            );

            // Create leaf nodes only
            for item in &items {
                RaptorRepo::insert_node(
                    &self.pool,
                    &tree.id,
                    0,
                    None,
                    &item.entity_type,
                    &item.entity_id,
                    None,
                    0,
                )
                .await?;
            }

            RaptorRepo::update_tree_stats(&self.pool, &tree.id, items.len() as i64, 0, "ready")
                .await?;

            let tree = RaptorRepo::get_tree_by_id(&self.pool, &tree.id).await?;
            return Ok(PrepareResult::FlatTree(tree));
        }

        Ok(PrepareResult::Ready { tree, items })
    }

    /// Compute embeddings and find the optimal number of clusters via silhouette score.
    ///
    /// Returns `None` if embedding fails (tree is marked as error).
    /// Returns `Some((k, assignments, embeddings))` on success.
    async fn find_optimal_clusters(
        &self,
        items: &[ClusterItem],
        tree_id: &str,
    ) -> Result<Option<(usize, Vec<usize>)>> {
        const EMBEDDING_BATCH_SIZE: usize = 50;
        let texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(EMBEDDING_BATCH_SIZE) {
            let batch = self.embedding.embed_text(chunk).await?;
            embeddings.extend(batch);
        }

        if embeddings.len() != items.len() {
            warn!(
                expected = items.len(),
                got = embeddings.len(),
                "embedding count mismatch"
            );
            RaptorRepo::update_tree_stats(&self.pool, tree_id, 0, 0, "error").await?;
            return Ok(None);
        }

        let n = items.len();
        let max_k = (n as f64).sqrt().ceil() as usize;
        let min_k = 2;

        let (best_k, assignments) = if max_k <= min_k {
            (min_k, kmeans(&embeddings, min_k, 20))
        } else {
            let mut best_score = f64::NEG_INFINITY;
            let mut best_k = min_k;
            let mut best_assignments = kmeans(&embeddings, min_k, 20);

            for k in min_k..=max_k {
                let assignments = kmeans(&embeddings, k, 20);
                let score = silhouette_score(&embeddings, &assignments, k);

                debug!(k, silhouette = %score, "evaluated k");

                if score > best_score {
                    best_score = score;
                    best_k = k;
                    best_assignments = assignments;
                }
            }

            info!(
                best_k,
                best_silhouette = %best_score,
                "selected optimal k"
            );

            (best_k, best_assignments)
        };

        Ok(Some((best_k, assignments)))
    }

    /// Create cluster parent nodes with LLM summaries and their leaf children.
    /// Returns the total number of nodes created.
    async fn build_cluster_nodes(
        &self,
        tree_id: &str,
        project_id: Option<&str>,
        items: &[ClusterItem],
        k: usize,
        assignments: &[usize],
    ) -> Result<i64> {
        let mut total_nodes: i64 = 0;

        let mut clusters: Vec<Vec<usize>> = vec![Vec::new(); k];
        for (i, &cluster) in assignments.iter().enumerate() {
            if cluster < k {
                clusters[cluster].push(i);
            }
        }

        for (cluster_idx, member_indices) in clusters.iter().enumerate() {
            if member_indices.is_empty() {
                continue;
            }

            // Generate cluster summary via LLM
            let cluster_text: String = member_indices
                .iter()
                .map(|&i| items[i].text.as_str())
                .collect::<Vec<_>>()
                .join("\n---\n");
            let summary = match self
                .llm
                .chat(
                    SUMMARY_SYSTEM_PROMPT,
                    &truncate_for_llm(&cluster_text, 4000),
                    0.3,
                )
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    warn!(cluster = cluster_idx, error = %e, "failed to generate cluster summary");
                    format!("Cluster {} ({} items)", cluster_idx, member_indices.len())
                }
            };

            // Create cluster parent node
            let parent_node = RaptorRepo::insert_node(
                &self.pool,
                tree_id,
                1,
                None,
                "cluster",
                &format!("cluster_{cluster_idx}"),
                Some(&summary),
                member_indices.len() as i32,
            )
            .await?;
            total_nodes += 1;

            self.embed_cluster_summary(cluster_idx, &parent_node.id, project_id, &summary)
                .await;

            // Create leaf nodes for this cluster
            for &idx in member_indices {
                RaptorRepo::insert_node(
                    &self.pool,
                    tree_id,
                    0,
                    Some(&parent_node.id),
                    &items[idx].entity_type,
                    &items[idx].entity_id,
                    None,
                    0,
                )
                .await?;
                total_nodes += 1;
            }
        }

        Ok(total_nodes)
    }

    /// Embed a cluster summary into Qdrant. Logs warnings on failure.
    async fn embed_cluster_summary(
        &self,
        cluster_idx: usize,
        node_id: &str,
        project_id: Option<&str>,
        summary: &str,
    ) {
        match self.embedding.embed_text(&[summary]).await {
            Ok(vecs) => {
                if let Some(vec) = vecs.into_iter().next()
                    && let Err(e) = DenseVectorOps::upsert_text(
                        self.qdrant.client(),
                        "raptor_node",
                        node_id,
                        project_id,
                        vec,
                    )
                    .await
                {
                    warn!(
                        cluster = cluster_idx,
                        error = %e,
                        "failed to embed cluster summary into Qdrant"
                    );
                }
            }
            Err(e) => {
                warn!(
                    cluster = cluster_idx,
                    error = %e,
                    "failed to generate embedding for cluster summary"
                );
            }
        }
    }

    /// Fetch all knowledge items, episodes, and procedures for a project.
    async fn fetch_all_items(&self, project_id: Option<&str>) -> Result<Vec<ClusterItem>> {
        let mut items = Vec::new();

        // Knowledge items
        let knowledge = KnowledgeRepo::list(
            &self.pool,
            &ListKnowledgeFilter {
                project: project_id.map(|s| s.to_string()),
                limit: Some(1000),
                ..Default::default()
            },
        )
        .await?;

        for k in knowledge {
            items.push(ClusterItem {
                entity_type: "knowledge_item".to_string(),
                entity_id: k.id,
                text: format!("{}\n{}", k.title, k.content),
            });
        }

        // Episodes
        let episodes = EpisodeRepo::list(
            &self.pool,
            &ListEpisodesFilter {
                project: project_id.map(|s| s.to_string()),
                limit: Some(1000),
                ..Default::default()
            },
        )
        .await?;

        for e in episodes {
            items.push(ClusterItem {
                entity_type: "episode".to_string(),
                entity_id: e.id,
                text: format!("{}\n{}", e.title, e.content),
            });
        }

        // Procedures
        let procedures = ProcedureRepo::list(
            &self.pool,
            &ListProceduresFilter {
                project: project_id.map(|s| s.to_string()),
                limit: Some(1000),
                ..Default::default()
            },
        )
        .await?;

        for p in procedures {
            items.push(ClusterItem {
                entity_type: "procedure".to_string(),
                entity_id: p.id,
                text: format!("{}\n{}", p.title, p.content),
            });
        }

        Ok(items)
    }
}

/// Simple K-Means clustering implementation.
///
/// Uses K-Means++ initialization: picks first centroid randomly from data,
/// then picks subsequent centroids with probability proportional to squared
/// distance from nearest existing centroid.
///
/// Returns cluster assignments for each data point.
fn kmeans(embeddings: &[Vec<f32>], k: usize, max_iters: usize) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 || k == 0 {
        return vec![];
    }
    if k >= n {
        return (0..n).collect();
    }

    let dim = embeddings[0].len();

    // K-Means++ initialization
    let mut centroids: Vec<Vec<f32>> = Vec::with_capacity(k);

    // K-Means++ init: first centroid picked randomly
    use rand::Rng;
    let mut rng = rand::thread_rng();
    centroids.push(embeddings[rng.gen_range(0..n)].clone());

    for _ in 1..k {
        // Compute squared distances to nearest centroid
        let distances: Vec<f32> = embeddings
            .iter()
            .map(|point| {
                centroids
                    .iter()
                    .map(|c| squared_euclidean(point, c))
                    .fold(f32::MAX, f32::min)
            })
            .collect();

        // K-Means++ proportional selection: pick with probability ∝ D(x)²
        let total: f32 = distances.iter().sum();
        if total <= 0.0 {
            // All points are at centroid locations; pick next unique point
            let next = centroids.len() % n;
            centroids.push(embeddings[next].clone());
            continue;
        }

        let threshold: f32 = rng.gen_range(0.0..total);
        let mut cumulative = 0.0_f32;
        let mut selected = 0;
        for (i, &d) in distances.iter().enumerate() {
            cumulative += d;
            if cumulative >= threshold {
                selected = i;
                break;
            }
        }

        centroids.push(embeddings[selected].clone());
    }

    // Iterate
    let mut assignments = vec![0_usize; n];

    for _iter in 0..max_iters {
        // Assign each point to nearest centroid
        let mut changed = false;
        for (i, point) in embeddings.iter().enumerate() {
            let nearest = centroids
                .iter()
                .enumerate()
                .map(|(j, c)| (j, squared_euclidean(point, c)))
                .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(j, _)| j)
                .unwrap_or(0);

            if assignments[i] != nearest {
                assignments[i] = nearest;
                changed = true;
            }
        }

        if !changed {
            break;
        }

        // Recalculate centroids
        let mut new_centroids = vec![vec![0.0_f32; dim]; k];
        let mut counts = vec![0_usize; k];

        for (i, point) in embeddings.iter().enumerate() {
            let cluster = assignments[i];
            counts[cluster] += 1;
            for (j, &val) in point.iter().enumerate() {
                new_centroids[cluster][j] += val;
            }
        }

        for (cluster, centroid) in new_centroids.iter_mut().enumerate() {
            if counts[cluster] > 0 {
                let count = counts[cluster] as f32;
                for val in centroid.iter_mut() {
                    *val /= count;
                }
            }
        }

        centroids = new_centroids;
    }

    assignments
}

/// Compute the silhouette score using a random sample for efficiency.
///
/// For datasets larger than `MAX_SAMPLE`, evaluates only a random subset
/// of points. This reduces complexity from O(n²k) to O(s²k) where s = min(n, MAX_SAMPLE).
fn silhouette_score(embeddings: &[Vec<f32>], assignments: &[usize], k: usize) -> f64 {
    let n = embeddings.len();
    if n <= 1 || k <= 1 {
        return 0.0;
    }

    // Sample at most MAX_SAMPLE points for efficiency
    const MAX_SAMPLE: usize = 100;
    let indices: Vec<usize> = if n <= MAX_SAMPLE {
        (0..n).collect()
    } else {
        use rand::seq::SliceRandom;
        let mut all: Vec<usize> = (0..n).collect();
        let mut rng = rand::thread_rng();
        all.shuffle(&mut rng);
        all.truncate(MAX_SAMPLE);
        all
    };

    let mut total_score: f64 = 0.0;
    let mut valid_count: usize = 0;

    for &i in &indices {
        let cluster_i = assignments[i];

        // Compute average distance to same cluster (a)
        let mut same_cluster_dist: f64 = 0.0;
        let mut same_cluster_count: usize = 0;

        for j in 0..n {
            if i == j {
                continue;
            }
            if assignments[j] == cluster_i {
                same_cluster_dist += euclidean(&embeddings[i], &embeddings[j]) as f64;
                same_cluster_count += 1;
            }
        }

        if same_cluster_count == 0 {
            continue;
        }
        let a = same_cluster_dist / same_cluster_count as f64;

        // Compute minimum average distance to any other cluster (b)
        let mut min_b = f64::MAX;
        for c in 0..k {
            if c == cluster_i {
                continue;
            }
            let mut other_dist: f64 = 0.0;
            let mut other_count: usize = 0;
            for j in 0..n {
                if assignments[j] == c {
                    other_dist += euclidean(&embeddings[i], &embeddings[j]) as f64;
                    other_count += 1;
                }
            }
            if other_count > 0 {
                let avg = other_dist / other_count as f64;
                if avg < min_b {
                    min_b = avg;
                }
            }
        }

        if min_b == f64::MAX {
            continue;
        }
        let b = min_b;
        let max_ab = a.max(b);
        if max_ab > 0.0 {
            total_score += (b - a) / max_ab;
            valid_count += 1;
        }
    }

    if valid_count == 0 {
        0.0
    } else {
        total_score / valid_count as f64
    }
}

/// Squared Euclidean distance between two vectors.
fn squared_euclidean(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| (x - y) * (x - y))
        .sum()
}

/// Euclidean distance between two vectors.
fn euclidean(a: &[f32], b: &[f32]) -> f32 {
    squared_euclidean(a, b).sqrt()
}

/// Truncate text for LLM consumption.
fn truncate_for_llm(s: &str, max_chars: usize) -> String {
    alaz_core::truncate_utf8(s, max_chars).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kmeans_basic() {
        // Two clear clusters
        let embeddings = vec![
            vec![0.0, 0.0],
            vec![0.1, 0.1],
            vec![0.2, 0.0],
            vec![10.0, 10.0],
            vec![10.1, 10.1],
            vec![10.2, 10.0],
        ];

        let assignments = kmeans(&embeddings, 2, 20);
        assert_eq!(assignments.len(), 6);

        // First three should be in same cluster, last three in another
        assert_eq!(assignments[0], assignments[1]);
        assert_eq!(assignments[1], assignments[2]);
        assert_eq!(assignments[3], assignments[4]);
        assert_eq!(assignments[4], assignments[5]);
        assert_ne!(assignments[0], assignments[3]);
    }

    #[test]
    fn test_silhouette_two_clusters() {
        let embeddings = vec![
            vec![0.0, 0.0],
            vec![0.1, 0.1],
            vec![10.0, 10.0],
            vec![10.1, 10.1],
        ];

        let assignments = vec![0, 0, 1, 1];
        let score = silhouette_score(&embeddings, &assignments, 2);

        // With clear clusters, silhouette should be close to 1.0
        assert!(score > 0.9, "Expected high silhouette, got {score}");
    }

    #[test]
    fn test_kmeans_empty() {
        let embeddings: Vec<Vec<f32>> = vec![];
        let assignments = kmeans(&embeddings, 2, 20);
        assert!(assignments.is_empty());
    }

    #[test]
    fn test_kmeans_single_point() {
        let embeddings = vec![vec![1.0, 2.0]];
        let assignments = kmeans(&embeddings, 1, 20);
        assert_eq!(assignments.len(), 1);
    }
}
