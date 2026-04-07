use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Embeddable;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Episode {
    pub id: String,
    pub title: String,
    pub content: String,
    pub kind: String,
    pub severity: Option<String>,
    pub resolved: bool,
    pub who_cues: Vec<String>,
    pub what_cues: Vec<String>,
    pub where_cues: Vec<String>,
    pub when_cues: Vec<String>,
    pub why_cues: Vec<String>,
    pub project_id: Option<String>,
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
    pub action: Option<String>,
    pub outcome: Option<String>,
    pub outcome_score: Option<f64>,
    pub related_files: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEpisode {
    pub title: String,
    pub content: String,
    pub kind: Option<String>,
    pub severity: Option<String>,
    pub resolved: Option<bool>,
    pub who_cues: Option<Vec<String>>,
    pub what_cues: Option<Vec<String>>,
    pub where_cues: Option<Vec<String>>,
    pub when_cues: Option<Vec<String>>,
    pub why_cues: Option<Vec<String>>,
    pub project: Option<String>,
    pub source: Option<String>,
    pub source_metadata: Option<serde_json::Value>,
    pub action: Option<String>,
    pub outcome: Option<String>,
    pub outcome_score: Option<f64>,
    pub related_files: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListEpisodesFilter {
    pub project: Option<String>,
    pub kind: Option<String>,
    pub resolved: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Embeddable for Episode {
    fn entity_type_name(&self) -> &'static str {
        "episode"
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
    fn create_episode_serde_round_trip() {
        let json = r#"{
            "title": "Bug found",
            "content": "UTF-8 truncation issue",
            "kind": "error",
            "severity": "high",
            "resolved": false,
            "who_cues": ["alice"],
            "what_cues": ["bug", "truncation"],
            "where_cues": ["learner.rs"],
            "project": "alaz",
            "related_files": ["src/learner.rs"]
        }"#;
        let parsed: CreateEpisode = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.title, "Bug found");
        assert_eq!(parsed.severity.unwrap(), "high");
        assert_eq!(parsed.resolved, Some(false));
        assert_eq!(parsed.who_cues.unwrap(), vec!["alice"]);
        assert_eq!(parsed.related_files.unwrap(), vec!["src/learner.rs"]);
    }

    #[test]
    fn create_episode_minimal() {
        let json = r#"{"title": "Test", "content": "body"}"#;
        let parsed: CreateEpisode = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.title, "Test");
        assert!(parsed.kind.is_none());
        assert!(parsed.severity.is_none());
        assert!(parsed.resolved.is_none());
    }

    #[test]
    fn list_episodes_filter_defaults() {
        let filter = ListEpisodesFilter::default();
        assert!(filter.project.is_none());
        assert!(filter.kind.is_none());
        assert!(filter.resolved.is_none());
        assert!(filter.limit.is_none());
        assert!(filter.offset.is_none());
    }
}
