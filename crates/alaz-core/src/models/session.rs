use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SessionLog {
    pub id: String,
    pub project_id: Option<String>,
    pub cost: Option<f64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub duration_seconds: Option<f64>,
    pub tools_used: serde_json::Value,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SessionCheckpoint {
    pub id: String,
    pub session_id: String,
    pub checkpoint_data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListSessionsFilter {
    pub project: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
