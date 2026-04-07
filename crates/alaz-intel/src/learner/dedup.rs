use alaz_core::{Result, cosine_similarity};
use alaz_db::repos::{CoreMemoryRepo, EpisodeRepo, KnowledgeRepo, ProcedureRepo};
use alaz_vector::DenseVectorOps;
use tracing::debug;

/// In-memory dedup buffer for the current pipeline run.
/// Catches semantic duplicates between chunks of the same session
/// before the backfill job has had time to embed them into Qdrant.
pub(crate) struct SessionDedup {
    /// (entity_type, embedding_vector) pairs saved during this run.
    entries: Vec<(String, Vec<f32>)>,
}

impl SessionDedup {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Check if a new item is semantically similar to anything already saved in this session.
    pub(crate) fn is_duplicate(
        &self,
        entity_type: &str,
        embedding: &[f32],
        threshold: f32,
    ) -> bool {
        for (etype, existing) in &self.entries {
            if etype == entity_type && cosine_similarity(embedding, existing) > threshold {
                return true;
            }
        }
        false
    }

    /// Record an item that was saved during this session.
    pub(crate) fn record(&mut self, entity_type: &str, embedding: Vec<f32>) {
        self.entries.push((entity_type.to_string(), embedding));
    }
}

impl super::SessionLearner {
    /// Generic three-layer dedup check: trigram → session-local buffer → Qdrant vector.
    ///
    /// - `entity_type`: "knowledge_item", "episode", or "procedure"
    /// - `trigram_checker`: async future performing the repo-specific trigram check
    /// - `vector_threshold`: score above which a Qdrant match is considered duplicate (0.85 for all)
    pub(crate) async fn is_duplicate<Fut>(
        &self,
        entity_type: &str,
        title: &str,
        project_id: Option<&str>,
        session_buf: &SessionDedup,
        vector_threshold: f32,
        trigram_checker: Fut,
    ) -> Result<bool>
    where
        Fut: std::future::Future<Output = Result<bool>>,
    {
        // Layer 1: Trigram similarity (fast, catches obvious duplicates)
        if trigram_checker.await? {
            return Ok(true);
        }

        // Layer 2 + 3: Vector similarity (Qdrant for previous sessions, buffer for current)
        if let Ok(vecs) = self.embedding.embed_text(&[title]).await
            && let Some(embedding) = vecs.into_iter().next()
        {
            // Layer 3: Session-local buffer (catches same-session cross-chunk dupes)
            if session_buf.is_duplicate(entity_type, &embedding, vector_threshold) {
                debug!(title, entity_type, "session buffer caught duplicate");
                return Ok(true);
            }

            // Layer 2: Qdrant (catches cross-session semantic dupes)
            if let Ok(results) =
                DenseVectorOps::search_text(self.qdrant.client(), embedding, project_id, 5).await
            {
                for (etype, _entity_id, score) in &results {
                    // Knowledge items may be stored as "knowledge" or "knowledge_item"
                    let matches = etype == entity_type
                        || (entity_type == "knowledge_item" && etype == "knowledge");
                    if matches && *score > vector_threshold {
                        debug!(title, score, entity_type, "vector dedup caught duplicate");
                        return Ok(true);
                    }
                }
            }
        }

        Ok(false)
    }

    /// Check if a knowledge item with a similar title already exists.
    pub(crate) async fn is_duplicate_knowledge(
        &self,
        title: &str,
        project_id: Option<&str>,
        session_buf: &SessionDedup,
    ) -> Result<bool> {
        let pool = &self.pool;
        let trigram_check = async {
            let similar =
                KnowledgeRepo::find_similar_by_title(pool, title, 0.4, project_id).await?;
            Ok(!similar.is_empty())
        };
        self.is_duplicate(
            "knowledge_item",
            title,
            project_id,
            session_buf,
            0.85,
            trigram_check,
        )
        .await
    }

