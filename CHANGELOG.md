# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0] — Active Intelligence

Major release: Alaz graduates from a passive knowledge store to an active collaborator that observes, learns, and proactively surfaces the right context.

### Added
- **CLAUDE.md generator** — `alaz generate-claude-md` emits a project-specific guide synthesized from the knowledge base; auto-refreshes on session start when stale (>24h).
- **Smart PostToolUse hook** — triggers on errors and file events with selective FTS; minimises noise while maximising signal.
- **Dual LLM backend** — Ollama native API (`think:false` fast path) with OpenAI-compatible fallback. Run fully local, fully remote, or mix and match.
- **Git integration** — post-commit hook ingests diffs; new MCP tools `alaz_git_hot_files`, `alaz_git_coupled_files`, `alaz_git_timeline`.
- **v2 MCP surface** — 76 tools total. New capabilities:
  - Alerts (`alaz_create_alert`, `alaz_list_alerts`, `alaz_alert_history`, `alaz_delete_alert`)
  - Error groups (`alaz_error_groups`, `alaz_error_group_detail`, `alaz_resolve_error`)
  - Logs (`alaz_logs_query`, `alaz_logs_stats`)
  - Work units (`alaz_create_work_unit`, `alaz_list_work_units`, `alaz_update_work_unit`, `alaz_link_session_work_unit`)
  - Project health (`alaz_project_health`), impact analysis (`alaz_impact`)
  - Advanced retrieval (`alaz_rag_fusion`, `alaz_agentic_search`)
  - Analytics (`alaz_learning_analytics`, `alaz_search_analytics`, `alaz_record_usage`)
- **Spaced repetition** — Migration 016 adds `sr_interval_days`, `sr_easiness`, `sr_next_review`, `sr_repetitions` to `knowledge_items` (SM-2 style).
- **Learning analytics** — Migration 018 exposes pipeline health and extraction outcomes.
- **Related files extraction** — learning pipeline now captures co-changed files per episode.
- **Implicit click tracking** — signal-weight learning consumes real usage, no manual feedback required.

### Changed
- **Lower cross-project promotion thresholds** — patterns cross over to other projects faster.
- **Hard-delete superseded items** after a 7-day grace period instead of indefinite retention.
- **Tighter extraction limits + minimum quality gates** for higher-signal outputs.
- **ColBERT & MCP refactor** — MCP module split from a 1455-line monolith into per-domain handler sub-modules.
- **FTS tokenization** — OR-keyword expansion in PostToolUse hook for broader recall.
- **LLM client** — flexible JSON deserialization, reflection foreign-key guard.

### Fixed
- Project name → ID resolution before repo queries.
- PostgreSQL `AVG()` NUMERIC cast to `float8` for sqlx `f64` compatibility.
- 21+ individual issues and 5 architectural improvements across the codebase.

### Removed
- Internal deploy-specific references and migration artifacts from public history.

## [1.0.0] — Initial public release

- Single Rust binary, 9 crates.
- 6-signal hybrid search: FTS + Dense Vector + ColBERT MaxSim + Graph Expansion + RAPTOR + Memory Decay, fused via Reciprocal Rank Fusion.
- Autonomous learning pipeline: patterns, episodes, procedures, core memories, reflections.
- Knowledge graph with 14+ relation types, causal chains, cross-project promotion.
- Encrypted vault (AES-256-GCM), JWT + API key auth, Argon2id password hashing.
- 22 MCP tools over StreamableHTTP.

[2.0.0]: https://github.com/Nonanti/Alaz/releases/tag/v2.0.0
[1.0.0]: https://github.com/Nonanti/Alaz/releases/tag/v1.0.0
