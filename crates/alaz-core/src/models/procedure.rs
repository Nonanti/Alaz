use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Embeddable;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Procedure {
    pub id: String,
    pub title: String,
    pub content: String,
    pub steps: serde_json::Value,
    pub times_used: i64,
    pub success: i64,
    pub failure: i64,
    pub success_rate: Option<f64>,
    pub project_id: Option<String>,
    pub tags: Vec<String>,
    pub utility_score: f64,
    pub access_count: i64,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub needs_embedding: bool,
    pub feedback_boost: f32,
    pub superseded_by: Option<String>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub source: Option<String>,
    pub source_metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProcedure {
    pub title: String,
    pub content: String,
    pub steps: Option<serde_json::Value>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub source_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListProceduresFilter {
    pub project: Option<String>,
    pub tag: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Embeddable for Procedure {
    fn entity_type_name(&self) -> &'static str {
        "procedure"
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
    fn create_procedure_serde_round_trip() {
        let json = r#"{
            "title": "Deploy steps",
            "content": "How to deploy Alaz",
            "steps": ["build", "rsync", "restart"],
            "project": "alaz",
            "tags": ["deploy", "ops"]
        }"#;
        let parsed: CreateProcedure = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.title, "Deploy steps");
        let steps = parsed.steps.unwrap();
        assert_eq!(steps.as_array().unwrap().len(), 3);
        assert_eq!(parsed.tags.unwrap(), vec!["deploy", "ops"]);
    }

    #[test]
    fn create_procedure_minimal() {
        let json = r#"{"title": "Min", "content": "body"}"#;
        let parsed: CreateProcedure = serde_json::from_str(json).unwrap();
        assert!(parsed.steps.is_none());
        assert!(parsed.tags.is_none());
        assert!(parsed.project.is_none());
    }

    #[test]
    fn list_procedures_filter_defaults() {
        let filter = ListProceduresFilter::default();
        assert!(filter.project.is_none());
        assert!(filter.tag.is_none());
        assert!(filter.limit.is_none());
        assert!(filter.offset.is_none());
    }
}
