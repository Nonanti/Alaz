//! Knowledge consolidation pipeline.
//!
//! Periodically merges similar knowledge items to prevent noise accumulation.
//! Groups items by vector similarity, generates LLM-powered merged summaries,
//! and supersedes the originals.

use std::sync::Arc;

use alaz_core::Result;
use alaz_core::models::{CreateKnowledge, KnowledgeItem, ListKnowledgeFilter};
use alaz_db::repos::{GraphRepo, KnowledgeRepo, ProjectRepo};
use alaz_vector::QdrantManager;
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::embeddings::EmbeddingService;
use crate::llm::LlmClient;

/// Minimum cluster size to trigger a merge.
const MIN_CLUSTER_SIZE: usize = 3;

/// Vector similarity threshold for grouping items into the same cluster.
const CLUSTER_THRESHOLD: f32 = 0.82;

/// Maximum items per project to consider (avoid processing huge backlogs).
const MAX_ITEMS_PER_PROJECT: i64 = 500;

/// Maximum characters per item content in the LLM merge prompt.
/// Prevents context window overflow when merging large items.
const MAX_CONTENT_PER_ITEM: usize = 2000;

/// Maximum total characters for the LLM merge prompt.
const MAX_MERGE_PROMPT_CHARS: usize = 16_000;

/// Maximum items to embed in a single API call.
const EMBEDDING_BATCH_SIZE: usize = 50;

const MERGE_SYSTEM_PROMPT: &str = r#"You are a knowledge consolidation assistant. You receive multiple related knowledge items and must merge them into a single, comprehensive item.

Rules:
- Combine all unique information into one cohesive item
- Resolve any contradictions by preferring the most recent information
- Remove redundant content
- Preserve specific code snippets, commands, and technical details
- Keep the merged item concise but complete
- If items have different tags, combine them

Return ONLY valid JSON:
{
  "title": "Merged title (concise, descriptive)",
  "content": "Merged content (comprehensive)",
  "tags": ["tag1", "tag2"]
}"#;

/// Result of a consolidation run.
#[derive(Debug)]
pub struct ConsolidationReport {
    /// Number of clusters detected (3+ similar items).
    pub clusters_found: usize,
    /// Number of clusters successfully merged.
    pub clusters_merged: usize,
    /// Total items superseded by merges.
    pub items_superseded: usize,
    /// Clusters skipped (LLM error, too few items after filtering, etc.).
    pub clusters_skipped: usize,
}

/// A cluster of similar knowledge items to be merged.
#[derive(Debug)]
struct Cluster {
    items: Vec<KnowledgeItem>,
    project_id: Option<String>,
}

/// LLM merge response.
#[derive(serde::Deserialize)]
struct MergeResult {
    title: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// The consolidation pipeline.
pub struct ConsolidationPipeline {
    pool: PgPool,
    llm: Arc<LlmClient>,
    embedding: Arc<EmbeddingService>,
    /// Reserved for future Qdrant vector cleanup during consolidation.
    #[allow(dead_code)]
    qdrant: Arc<QdrantManager>,
}

impl ConsolidationPipeline {
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

    /// Run the full consolidation pipeline.
    ///
    /// 1. List all projects + global (None)
    /// 2. For each scope, fetch knowledge items
    /// 3. Embed and cluster by vector similarity
    /// 4. For clusters of 3+, merge via LLM
    /// 5. Create merged item, supersede originals, transfer graph edges
    pub async fn run(&self) -> Result<ConsolidationReport> {
        let mut report = ConsolidationReport {
            clusters_found: 0,
            clusters_merged: 0,
            items_superseded: 0,
            clusters_skipped: 0,
        };

        // Collect all project scopes: each project ID + global (None)
        let projects = ProjectRepo::list(&self.pool).await?;
        let mut scopes: Vec<Option<String>> = projects.into_iter().map(|p| Some(p.id)).collect();
        scopes.push(None); // global scope

        for scope in &scopes {
            let label = scope.as_deref().unwrap_or("global");
            debug!(scope = label, "consolidation: processing scope");

            match self.consolidate_scope(scope.as_deref()).await {
                Ok(scope_report) => {
                    report.clusters_found += scope_report.clusters_found;
                    report.clusters_merged += scope_report.clusters_merged;
                    report.items_superseded += scope_report.items_superseded;
                    report.clusters_skipped += scope_report.clusters_skipped;
                }
                Err(e) => {
                    warn!(scope = label, error = %e, "consolidation: scope failed");
                }
            }
        }

        info!(
            clusters_found = report.clusters_found,
            clusters_merged = report.clusters_merged,
            items_superseded = report.items_superseded,
            clusters_skipped = report.clusters_skipped,
            "consolidation: pipeline complete"
        );

        Ok(report)
    }

