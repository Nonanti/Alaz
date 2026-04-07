use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RaptorTree {
    pub id: String,
    pub project_id: Option<String>,
    pub status: String,
    pub total_nodes: i64,
    pub max_depth: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RaptorNode {
    pub id: String,
    pub tree_id: String,
    pub level: i32,
    pub parent_id: Option<String>,
    pub entity_type: String,
    pub entity_id: String,
    pub summary: Option<String>,
    pub children_count: i32,
    pub created_at: DateTime<Utc>,
}
