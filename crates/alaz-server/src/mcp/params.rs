use rmcp::schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveParams {
    /// Short descriptive title
    pub title: String,
    /// The actual content (code, text, etc.)
    pub content: String,
    /// Optional longer description
    pub description: Option<String>,
    /// Type of knowledge: "artifact" or "pattern"
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// Programming language (e.g. typescript, rust)
    pub language: Option<String>,
    /// Original file path for reference
    pub file_path: Option<String>,
    /// Project name to associate with
    pub project: Option<String>,
    /// Tags for categorization
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetParams {
    /// Knowledge item ID
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Search query (supports FTS syntax)
    pub query: String,
    /// Filter by project name
    pub project: Option<String>,
    /// Max results
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HybridSearchParams {
    /// Search query
    pub query: String,
    /// Filter by project name
    pub project: Option<String>,
    /// Max results
    pub limit: Option<usize>,
    /// Apply LLM reranking
    pub rerank: Option<bool>,
    /// Use HyDE for improved recall
    pub hyde: Option<bool>,
    /// Include graph expansion signal
    pub graph_expand: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListParams {
    /// Filter by type
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// Filter by language
    pub language: Option<String>,
    /// Filter by project name
    pub project: Option<String>,
    /// Filter by tag
    pub tag: Option<String>,
    /// Max results
    pub limit: Option<i64>,
    /// Offset for pagination
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateParams {
    /// Knowledge item ID
    pub id: String,
    /// New title
    pub title: Option<String>,
    /// New content
    pub content: Option<String>,
    /// New description
    pub description: Option<String>,
    /// New language
    pub language: Option<String>,
    /// New file path
    pub file_path: Option<String>,
    /// Replace tags
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteParams {
    /// The item ID
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RelateParams {
    /// Source knowledge item ID
    pub source_id: String,
    /// Target knowledge item ID
    pub target_id: String,
    /// Relation type
    pub relation: String,
    /// Optional description
    pub description: Option<String>,
    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UnrelateParams {
    /// Edge ID to remove
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GraphExploreInput {
    /// Entity type to start from
    pub entity_type: String,
    /// Entity ID to start from
    pub entity_id: String,
    /// Traversal depth (default 1)
    pub depth: Option<u32>,
    /// Minimum edge weight
    pub min_weight: Option<f64>,
    /// Filter by relation type
    pub relation_filter: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EpisodesParams {
    /// Filter by type (error|decision|success|discovery)
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// Filter by project name
    pub project: Option<String>,
    /// Filter by resolved status
    pub resolved: Option<bool>,
    /// Max results
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProceduresParams {
    /// Filter by project name
    pub project: Option<String>,
    /// Filter by tag
    pub tag: Option<String>,
    /// Max results
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CoreMemoryParams {
    /// Filter by category (preference|fact|convention|constraint)
    pub category: Option<String>,
    /// Filter by project name
    pub project: Option<String>,
    /// Max results (default 50)
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RaptorRebuildInput {
    /// Filter by project name
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RaptorStatusInput {
    /// Filter by project name
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionsParams {
    /// Filter by project name
    pub project: Option<String>,
    /// Filter by session status
    pub status: Option<String>,
    /// Max results (default 20)
    pub limit: Option<i64>,
    /// Offset for pagination
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RelationsParams {
    /// Knowledge item ID to get relations for
    pub item_id: String,
    /// Filter by direction (outgoing|incoming|both)
    pub direction: Option<String>,
    /// Filter by relation type
    #[serde(rename = "type")]
    pub relation_type: Option<String>,
    /// Traversal depth
    pub depth: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SimilarParams {
    /// Entity type to find similar for
    pub entity_type: String,
    /// Entity ID to find similar for
    pub entity_id: String,
    /// Max results
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CueSearchParams {
    /// Who cues
    pub who: Option<Vec<String>>,
    /// What cues
    pub what: Option<Vec<String>>,
    /// Where cues
    pub where_: Option<Vec<String>>,
    /// When cues
    pub when: Option<Vec<String>>,
    /// Why cues
    pub why: Option<Vec<String>>,
    /// Filter by project name
    pub project: Option<String>,
    /// Max results
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckpointSaveParams {
    /// Session ID to save checkpoint for
    pub session_id: String,
    /// Checkpoint data (arbitrary JSON state)
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckpointGetParams {
    /// Session ID to get checkpoints for
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckpointRestoreParams {
    /// Session ID to restore latest checkpoint for
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EpisodeChainParams {
    /// Episode ID to start from
    pub episode_id: String,
    /// Direction: "forward" (what this led to) or "backward" (what caused this)
    pub direction: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EpisodeLinkParams {
    /// Source episode ID (the cause)
    pub source_id: String,
    /// Target episode ID (the effect)
    pub target_id: String,
    /// Relation type (default: "led_to")
    pub relation: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VaultStoreParams {
    /// Secret name (unique per owner)
    pub name: String,
    /// Secret value (will be encrypted)
    pub value: String,
    /// Optional description
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VaultGetParams {
    /// Secret name to retrieve
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VaultListParams {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VaultDeleteParams {
    /// Secret name to delete
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CrossProjectParams {
    /// Search query
    pub query: String,
    /// Exclude this project from results
    pub exclude_project: Option<String>,
    /// Max results
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContextBudgetParams {
    /// Current conversation length in characters
    pub current_length: Option<u64>,
    /// Context window size in tokens (default: 200000 for Opus)
    pub context_window: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OptimizeContextParams {
    /// The text to optimize
    pub text: String,
    /// Maximum tokens for output (default: 80000)
    pub max_tokens: Option<u64>,
    /// Whether to use LLM summarization (default: true)
    pub use_summarization: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompactRestoreParams {
    /// Session ID to restore context for
    pub session_id: String,
    /// Project name
    pub project: Option<String>,
    /// Maximum message limit (default: 50)
    pub message_limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TimelineParams {
    /// Start date (YYYY-MM-DD format)
    pub start_date: String,
    /// End date (YYYY-MM-DD format)
    pub end_date: String,
    /// Filter by project name
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateReflectionParams {
    /// Session ID this reflection belongs to
    pub session_id: String,
    /// What went well
    pub what_worked: Option<String>,
    /// What didn't work
    pub what_failed: Option<String>,
    /// Key takeaways
    pub lessons_learned: Option<String>,
    /// Reflection kind: session_end, periodic, on_error, prompted, consolidation
    pub kind: Option<String>,
    /// Overall quality score (0.0-1.0)
    pub overall_score: Option<f64>,
    /// Knowledge quality score (0.0-1.0)
    pub knowledge_score: Option<f64>,
    /// Decision quality score (0.0-1.0)
    pub decision_score: Option<f64>,
    /// Efficiency score (0.0-1.0)
    pub efficiency_score: Option<f64>,
    /// Action items as JSON array
    pub action_items: Option<serde_json::Value>,
    /// Episode IDs evaluated in this reflection
    pub evaluated_episode_ids: Option<Vec<String>>,
    /// Project name
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReflectionsParams {
    /// Filter by project name
    pub project: Option<String>,
    /// Filter by kind (session_end, periodic, etc.)
    pub kind: Option<String>,
    /// Filter by session ID
    pub session_id: Option<String>,
    /// Max results (default 20)
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReflectionInsightsParams {
    /// Filter by project name
    pub project: Option<String>,
    /// Number of days to analyze (default 30)
    pub days: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExplainParams {
    /// The search query to explain (must match a recent search exactly).
    pub query: String,
    /// Optional: specific entity ID to explain. If omitted, explains all results.
    pub entity_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImpactParams {
    /// Symbol name to analyze (e.g., "KnowledgeRepo", "find_similar_by_title").
    pub symbol_name: String,
    /// Project name (optional).
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HealthParams {
    /// Project name (optional, omit for global).
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvolutionParams {
    /// Knowledge item ID to trace evolution for.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReviewParams {
    /// Knowledge item ID.
    pub id: String,
    /// Review quality score (0-5): 0=blackout, 3=correct, 5=perfect.
    pub quality: i32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReviewListParams {
    /// Project name (optional).
    pub project: Option<String>,
    /// Max items to return (default 5).
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SupersedeParams {
    /// ID of the old (outdated) entity
    pub old_id: String,
    /// ID of the new (replacement) entity
    pub new_id: String,
    /// Reason for invalidation
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchFeedbackParams {
    /// Entity ID that was clicked/selected from search results
    pub entity_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProcedureOutcomeParams {
    /// Procedure ID
    pub id: String,
    /// Whether the procedure execution was successful
    pub success: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code)]
pub struct IngestParams {
    /// The content to ingest (text, markdown, notes, etc.)
    pub content: String,
    /// Source: "mobile_note", "web_clip", "voice_memo", "photo", "manual", etc.
    pub source: String,
    /// Content type: "text", "markdown", "transcript", "url", "image_description"
    pub content_type: Option<String>,
    /// Optional title for the content
    pub title: Option<String>,
    /// Project name to associate with
    pub project: Option<String>,
    /// Tags for categorization
    pub tags: Option<Vec<String>>,
    /// Source-specific metadata (url, device_id, etc.)
    pub metadata: Option<serde_json::Value>,
}
