use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Learned signal weights for a specific query type.
///
/// Stored in the `signal_weights` table and updated weekly by the
/// weight learning job based on click-through data.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SignalWeight {
    pub id: String,
    pub query_type: String,
    pub fts: f32,
    pub dense: f32,
    pub raptor: f32,
    pub graph: f32,
    pub cue: f32,
    pub sample_size: i32,
    pub created_at: DateTime<Utc>,
}
