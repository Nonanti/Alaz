use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GraphEdge {
    pub id: String,
    pub source_type: String,
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub relation: String,
    pub weight: f64,
    pub usage_count: i64,
    pub description: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRef {
    pub entity_type: String,
    pub entity_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRelation {
    pub source_type: String,
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub relation: String,
    pub weight: Option<f64>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct GraphExploreParams {
    pub entity_type: String,
    pub entity_id: String,
    pub depth: Option<u32>,
    pub min_weight: Option<f64>,
    pub relation_filter: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoredEntity {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub score: f64,
    pub relation: String,
    pub depth: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_ref_serde_round_trip() {
        let er = EntityRef {
            entity_type: "knowledge_item".into(),
            entity_id: "abc123".into(),
        };
        let json = serde_json::to_string(&er).unwrap();
        let parsed: EntityRef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.entity_type, "knowledge_item");
        assert_eq!(parsed.entity_id, "abc123");
    }

    #[test]
    fn create_relation_serde() {
        let json = r#"{
            "source_type": "episode",
            "source_id": "ep1",
            "target_type": "knowledge_item",
            "target_id": "ki1",
            "relation": "led_to",
            "weight": 0.8,
            "description": "Bug led to fix"
        }"#;
        let parsed: CreateRelation = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.relation, "led_to");
        assert!((parsed.weight.unwrap() - 0.8).abs() < 0.001);
        assert!(parsed.metadata.is_none());
    }

    #[test]
    fn graph_explore_params_defaults() {
        let json = r#"{"entity_type": "episode", "entity_id": "ep1"}"#;
        let parsed: GraphExploreParams = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.entity_type, "episode");
        assert!(parsed.depth.is_none());
        assert!(parsed.min_weight.is_none());
        assert!(parsed.relation_filter.is_none());
    }
}