    /// Consolidate items within a single project scope.
    async fn consolidate_scope(&self, project_id: Option<&str>) -> Result<ConsolidationReport> {
        let mut report = ConsolidationReport {
            clusters_found: 0,
            clusters_merged: 0,
            items_superseded: 0,
            clusters_skipped: 0,
        };

        // Fetch active (non-superseded) items
        let filter = ListKnowledgeFilter {
            project: project_id.map(String::from),
            limit: Some(MAX_ITEMS_PER_PROJECT),
            ..Default::default()
        };
        let items: Vec<KnowledgeItem> = KnowledgeRepo::list(&self.pool, &filter)
            .await?
            .into_iter()
            .filter(|item| item.superseded_by.is_none())
            .collect();

        if items.len() < MIN_CLUSTER_SIZE {
            return Ok(report);
        }

        // Embed all items in batches to avoid timeout/memory issues.
        let texts: Vec<String> = items.iter().map(|i| i.content.clone()).collect();
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(EMBEDDING_BATCH_SIZE) {
            let refs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
            match self.embedding.embed_text(&refs).await {
                Ok(batch_vecs) => embeddings.extend(batch_vecs),
                Err(e) => {
                    warn!(error = %e, "consolidation: embedding batch failed, skipping scope");
                    return Ok(report);
                }
            }
        }

        if embeddings.len() != items.len() {
            warn!(
                expected = items.len(),
                got = embeddings.len(),
                "consolidation: embedding count mismatch, skipping scope"
            );
            return Ok(report);
        }

        // Cluster by greedy nearest-neighbor grouping
        let clusters = greedy_cluster(&items, &embeddings, CLUSTER_THRESHOLD);

        // Filter to clusters of MIN_CLUSTER_SIZE or more
        let merge_candidates: Vec<Cluster> = clusters
            .into_iter()
            .filter(|c| c.items.len() >= MIN_CLUSTER_SIZE)
            .collect();

        report.clusters_found = merge_candidates.len();

        for cluster in merge_candidates {
            match self.merge_cluster(&cluster).await {
                Ok(superseded_count) => {
                    report.clusters_merged += 1;
                    report.items_superseded += superseded_count;
                }
                Err(e) => {
                    warn!(
                        cluster_size = cluster.items.len(),
                        error = %e,
                        "consolidation: merge failed"
                    );
                    report.clusters_skipped += 1;
                }
            }
        }

        Ok(report)
    }

    /// Merge a cluster of similar items into one via LLM.
    async fn merge_cluster(&self, cluster: &Cluster) -> Result<usize> {
        // Build the user prompt with all cluster items, truncating content
        // to prevent LLM context window overflow.
        let mut user_prompt = String::from("Merge these related knowledge items:\n\n");
        for (i, item) in cluster.items.iter().enumerate() {
            let content = truncate_at_char_boundary(&item.content, MAX_CONTENT_PER_ITEM);
            user_prompt.push_str(&format!(
                "--- Item {} (created: {}) ---\nTitle: {}\nTags: [{}]\n{}\n\n",
                i + 1,
                item.created_at.format("%Y-%m-%d"),
                item.title,
                item.tags.join(", "),
                content,
            ));

            // Hard cap on total prompt size
            if user_prompt.len() >= MAX_MERGE_PROMPT_CHARS {
                user_prompt.truncate(user_prompt.floor_char_boundary(MAX_MERGE_PROMPT_CHARS));
                user_prompt.push_str("\n\n[Remaining items truncated for context limits]\n");
                break;
            }
        }

        // Ask LLM to merge
        let merged: MergeResult = self
            .llm
            .chat_json(MERGE_SYSTEM_PROMPT, &user_prompt, 0.3)
            .await?;

        // Determine the best language and kind from the cluster
        let language = cluster
            .items
            .iter()
            .filter_map(|i| i.language.as_deref())
            .next()
            .map(String::from);
        let kind = cluster.items.first().map(|i| i.kind.clone());

        // Create the merged item
        let input = CreateKnowledge {
            title: merged.title,
            content: merged.content,
            description: Some("Auto-consolidated from multiple similar items".to_string()),
            kind,
            language,
            file_path: None,
            project: None, // project_id is passed separately
            tags: Some(merged.tags),
            valid_from: None,
            valid_until: None,
            source: Some("consolidation".to_string()),
            source_metadata: Some(serde_json::json!({
                "merged_from": cluster.items.iter().map(|i| &i.id).collect::<Vec<_>>(),
                "merged_count": cluster.items.len(),
            })),
        };

        let new_item =
            KnowledgeRepo::create(&self.pool, &input, cluster.project_id.as_deref()).await?;

        info!(
            new_id = %new_item.id,
            merged_count = cluster.items.len(),
            title = %new_item.title,
            "consolidation: merged cluster"
        );

        // Supersede all old items and transfer graph edges
        let mut superseded = 0;
        for old_item in &cluster.items {
            KnowledgeRepo::supersede(
                &self.pool,
                &old_item.id,
                &new_item.id,
                Some("auto-consolidated"),
            )
            .await?;

            // Transfer incoming graph edges from old → new
            self.transfer_graph_edges(&old_item.id, &new_item.id).await;

            superseded += 1;
        }

        Ok(superseded)
    }

