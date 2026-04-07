use alaz_core::{AlazError, Result};
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, Filter, PointStruct, SearchPointsBuilder, UpsertPointsBuilder,
};
use tracing::debug;

use crate::client::{COLLECTION_COLBERT, point_id};

/// Compute the average of a set of token embeddings.
pub(crate) fn average_embedding(tokens: &[Vec<f32>]) -> Vec<f32> {
    if tokens.is_empty() {
        return vec![];
    }

    let dim = tokens[0].len();
    let mut avg = vec![0.0_f32; dim];
    let count = tokens.len() as f32;

    for token in tokens {
        for (i, &val) in token.iter().enumerate() {
            if i < dim {
                avg[i] += val;
            }
        }
    }

    for v in &mut avg {
        *v /= count;
    }

    avg
}

// Re-export from alaz-core to avoid duplication.
pub(crate) use alaz_core::cosine_similarity;

/// Compute MaxSim score between query tokens and document tokens.
///
/// MaxSim = sum over each query token of the maximum cosine similarity
/// between that query token and all document tokens.
pub(crate) fn max_sim(query_tokens: &[Vec<f32>], doc_tokens: &[Vec<f32>]) -> f32 {
    query_tokens
        .iter()
        .map(|qt| {
            doc_tokens
                .iter()
                .map(|dt| cosine_similarity(qt, dt))
                .fold(f32::NEG_INFINITY, f32::max)
        })
        .sum()
}

/// Operations for ColBERT multi-vector embeddings.
pub struct ColbertOps;

impl ColbertOps {
    /// Upsert a document's ColBERT token embeddings.
    ///
    /// The point vector is the average of all token embeddings (for initial retrieval).
    /// The full token vectors are stored in the payload as JSON for client-side MaxSim reranking.
    pub async fn upsert_colbert(
        client: &Qdrant,
        entity_type: &str,
        entity_id: &str,
        project_id: Option<&str>,
        token_embeddings: Vec<Vec<f32>>,
    ) -> Result<()> {
        if token_embeddings.is_empty() {
            return Err(AlazError::Validation(
                "token_embeddings cannot be empty".into(),
            ));
        }

        let pid = point_id(entity_type, entity_id);
        let avg = average_embedding(&token_embeddings);

        // Serialize token embeddings as JSON for payload storage
        let tokens_json = serde_json::to_string(&token_embeddings)
            .map_err(|e| AlazError::Qdrant(format!("failed to serialize token embeddings: {e}")))?;

        let mut payload = qdrant_client::Payload::new();
        payload.insert("entity_type", entity_type);
        payload.insert("entity_id", entity_id);
        payload.insert("token_vectors", tokens_json.as_str());
        if let Some(project) = project_id {
            payload.insert("project_id", project);
        }

        let point = PointStruct::new(pid, avg, payload);

        client
            .upsert_points(UpsertPointsBuilder::new(COLLECTION_COLBERT, vec![point]).wait(true))
            .await
            .map_err(|e| {
                AlazError::Qdrant(format!(
                    "failed to upsert ColBERT {entity_type}:{entity_id}: {e}"
                ))
            })?;

        debug!(
            entity_type,
            entity_id,
            num_tokens = token_embeddings.len(),
            "upserted ColBERT embeddings"
        );
        Ok(())
    }

