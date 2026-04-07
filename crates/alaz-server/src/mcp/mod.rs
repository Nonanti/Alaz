//! MCP (Model Context Protocol) server implementation using rmcp.
//!
//! Exposes all Alaz tools via the MCP protocol.
//! Tool logic lives in `handlers/` sub-modules; this file contains only
//! the rmcp-required struct, thin wrapper methods, and the ServerHandler impl.

use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ServerInfo;
use rmcp::tool;

mod handlers;
mod helpers;
mod params;
use params::*;

use crate::state::AppState;

/// The MCP server handler struct.
pub struct AlazMcpServer {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

impl AlazMcpServer {
    pub fn new(state: AppState) -> Self {
        let tool_router = Self::tool_router();
        Self { state, tool_router }
    }
}

// === Tool wrappers — each delegates to a handler in handlers/ ===

#[rmcp::tool_router]
impl AlazMcpServer {
    // --- Knowledge ---

    #[tool(
        description = "Save a knowledge item (code snippet, pattern, architecture decision, etc.) to Alaz knowledge base for cross-session reuse."
    )]
    async fn alaz_save(
        &self,
        Parameters(params): Parameters<SaveParams>,
    ) -> Result<String, String> {
        handlers::knowledge::save(&self.state, params).await
    }

    #[tool(description = "Get a specific knowledge item by ID from Alaz.")]
    async fn alaz_get(&self, Parameters(params): Parameters<GetParams>) -> Result<String, String> {
        handlers::knowledge::get(&self.state, params).await
    }

    #[tool(description = "Full-text search across all knowledge items in Alaz.")]
    async fn alaz_search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<String, String> {
        handlers::knowledge::search(&self.state, params).await
    }

    #[tool(
        description = "Hybrid search across knowledge, episodes, and procedures. Combines FTS + vector similarity + graph expansion via Reciprocal Rank Fusion for superior recall."
    )]
    async fn alaz_hybrid_search(
        &self,
        Parameters(params): Parameters<HybridSearchParams>,
    ) -> Result<String, String> {
        handlers::knowledge::hybrid_search(&self.state, params).await
    }

    #[tool(description = "List knowledge items from Alaz with optional filters.")]
    async fn alaz_list(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<String, String> {
        handlers::knowledge::list(&self.state, params).await
    }

    #[tool(description = "Update an existing knowledge item in Alaz.")]
    async fn alaz_update(
        &self,
        Parameters(params): Parameters<UpdateParams>,
    ) -> Result<String, String> {
        handlers::knowledge::update(&self.state, params).await
    }

    #[tool(description = "Delete a knowledge item from Alaz by ID.")]
    async fn alaz_delete(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<String, String> {
        handlers::knowledge::delete(&self.state, params).await
    }

    #[tool(description = "Find entities similar to a given entity using vector embeddings.")]
    async fn alaz_similar(
        &self,
        Parameters(params): Parameters<SimilarParams>,
    ) -> Result<String, String> {
        handlers::knowledge::similar(&self.state, params).await
    }

    // --- Graph ---

    #[tool(
        description = "Create a directed relation between two knowledge items (source -> target)."
    )]
    async fn alaz_relate(
        &self,
        Parameters(params): Parameters<RelateParams>,
    ) -> Result<String, String> {
        handlers::graph::relate(&self.state, params).await
    }

    #[tool(description = "Remove a relation between knowledge items by relation ID.")]
    async fn alaz_unrelate(
        &self,
        Parameters(params): Parameters<UnrelateParams>,
    ) -> Result<String, String> {
        handlers::graph::unrelate(&self.state, params).await
    }

    #[tool(
        description = "Multi-hop scored traversal across all entity types in the knowledge graph."
    )]
    async fn alaz_graph_explore(
        &self,
        Parameters(params): Parameters<GraphExploreInput>,
    ) -> Result<String, String> {
        handlers::graph::graph_explore(&self.state, params).await
    }

    #[tool(
        description = "Get relations for a knowledge item. Supports direction filtering, type filtering, and transitive traversal with depth."
    )]
    async fn alaz_relations(
        &self,
        Parameters(params): Parameters<RelationsParams>,
    ) -> Result<String, String> {
        handlers::graph::relations(&self.state, params).await
    }

    // --- Episodic ---

    #[tool(description = "List/search episodes (notable events from sessions).")]
    async fn alaz_episodes(
        &self,
        Parameters(params): Parameters<EpisodesParams>,
    ) -> Result<String, String> {
        handlers::episodic::episodes(&self.state, params).await
    }

    #[tool(
        description = "Search episodes using 5W cues (who, what, where, when, why) for episodic memory retrieval."
    )]
    async fn alaz_cue_search(
        &self,
        Parameters(params): Parameters<CueSearchParams>,
    ) -> Result<String, String> {
        handlers::episodic::cue_search(&self.state, params).await
    }

    #[tool(
        description = "Follow the causal chain from an episode. Shows what this episode led to (forward) or what caused it (backward)."
    )]
    async fn alaz_episode_chain(
        &self,
        Parameters(params): Parameters<EpisodeChainParams>,
    ) -> Result<String, String> {
        handlers::episodic::episode_chain(&self.state, params).await
    }

    #[tool(description = "Link two episodes in a causal chain (source led_to target).")]
    async fn alaz_episode_link(
        &self,
        Parameters(params): Parameters<EpisodeLinkParams>,
    ) -> Result<String, String> {
        handlers::episodic::episode_link(&self.state, params).await
    }

    #[tool(
        description = "View a chronological timeline of episodes and sessions within a date range. Answers questions like 'What did we do last week?'"
    )]
    async fn alaz_timeline(
        &self,
        Parameters(params): Parameters<TimelineParams>,
    ) -> Result<String, String> {
        handlers::episodic::timeline(&self.state, params).await
    }

    #[tool(
        description = "Create a reflection with granular scoring. Used for session analysis, periodic reviews, or prompted reflections."
    )]
    async fn alaz_create_reflection(
        &self,
        Parameters(params): Parameters<CreateReflectionParams>,
    ) -> Result<String, String> {
        handlers::episodic::create_reflection(&self.state, params).await
    }

    #[tool(
        description = "List/search reflections with optional filters by kind, session, and project."
    )]
    async fn alaz_reflections(
        &self,
        Parameters(params): Parameters<ReflectionsParams>,
    ) -> Result<String, String> {
        handlers::episodic::reflections(&self.state, params).await
    }

    #[tool(
        description = "Get reflection score trends over time. Shows daily averages of overall, knowledge, decision, and efficiency scores."
    )]
    async fn alaz_reflection_insights(
        &self,
        Parameters(params): Parameters<ReflectionInsightsParams>,
    ) -> Result<String, String> {
        handlers::episodic::reflection_insights(&self.state, params).await
    }

    // --- Session ---

    #[tool(description = "Query Claude Code session history from Alaz.")]
    async fn alaz_sessions(
        &self,
        Parameters(params): Parameters<SessionsParams>,
    ) -> Result<String, String> {
        handlers::session::sessions(&self.state, params).await
    }

    #[tool(description = "Save a session checkpoint with current state data.")]
    async fn alaz_checkpoint_save(
        &self,
        Parameters(params): Parameters<CheckpointSaveParams>,
    ) -> Result<String, String> {
        handlers::session::checkpoint_save(&self.state, params).await
    }

    #[tool(description = "Get all checkpoints for a session.")]
    async fn alaz_checkpoint_list(
        &self,
        Parameters(params): Parameters<CheckpointGetParams>,
    ) -> Result<String, String> {
        handlers::session::checkpoint_list(&self.state, params).await
    }

    #[tool(description = "Restore the latest checkpoint for a session, returning its data.")]
    async fn alaz_checkpoint_restore(
        &self,
        Parameters(params): Parameters<CheckpointRestoreParams>,
    ) -> Result<String, String> {
        handlers::session::checkpoint_restore(&self.state, params).await
    }

    #[tool(
        description = "Build a compact restore context for resuming work in a new session. Collects checkpoints, episodes, knowledge, and core memories into a formatted digest."
    )]
    async fn alaz_compact_restore(
        &self,
        Parameters(params): Parameters<CompactRestoreParams>,
    ) -> Result<String, String> {
        handlers::session::compact_restore(&self.state, params).await
    }

    // --- Search / Context ---

    #[tool(description = "Cross-project search to find knowledge from other projects.")]
    async fn alaz_cross_project(
        &self,
        Parameters(params): Parameters<CrossProjectParams>,
    ) -> Result<String, String> {
        handlers::search::cross_project(&self.state, params).await
    }

    #[tool(
        description = "Record a click (implicit feedback) on a search result. Call this when a user views or uses an entity from search results to improve future ranking."
    )]
    async fn alaz_search_feedback(
        &self,
        Parameters(params): Parameters<SearchFeedbackParams>,
    ) -> Result<String, String> {
        handlers::search::search_feedback(&self.state, params).await
    }

    #[tool(
        description = "Explain why a search returned specific results. Shows per-signal score contributions (FTS, dense, ColBERT, graph, RAPTOR, cue), decay impact, and feedback boost for each result."
    )]
    async fn alaz_explain(
        &self,
        Parameters(params): Parameters<ExplainParams>,
    ) -> Result<String, String> {
        handlers::search::explain(&self.state, params).await
    }

    #[tool(
        description = "Check context window budget. Returns token usage estimates, warning levels, and suggested actions for managing conversation length."
    )]
    async fn alaz_context_budget(
        &self,
        Parameters(params): Parameters<ContextBudgetParams>,
    ) -> Result<String, String> {
        handlers::search::context_budget(&self.state, params).await
    }

    #[tool(
        description = "Optimize text for context window efficiency. Applies whitespace cleanup, optional LLM summarization, and truncation to fit within token budget."
    )]
    async fn alaz_optimize_context(
        &self,
        Parameters(params): Parameters<OptimizeContextParams>,
    ) -> Result<String, String> {
        handlers::search::optimize_context(&self.state, params).await
    }

    // --- Vault ---

    #[tool(description = "Store an encrypted secret in the vault (name + value).")]
    async fn alaz_vault_store(
        &self,
        Parameters(params): Parameters<VaultStoreParams>,
    ) -> Result<String, String> {
        handlers::vault::store(&self.state, params).await
    }

    #[tool(description = "Retrieve and decrypt a secret from the vault by name.")]
    async fn alaz_vault_get(
        &self,
        Parameters(params): Parameters<VaultGetParams>,
    ) -> Result<String, String> {
        handlers::vault::get(&self.state, params).await
    }

    #[tool(description = "List all secret names in the vault (no values returned).")]
    async fn alaz_vault_list(
        &self,
        Parameters(_params): Parameters<VaultListParams>,
    ) -> Result<String, String> {
        handlers::vault::list(&self.state, _params).await
    }

    #[tool(description = "Delete a secret from the vault by name.")]
    async fn alaz_vault_delete(
        &self,
        Parameters(params): Parameters<VaultDeleteParams>,
    ) -> Result<String, String> {
        handlers::vault::delete(&self.state, params).await
    }

    // --- System ---

    #[tool(
        description = "Get a health report for the knowledge base. Shows per-topic freshness, confidence, stale items, and detected knowledge gaps."
    )]
    async fn alaz_health(
        &self,
        Parameters(params): Parameters<HealthParams>,
    ) -> Result<String, String> {
        handlers::system::health(&self.state, params).await
    }

    #[tool(
        description = "Analyze the impact of changing a code symbol. Shows where it's defined, who calls it, and what would break if it changed. Requires prior code indexing via POST /api/v1/code/index."
    )]
    async fn alaz_impact(
        &self,
        Parameters(params): Parameters<ImpactParams>,
    ) -> Result<String, String> {
        handlers::system::impact(&self.state, params).await
    }

    #[tool(
        description = "Trace the evolution history of a knowledge item through its supersede chain. Shows all versions from oldest to newest with reasons for each change."
    )]
    async fn alaz_evolution(
        &self,
        Parameters(params): Parameters<EvolutionParams>,
    ) -> Result<String, String> {
        handlers::system::evolution(&self.state, params).await
    }

    #[tool(
        description = "Record a spaced repetition review for a knowledge item. Quality: 0=blackout, 1=incorrect, 2=difficult, 3=correct, 4=hesitation, 5=perfect. Updates the review schedule automatically."
    )]
    async fn alaz_review(
        &self,
        Parameters(params): Parameters<ReviewParams>,
    ) -> Result<String, String> {
        handlers::system::review(&self.state, params).await
    }

    #[tool(
        description = "List knowledge items due for spaced repetition review. These are items you should revisit to keep them fresh in memory."
    )]
    async fn alaz_review_list(
        &self,
        Parameters(params): Parameters<ReviewListParams>,
    ) -> Result<String, String> {
        handlers::system::review_list(&self.state, params).await
    }

    #[tool(
        description = "Supersede an entity (knowledge item, episode, or procedure) with a newer version. The old entity is marked as superseded and excluded from search results."
    )]
    async fn alaz_supersede(
        &self,
        Parameters(params): Parameters<SupersedeParams>,
    ) -> Result<String, String> {
        handlers::system::supersede(&self.state, params).await
    }

    #[tool(
        description = "Record the outcome of a procedure execution. Call this when a known procedure was followed and you know whether it succeeded or failed. This feeds the Wilson score confidence metric — procedures with enough successful outcomes become 'proven' and get surfaced in context injection."
    )]
    async fn alaz_procedure_outcome(
        &self,
        Parameters(params): Parameters<ProcedureOutcomeParams>,
    ) -> Result<String, String> {
        handlers::system::procedure_outcome(&self.state, params).await
    }

    #[tool(
        description = "Ingest any content into Alaz knowledge base. Accepts notes, web clips, voice memos, photos, or any text. Automatically detects content domain and extracts structured knowledge."
    )]
    async fn alaz_ingest(
        &self,
        Parameters(params): Parameters<IngestParams>,
    ) -> Result<String, String> {
        handlers::system::ingest(&self.state, params).await
    }

    #[tool(
        description = "Rebuild RAPTOR hierarchical clustering tree for improved conceptual search."
    )]
    async fn alaz_raptor_rebuild(
        &self,
        Parameters(params): Parameters<RaptorRebuildInput>,
    ) -> Result<String, String> {
        handlers::system::raptor_rebuild(&self.state, params).await
    }

    #[tool(
        description = "Check the status of the RAPTOR hierarchical clustering tree for a project."
    )]
    async fn alaz_raptor_status(
        &self,
        Parameters(params): Parameters<RaptorStatusInput>,
    ) -> Result<String, String> {
        handlers::system::raptor_status(&self.state, params).await
    }

    #[tool(description = "List procedures with success rates.")]
    async fn alaz_procedures(
        &self,
        Parameters(params): Parameters<ProceduresParams>,
    ) -> Result<String, String> {
        handlers::system::procedures(&self.state, params).await
    }

    #[tool(description = "Read/write persistent facts, preferences, conventions, and constraints.")]
    async fn alaz_core_memory(
        &self,
        Parameters(params): Parameters<CoreMemoryParams>,
    ) -> Result<String, String> {
        handlers::system::core_memory(&self.state, params).await
    }
}

#[rmcp::tool_handler]
impl ServerHandler for AlazMcpServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build();
        ServerInfo::new(capabilities)
            .with_instructions("Alaz -- AI Knowledge System. Provides tools for managing knowledge, episodes, procedures, core memories, search, graph exploration, context management, and universal content ingestion.")
    }
}
