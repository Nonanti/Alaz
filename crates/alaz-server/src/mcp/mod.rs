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

    // --- Database Intelligence ---

    #[tool(
        description = "Execute a read-only SQL query against the Alaz database. Only SELECT/WITH statements allowed. Returns results as a markdown table. Use for debugging, data exploration, and ad-hoc analysis."
    )]
    async fn alaz_db_query(
        &self,
        Parameters(params): Parameters<DbQueryParams>,
    ) -> Result<String, String> {
        handlers::database::db_query(&self.state, params).await
    }

    #[tool(
        description = "Explore the Alaz database schema. Actions: 'tables' (list all), 'describe' (columns of a table), 'indexes' (indexes on a table), 'fk' (foreign keys)."
    )]
    async fn alaz_db_schema(
        &self,
        Parameters(params): Parameters<DbSchemaParams>,
    ) -> Result<String, String> {
        handlers::database::db_schema(&self.state, params).await
    }

    // --- Observability ---

    #[tool(
        description = "Get real-time system metrics: search count/latency, LLM calls/errors, embedding count, backfill processed, decay pruned, consolidation merged."
    )]
    async fn alaz_system_metrics(
        &self,
        Parameters(_params): Parameters<SystemMetricsParams>,
    ) -> Result<String, String> {
        handlers::system::metrics(&self.state).await
    }

    #[tool(
        description = "View learning pipeline analytics: recent learning runs, extraction counts, duration, success rates."
    )]
    async fn alaz_learning_analytics(
        &self,
        Parameters(params): Parameters<LearningAnalyticsParams>,
    ) -> Result<String, String> {
        handlers::system::learning_analytics(&self.state, params).await
    }

    #[tool(
        description = "View search analytics: query distribution, click-through rates, signal effectiveness over recent days."
    )]
    async fn alaz_search_analytics(
        &self,
        Parameters(params): Parameters<SearchAnalyticsParams>,
    ) -> Result<String, String> {
        handlers::system::search_analytics(&self.state, params).await
    }

    // --- Session Search ---

    #[tool(
        description = "Full-text search across past session transcripts. Find conversations about specific topics, decisions, or file paths from previous sessions."
    )]
    async fn alaz_session_search(
        &self,
        Parameters(params): Parameters<SessionSearchParams>,
    ) -> Result<String, String> {
        handlers::session::search_transcripts(&self.state, params).await
    }

    // --- Pattern Usage ---

    #[tool(
        description = "Record explicit usage of a knowledge item with outcome tracking. Call this when you used a pattern/snippet and know if it worked. Outcomes: 'success', 'failure', 'partial'. Feeds the Wilson score confidence metric."
    )]
    async fn alaz_record_usage(
        &self,
        Parameters(params): Parameters<RecordUsageParams>,
    ) -> Result<String, String> {
        handlers::knowledge::record_usage(&self.state, params).await
    }

    // --- Advanced Search ---

    #[tool(
        description = "Agentic multi-hop search with iterative query refinement. For complex questions that need multiple rounds of search (e.g., 'What patterns did we use when fixing the auth issue last week?'). Uses LLM to analyze intermediate results and refine the query up to 3 times."
    )]
    async fn alaz_agentic_search(
        &self,
        Parameters(params): Parameters<AgenticSearchParams>,
    ) -> Result<String, String> {
        handlers::search::agentic_search(&self.state, params).await
    }

    #[tool(
        description = "RAG fusion search: expands query into 3-4 alternative phrasings, searches each independently, and fuses results via RRF for superior recall. Best for ambiguous or broad queries."
    )]
    async fn alaz_rag_fusion(
        &self,
        Parameters(params): Parameters<RagFusionSearchParams>,
    ) -> Result<String, String> {
        handlers::search::rag_fusion(&self.state, params).await
    }

    // --- Session State ---

    #[tool(
        description = "Update structured session state: goals, accomplished items, pending tasks, handoff summary. Call this periodically during sessions to maintain continuity."
    )]
    async fn alaz_update_session_state(
        &self,
        Parameters(params): Parameters<UpdateSessionStateParams>,
    ) -> Result<String, String> {
        handlers::session::update_session_state(&self.state, params).await
    }

    #[tool(
        description = "Get the structured state of a session: goals, accomplished, pending, current task, handoff summary."
    )]
    async fn alaz_get_session_state(
        &self,
        Parameters(params): Parameters<GetSessionStateParams>,
    ) -> Result<String, String> {
        handlers::session::get_session_state(&self.state, params).await
    }

    // --- Work Units ---

    #[tool(
        description = "Create a work unit — a task that spans multiple sessions (e.g., 'Implement auth system'). Sessions can be linked to track cross-session progress."
    )]
    async fn alaz_create_work_unit(
        &self,
        Parameters(params): Parameters<CreateWorkUnitParams>,
    ) -> Result<String, String> {
        handlers::session::create_work_unit(&self.state, params).await
    }

    #[tool(
        description = "List work units for a project, optionally filtered by status (active/completed/paused)."
    )]
    async fn alaz_list_work_units(
        &self,
        Parameters(params): Parameters<ListWorkUnitsParams>,
    ) -> Result<String, String> {
        handlers::session::list_work_units(&self.state, params).await
    }

    #[tool(description = "Update work unit status: active, completed, paused, cancelled.")]
    async fn alaz_update_work_unit(
        &self,
        Parameters(params): Parameters<UpdateWorkUnitParams>,
    ) -> Result<String, String> {
        handlers::session::update_work_unit(&self.state, params).await
    }

    #[tool(description = "Link the current session to a work unit for cross-session tracking.")]
    async fn alaz_link_session_work_unit(
        &self,
        Parameters(params): Parameters<LinkSessionWorkUnitParams>,
    ) -> Result<String, String> {
        handlers::session::link_session_work_unit(&self.state, params).await
    }

    // --- Project Health ---

    #[tool(
        description = "Comprehensive project health check across 6 dimensions: knowledge freshness, procedure health, episode coverage, core memory completeness, search effectiveness, learning pipeline health. Returns overall score 0-100% with per-dimension breakdown and recommendations."
    )]
    async fn alaz_project_health(
        &self,
        Parameters(params): Parameters<ProjectHealthParams>,
    ) -> Result<String, String> {
        handlers::system::project_health(&self.state, params).await
    }

    // --- Session Messages ---

    #[tool(
        description = "Full-text search across individual session messages. More granular than session_search — finds specific messages, tool calls, or decisions within sessions."
    )]
    async fn alaz_search_messages(
        &self,
        Parameters(params): Parameters<SearchMessagesParams>,
    ) -> Result<String, String> {
        handlers::session::search_messages(&self.state, params).await
    }

    #[tool(
        description = "Get messages from a specific session, optionally filtered by role (user/assistant). For reviewing what happened in a session."
    )]
    async fn alaz_get_messages(
        &self,
        Parameters(params): Parameters<GetMessagesParams>,
    ) -> Result<String, String> {
        handlers::session::get_messages(&self.state, params).await
    }

    // --- Observability ---

    #[tool(
        description = "Query structured application logs from the database. Filter by level, target module, search text, or time window. Use for debugging and incident investigation."
    )]
    async fn alaz_logs_query(
        &self,
        Parameters(params): Parameters<LogsQueryParams>,
    ) -> Result<String, String> {
        handlers::observability::logs_query(&self.state, params).await
    }

    #[tool(
        description = "Get log statistics by level for a time window. Shows count of trace/debug/info/warn/error logs."
    )]
    async fn alaz_logs_stats(
        &self,
        Parameters(params): Parameters<LogStatsParams>,
    ) -> Result<String, String> {
        handlers::observability::logs_stats(&self.state, params).await
    }

    #[tool(
        description = "List error groups (Sentry-style aggregation). Shows unique errors grouped by fingerprint with event counts."
    )]
    async fn alaz_error_groups(
        &self,
        Parameters(params): Parameters<ErrorGroupsParams>,
    ) -> Result<String, String> {
        handlers::observability::error_groups_list(&self.state, params).await
    }

    #[tool(
        description = "Get detailed information about a specific error group: fingerprint, first/last seen, event count, and resolution status."
    )]
    async fn alaz_error_group_detail(
        &self,
        Parameters(params): Parameters<ErrorGroupDetailParams>,
    ) -> Result<String, String> {
        handlers::observability::error_group_detail(&self.state, params).await
    }

    #[tool(
        description = "Mark an error group as resolved with optional notes. Use after fixing the root cause of an error."
    )]
    async fn alaz_resolve_error(
        &self,
        Parameters(params): Parameters<ResolveErrorGroupParams>,
    ) -> Result<String, String> {
        handlers::observability::resolve_error_group(&self.state, params).await
    }

    #[tool(
        description = "Create an alert rule that triggers when log patterns exceed thresholds. Condition types: error_rate, log_level_count, specific_target."
    )]
    async fn alaz_create_alert(
        &self,
        Parameters(params): Parameters<CreateAlertRuleParams>,
    ) -> Result<String, String> {
        handlers::observability::create_alert_rule(&self.state, params).await
    }

    #[tool(
        description = "List all configured alert rules with their status, trigger counts, and last fire times."
    )]
    async fn alaz_list_alerts(
        &self,
        Parameters(params): Parameters<ListAlertRulesParams>,
    ) -> Result<String, String> {
        handlers::observability::list_alert_rules(&self.state, params).await
    }

    #[tool(description = "Delete an alert rule by ID.")]
    async fn alaz_delete_alert(
        &self,
        Parameters(params): Parameters<DeleteAlertRuleParams>,
    ) -> Result<String, String> {
        handlers::observability::delete_alert_rule(&self.state, params).await
    }

    #[tool(
        description = "View recent alert trigger history. Shows when alerts fired and the matched counts."
    )]
    async fn alaz_alert_history(
        &self,
        Parameters(params): Parameters<AlertHistoryParams>,
    ) -> Result<String, String> {
        handlers::observability::alert_history(&self.state, params).await
    }

    // --- Git Timeline ---

    #[tool(
        description = "View recent git commits as a timeline. Shows commits ingested from hook stop — what actually happened in the codebase, with author, files, and stats."
    )]
    async fn alaz_git_timeline(
        &self,
        Parameters(params): Parameters<GitTimelineParams>,
    ) -> Result<String, String> {
        handlers::git::git_timeline(&self.state, params).await
    }

    #[tool(
        description = "List the most frequently changed files (hot files) from git activity. Identifies churn hotspots."
    )]
    async fn alaz_git_hot_files(
        &self,
        Parameters(params): Parameters<GitHotFilesParams>,
    ) -> Result<String, String> {
        handlers::git::git_hot_files(&self.state, params).await
    }

    #[tool(
        description = "Find files that tend to change together (temporal coupling). Detects hidden dependencies between files."
    )]
    async fn alaz_git_coupled_files(
        &self,
        Parameters(params): Parameters<GitCoupledFilesParams>,
    ) -> Result<String, String> {
        handlers::git::git_coupled_files(&self.state, params).await
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
