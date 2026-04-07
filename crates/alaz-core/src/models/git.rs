use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single file change from a git commit.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GitActivity {
    pub id: String,
    pub project_id: Option<String>,
    pub commit_hash: String,
    pub commit_message: String,
    pub file_path: String,
    pub change_type: String,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub created_at: DateTime<Utc>,
}

/// A frequently changed file with aggregated metrics.
#[derive(Debug, Clone, Serialize)]
pub struct HotFile {
    pub file_path: String,
    pub commit_count: i64,
    pub total_lines_added: i64,
    pub total_lines_removed: i64,
    pub total_churn: i64,
}

/// Two files that frequently change together.
#[derive(Debug, Clone, Serialize)]
pub struct CoupledFiles {
    pub file_a: String,
    pub file_b: String,
    /// How many commits both files appeared in together.
    pub co_change_count: i64,
    /// Percentage of file_a's commits that also include file_b.
    pub coupling_ratio: f64,
}
