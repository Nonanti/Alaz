use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Embeddable;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionKind {
    SessionEnd,
    Periodic,
    OnError,
    Prompted,
    Consolidation,
}

impl ReflectionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionEnd => "session_end",
            Self::Periodic => "periodic",
            Self::OnError => "on_error",
            Self::Prompted => "prompted",
            Self::Consolidation => "consolidation",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "periodic" => Self::Periodic,
            "on_error" => Self::OnError,
            "prompted" => Self::Prompted,
            "consolidation" => Self::Consolidation,
            _ => Self::SessionEnd,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionItem {
    pub description: String,
    pub status: String,   // "pending", "done", "skipped"
    pub priority: String, // "low", "medium", "high"
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Reflection {
    pub id: String,
    pub session_id: String,
    pub what_worked: Option<String>,
    pub what_failed: Option<String>,
    pub lessons_learned: Option<String>,
    pub effectiveness_score: Option<f64>,
    pub complexity_score: Option<f64>,
    pub project_id: Option<String>,
    pub kind: String,
    pub action_items: serde_json::Value,
    pub overall_score: Option<f64>,
    pub knowledge_score: Option<f64>,
    pub decision_score: Option<f64>,
    pub efficiency_score: Option<f64>,
    pub evaluated_episode_ids: Vec<String>,
    pub needs_embedding: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReflection {
    pub session_id: String,
    pub what_worked: Option<String>,
    pub what_failed: Option<String>,
    pub lessons_learned: Option<String>,
    pub effectiveness_score: Option<f64>,
    pub complexity_score: Option<f64>,
    pub kind: Option<String>,
    pub action_items: Option<Vec<ActionItem>>,
    pub overall_score: Option<f64>,
    pub knowledge_score: Option<f64>,
    pub decision_score: Option<f64>,
    pub efficiency_score: Option<f64>,
    pub evaluated_episode_ids: Option<Vec<String>>,
    pub project: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListReflectionsFilter {
    pub project: Option<String>,
    pub kind: Option<String>,
    pub session_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Embeddable for Reflection {
    fn entity_type_name(&self) -> &'static str {
        "reflection"
    }
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }
    fn embed_content(&self) -> String {
        format!(
            "{}\n{}\n{}",
            self.what_worked.as_deref().unwrap_or(""),
            self.what_failed.as_deref().unwrap_or(""),
            self.lessons_learned.as_deref().unwrap_or("")
        )
    }
    fn needs_colbert(&self) -> bool {
        false
    }
}