    /// Check if an episode with a similar title already exists.
    pub(crate) async fn is_duplicate_episode(
        &self,
        title: &str,
        project_id: Option<&str>,
        session_buf: &SessionDedup,
    ) -> Result<bool> {
        let pool = &self.pool;
        let trigram_check = async {
            let similar = EpisodeRepo::find_similar_by_title(pool, title, 0.35, project_id).await?;
            Ok(!similar.is_empty())
        };
        self.is_duplicate(
            "episode",
            title,
            project_id,
            session_buf,
            0.85,
            trigram_check,
        )
        .await
    }

    /// Check if a procedure with a similar title already exists.
    pub(crate) async fn is_duplicate_procedure(
        &self,
        title: &str,
        project_id: Option<&str>,
        session_buf: &SessionDedup,
    ) -> Result<bool> {
        let pool = &self.pool;
        let trigram_check = async {
            let similar =
                ProcedureRepo::find_similar_by_title(pool, title, 0.35, project_id).await?;
            Ok(!similar.is_empty())
        };
        self.is_duplicate(
            "procedure",
            title,
            project_id,
            session_buf,
            0.85,
            trigram_check,
        )
        .await
    }

    /// Check if a core memory with a similar key already exists in the same category.
    /// If found, returns the existing key so we can reuse it for upsert.
    /// Two-layer: trigram on key first, then vector similarity on "category:key" text.
    pub(crate) async fn find_existing_memory_key(
        &self,
        category: &str,
        key: &str,
        project_id: Option<&str>,
    ) -> Result<Option<String>> {
        // Layer 1: Trigram on key within same category
        let similar =
            CoreMemoryRepo::find_similar_by_key(&self.pool, category, key, 0.3, project_id).await?;
        if let Some(existing) = similar.first() {
            return Ok(Some(existing.key.clone()));
        }

        // Layer 2: Vector similarity on "category:key" text
        let search_text = format!("{category}: {key}");
        if let Ok(vecs) = self.embedding.embed_text(&[&search_text]).await
            && let Some(embedding) = vecs.into_iter().next()
            && let Ok(results) =
                DenseVectorOps::search_text(self.qdrant.client(), embedding, project_id, 5).await
        {
            for (entity_type, entity_id, score) in &results {
                if entity_type == "core_memory" && *score > 0.88 {
                    // Fetch the actual memory to get its key
                    if let Ok(mem) = CoreMemoryRepo::get(&self.pool, entity_id).await
                        && mem.category == category
                    {
                        debug!(
                            key, existing_key = %mem.key, score,
                            "vector dedup found existing memory key"
                        );
                        return Ok(Some(mem.key.clone()));
                    }
                }
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alaz_core::cosine_similarity;

    // === cosine_similarity tests ===

    #[test]
    fn test_cosine_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_empty_vectors() {
        let a: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &a), 0.0);
    }

    #[test]
    fn test_cosine_zero_vector() {
        let a = vec![0.0, 0.0];
        let b = vec![1.0, 2.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    // === SessionDedup tests ===

    #[test]
    fn test_session_dedup_empty() {
        let dedup = SessionDedup::new();
        let vec = vec![1.0, 0.0, 0.0];
        assert!(!dedup.is_duplicate("knowledge_item", &vec, 0.85));
    }

    #[test]
    fn test_session_dedup_records_and_detects() {
        let mut dedup = SessionDedup::new();
        let vec1 = vec![1.0, 0.0, 0.0];
        dedup.record("knowledge_item", vec1.clone());
        // Same vector should be detected as duplicate
        assert!(dedup.is_duplicate("knowledge_item", &vec1, 0.85));
    }

    #[test]
    fn test_session_dedup_different_entity_type() {
        let mut dedup = SessionDedup::new();
        let vec1 = vec![1.0, 0.0, 0.0];
        dedup.record("knowledge_item", vec1.clone());
        // Same vector but different entity type — not a duplicate
        assert!(!dedup.is_duplicate("episode", &vec1, 0.85));
    }
}
