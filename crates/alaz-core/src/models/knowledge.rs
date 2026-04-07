use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Embeddable;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgeItem {
    pub id: String,
    pub title: String,
    pub content: String,
    pub description: Option<String>,
    pub kind: String,
    pub language: Option<String>,
    pub file_path: Option<String>,
    pub project_id: Option<String>,
    pub tags: Vec<String>,
    pub utility_score: f64,
    pub access_count: i64,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub needs_embedding: bool,
    pub feedback_boost: f32,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub superseded_by: Option<String>,
    pub invalidation_reason: Option<String>,
    pub source: Option<String>,
    pub source_metadata: Option<serde_json::Value>,
    pub times_used: i64,
    pub times_success: i64,
    pub pattern_score: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateKnowledge {
    pub title: String,
    pub content: String,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub file_path: Option<String>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub source: Option<String>,
    pub source_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateKnowledge {
    pub title: Option<String>,
    pub content: Option<String>,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub file_path: Option<String>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListKnowledgeFilter {
    pub project: Option<String>,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub tag: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Embeddable for KnowledgeItem {
    fn entity_type_name(&self) -> &'static str {
        "knowledge_item"
    }
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }
    fn embed_content(&self) -> String {
        self.content.clone()
    }
    fn needs_colbert(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_knowledge_serde_round_trip() {
        let json = r#"{
            "title": "Test Pattern",
            "content": "Use X for Y",
            "description": "A useful pattern",
            "kind": "pattern",
            "language": "rust",
            "file_path": "src/main.rs",
            "project": "alaz",
            "tags": ["rust", "pattern"],
            "source": "manual"
        }"#;
        let parsed: CreateKnowledge = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.title, "Test Pattern");
        assert_eq!(parsed.kind.unwrap(), "pattern");
        assert_eq!(parsed.tags.unwrap(), vec!["rust", "pattern"]);
    }

    #[test]
    fn create_knowledge_minimal() {
        let json = r#"{"title": "Min", "content": "body"}"#;
        let parsed: CreateKnowledge = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.title, "Min");
        assert!(parsed.kind.is_none());
        assert!(parsed.tags.is_none());
        assert!(parsed.project.is_none());
    }

    #[test]
    fn list_knowledge_filter_defaults() {
        let filter = ListKnowledgeFilter::default();
        assert!(filter.project.is_none());
        assert!(filter.kind.is_none());
        assert!(filter.language.is_none());
        assert!(filter.tag.is_none());
        assert!(filter.limit.is_none());
        assert!(filter.offset.is_none());
    }
}
