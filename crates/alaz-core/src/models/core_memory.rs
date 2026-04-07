use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Embeddable;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CoreMemory {
    pub id: String,
    pub category: String,
    pub key: String,
    pub value: String,
    pub confidence: f64,
    pub confirmations: i64,
    pub contradictions: i64,
    pub project_id: Option<String>,
    pub needs_embedding: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertCoreMemory {
    pub category: String,
    pub key: String,
    pub value: String,
    pub confidence: Option<f64>,
    pub project: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListCoreMemoryFilter {
    pub project: Option<String>,
    pub category: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Embeddable for CoreMemory {
    fn entity_type_name(&self) -> &'static str {
        "core_memory"
    }
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }
    fn embed_content(&self) -> String {
        format!("{}: {} — {}", self.category, self.key, self.value)
    }
    fn needs_colbert(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_core_memory_serde_round_trip() {
        let json = r#"{
            "category": "fact",
            "key": "db_port",
            "value": "5434",
            "confidence": 0.95,
            "project": "alaz"
        }"#;
        let parsed: UpsertCoreMemory = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.category, "fact");
        assert_eq!(parsed.key, "db_port");
        assert_eq!(parsed.value, "5434");
        assert!((parsed.confidence.unwrap() - 0.95).abs() < 0.001);
        assert_eq!(parsed.project.unwrap(), "alaz");
    }

    #[test]
    fn upsert_core_memory_minimal() {
        let json = r#"{"category": "preference", "key": "lang", "value": "tr"}"#;
        let parsed: UpsertCoreMemory = serde_json::from_str(json).unwrap();
        assert!(parsed.confidence.is_none());
        assert!(parsed.project.is_none());
    }

    #[test]
    fn list_core_memory_filter_defaults() {
        let filter = ListCoreMemoryFilter::default();
        assert!(filter.project.is_none());
        assert!(filter.category.is_none());
        assert!(filter.limit.is_none());
        assert!(filter.offset.is_none());
    }
}