    /// Search using ColBERT: initial retrieval via average embedding, then MaxSim reranking.
    ///
    /// 1. Search the colbert collection using the average of query token embeddings.
    /// 2. Retrieve candidate points with their stored token vectors.
    /// 3. Rerank candidates using MaxSim between query tokens and document tokens.
    ///
    /// Returns `(entity_type, entity_id, maxsim_score)` sorted by score descending.
    pub async fn search_colbert(
        client: &Qdrant,
        query_tokens: Vec<Vec<f32>>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>> {
        if query_tokens.is_empty() {
            return Ok(vec![]);
        }

        let avg_query = average_embedding(&query_tokens);

        // Retrieve more candidates than needed for reranking
        let candidate_limit = (limit * 4).max(20) as u64;

        let mut builder = SearchPointsBuilder::new(COLLECTION_COLBERT, avg_query, candidate_limit)
            .with_payload(true);

        if let Some(project_id) = project {
            let filter = Filter::must([Condition::matches("project_id", project_id.to_string())]);
            builder = builder.filter(filter);
        }

        let response = client
            .search_points(builder)
            .await
            .map_err(|e| AlazError::Qdrant(format!("failed to search ColBERT collection: {e}")))?;

        // Rerank with MaxSim
        let mut scored: Vec<(String, String, f32)> = response
            .result
            .into_iter()
            .filter_map(|point| {
                let entity_type = point
                    .payload
                    .get("entity_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let entity_id = point
                    .payload
                    .get("entity_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let token_vectors_json = point
                    .payload
                    .get("token_vectors")
                    .and_then(|v| v.as_str())?;

                let doc_tokens: Vec<Vec<f32>> = serde_json::from_str(token_vectors_json).ok()?;

                let score = max_sim(&query_tokens, &doc_tokens);
                Some((entity_type, entity_id, score))
            })
            .collect();

        // Sort by MaxSim score descending
        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- average_embedding ---

    #[test]
    fn average_embedding_empty_input() {
        let result = average_embedding(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn average_embedding_single_vector() {
        let tokens = vec![vec![1.0, 2.0, 3.0]];
        let avg = average_embedding(&tokens);
        assert_eq!(avg, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn average_embedding_multiple_vectors() {
        let tokens = vec![vec![1.0, 0.0, 3.0], vec![3.0, 4.0, 1.0]];
        let avg = average_embedding(&tokens);
        assert!((avg[0] - 2.0).abs() < 0.001);
        assert!((avg[1] - 2.0).abs() < 0.001);
        assert!((avg[2] - 2.0).abs() < 0.001);
    }

    // --- cosine_similarity ---

    #[test]
    fn cosine_identical_vectors_returns_one() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 0.001, "expected ~1.0, got {sim}");
    }

    #[test]
    fn cosine_orthogonal_vectors_returns_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 0.001, "expected ~0.0, got {sim}");
    }

    #[test]
    fn cosine_opposite_vectors_returns_neg_one() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 0.001, "expected ~-1.0, got {sim}");
    }

    #[test]
    fn cosine_zero_vector_returns_zero() {
        let a = vec![1.0, 2.0, 3.0];
        let zero = vec![0.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &zero)).abs() < 0.001);
        assert!((cosine_similarity(&zero, &a)).abs() < 0.001);
        assert!((cosine_similarity(&zero, &zero)).abs() < 0.001);
    }

    // --- max_sim ---

    #[test]
    fn max_sim_single_tokens() {
        let query = vec![vec![1.0, 0.0]];
        let doc = vec![vec![1.0, 0.0]];
        let score = max_sim(&query, &doc);
        assert!((score - 1.0).abs() < 0.001, "expected ~1.0, got {score}");
    }

    #[test]
    fn max_sim_multiple_tokens() {
        // 2 query tokens, 2 doc tokens
        let query = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let doc = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        // Each query token has a perfect match → max_sim = 1.0 + 1.0 = 2.0
        let score = max_sim(&query, &doc);
        assert!((score - 2.0).abs() < 0.001, "expected ~2.0, got {score}");
    }

    // --- point_id ---

    #[test]
    fn point_id_deterministic() {
        let id1 = point_id("knowledge_item", "abc123");
        let id2 = point_id("knowledge_item", "abc123");
        assert_eq!(id1, id2);
    }

    #[test]
    fn point_id_different_inputs_differ() {
        let id1 = point_id("knowledge_item", "abc123");
        let id2 = point_id("episode", "abc123");
        let id3 = point_id("knowledge_item", "xyz789");
        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
    }
}