    /// Transfer graph edges from an old entity to a new one.
    ///
    /// Best-effort: errors are logged but don't fail the consolidation.
    async fn transfer_graph_edges(&self, old_id: &str, new_id: &str) {
        // Get outgoing edges from the old item
        let outgoing = GraphRepo::get_edges(&self.pool, "knowledge_item", old_id, "outgoing")
            .await
            .unwrap_or_default();
        // Get incoming edges to the old item
        let incoming = GraphRepo::get_edges(&self.pool, "knowledge_item", old_id, "incoming")
            .await
            .unwrap_or_default();

        let mut edges = outgoing;
        edges.extend(incoming);

        for edge in &edges {
            let new_edge = alaz_core::models::CreateRelation {
                source_type: edge.source_type.clone(),
                source_id: if edge.source_id == old_id {
                    new_id.to_string()
                } else {
                    edge.source_id.clone()
                },
                target_type: edge.target_type.clone(),
                target_id: if edge.target_id == old_id {
                    new_id.to_string()
                } else {
                    edge.target_id.clone()
                },
                relation: edge.relation.clone(),
                weight: Some(edge.weight),
                description: edge.description.clone(),
                metadata: Some(edge.metadata.clone()),
            };

            if let Err(e) = GraphRepo::create_edge(&self.pool, &new_edge).await {
                debug!(
                    old_id,
                    new_id,
                    error = %e,
                    "consolidation: failed to transfer graph edge (may already exist)"
                );
            }
        }
    }
}

/// Greedy nearest-neighbor clustering.
///
/// Assigns each item to the first cluster whose centroid is within
/// `threshold` cosine similarity, or creates a new cluster.
fn greedy_cluster(
    items: &[KnowledgeItem],
    embeddings: &[Vec<f32>],
    threshold: f32,
) -> Vec<Cluster> {
    let mut clusters: Vec<(Vec<usize>, Vec<f32>)> = Vec::new(); // (item indices, centroid)

    for (i, embedding) in embeddings.iter().enumerate() {
        let mut assigned = false;

        for (indices, centroid) in &mut clusters {
            let sim = alaz_core::cosine_similarity(embedding, centroid);
            if sim >= threshold {
                indices.push(i);
                // Update centroid as running average
                update_centroid(centroid, embedding, indices.len());
                assigned = true;
                break;
            }
        }

        if !assigned {
            clusters.push((vec![i], embedding.clone()));
        }
    }

    clusters
        .into_iter()
        .map(|(indices, _centroid)| {
            let cluster_items: Vec<KnowledgeItem> =
                indices.iter().map(|&i| items[i].clone()).collect();
            let project_id = cluster_items.first().and_then(|i| i.project_id.clone());
            Cluster {
                items: cluster_items,
                project_id,
            }
        })
        .collect()
}

/// Truncate a string at a UTF-8 character boundary.
fn truncate_at_char_boundary(s: &str, max_chars: usize) -> &str {
    alaz_core::truncate_utf8(s, max_chars)
}

/// Update a centroid with a new vector using running average.
fn update_centroid(centroid: &mut [f32], new_vec: &[f32], count: usize) {
    let n = count as f32;
    for (c, v) in centroid.iter_mut().zip(new_vec.iter()) {
        *c = *c * ((n - 1.0) / n) + *v / n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_centroid_two_vectors() {
        let mut centroid = vec![1.0, 0.0, 0.0];
        let new = vec![0.0, 1.0, 0.0];
        update_centroid(&mut centroid, &new, 2);
        // Average of [1,0,0] and [0,1,0] = [0.5, 0.5, 0.0]
        assert!((centroid[0] - 0.5).abs() < 1e-6);
        assert!((centroid[1] - 0.5).abs() < 1e-6);
        assert!((centroid[2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn update_centroid_three_vectors() {
        let mut centroid = vec![0.5, 0.5, 0.0]; // already average of 2 vectors
        let new = vec![0.0, 0.0, 1.0];
        update_centroid(&mut centroid, &new, 3);
        // Average of 3: [1/3, 1/3, 1/3] ≈ [0.333, 0.333, 0.333]
        assert!((centroid[0] - 1.0 / 3.0).abs() < 1e-5);
        assert!((centroid[1] - 1.0 / 3.0).abs() < 1e-5);
        assert!((centroid[2] - 1.0 / 3.0).abs() < 1e-5);
    }
}
