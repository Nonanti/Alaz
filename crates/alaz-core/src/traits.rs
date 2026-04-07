use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub content: String,
    pub score: f64,
    pub project: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct SignalResult {
    pub entity_type: String,
    pub entity_id: String,
    pub rank: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub project: Option<String>,
    pub limit: Option<usize>,
    pub rerank: Option<bool>,
    pub hyde: Option<bool>,
    pub graph_expand: Option<bool>,
}

/// Trait for entities that can be embedded into vector collections.
///
/// Implemented by KnowledgeItem, Episode, Procedure, CoreMemory, and Reflection.
/// Used by the embedding backfill job to process all entity types generically.
pub trait Embeddable: Send + Sync {
    /// The entity type name stored in Qdrant payload (e.g. "knowledge", "episode").
    fn entity_type_name(&self) -> &'static str;

    /// The entity's unique ID.
    fn entity_id(&self) -> &str;

    /// Optional project ID for filtering.
    fn project_id(&self) -> Option<&str>;

    /// The text content to embed.
    fn embed_content(&self) -> String;

    /// Whether this entity should also get ColBERT token-level embeddings.
    /// Short key-value items (CoreMemory, Reflection) return false.
    fn needs_colbert(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_defaults() {
        let q: SearchQuery = serde_json::from_str(r#"{"query": "hello"}"#).unwrap();
        assert_eq!(q.query, "hello");
        assert!(q.project.is_none());
        assert!(q.limit.is_none());
        assert!(q.rerank.is_none());
        assert!(q.hyde.is_none());
        assert!(q.graph_expand.is_none());
    }

    #[test]
    fn search_query_with_all_fields() {
        let q: SearchQuery = serde_json::from_str(
            r#"{"query": "test", "project": "alaz", "limit": 10, "rerank": true, "hyde": false, "graph_expand": true}"#,
        ).unwrap();
        assert_eq!(q.query, "test");
        assert_eq!(q.project.unwrap(), "alaz");
        assert_eq!(q.limit.unwrap(), 10);
        assert!(q.rerank.unwrap());
        assert!(!q.hyde.unwrap());
        assert!(q.graph_expand.unwrap());
    }

    #[test]
    fn signal_result_construction() {
        let sr = SignalResult {
            entity_type: "knowledge_item".into(),
            entity_id: "abc123".into(),
            rank: 3,
        };
        assert_eq!(sr.entity_type, "knowledge_item");
        assert_eq!(sr.entity_id, "abc123");
        assert_eq!(sr.rank, 3);
    }

    #[test]
    fn search_result_serializes() {
        let sr = SearchResult {
            entity_type: "episode".into(),
            entity_id: "ep1".into(),
            title: "Test".into(),
            content: "Content".into(),
            score: 0.95,
            project: Some("alaz".into()),
            metadata: None,
        };
        let json = serde_json::to_value(&sr).unwrap();
        assert_eq!(json["entity_type"], "episode");
        assert_eq!(json["score"], 0.95);
        assert_eq!(json["project"], "alaz");
    }
}
