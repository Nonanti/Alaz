# Alaz Architecture

> AI Knowledge System — A single Rust binary with 9 crates.

## Crate Dependency Graph

```
                        ┌───────────┐
                        │ alaz-core │  (models, traits, error types, helpers)
                        └─────┬─────┘
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        ┌──────────┐   ┌────────────┐   ┌────────────┐
        │ alaz-db  │   │ alaz-vector│   │ (external) │
        │(postgres)│   │  (qdrant)  │   │            │
        └────┬─────┘   └─────┬──────┘   └────────────┘
             │  ┌─────────────┘
             ▼  ▼
       ┌────────────┐
       │ alaz-graph │  (graph traversal, causal chains)
       └──────┬─────┘
              │
       ┌──────┴──────┐
       ▼             ▼
 ┌───────────┐ ┌────────────┐
 │ alaz-intel│ │ alaz-auth  │  (JWT, API keys, vault crypto)
 │ (LLM, RAP│ └──────┬─────┘
 │  TOR,     │        │
 │ learning) │        │
 └─────┬─────┘        │
       │               │
       ▼               ▼
 ┌────────────┐  ┌────────────┐
 │alaz-search │  │alaz-server │  (Axum HTTP, MCP, jobs, routing)
 │(pipeline,  │──│            │
 │ signals,   │  │            │
 │ reranking) │  └──────┬─────┘
 └────────────┘         │
                        ▼
                  ┌──────────┐
                  │ alaz-cli │  (binary entry point, CLI commands)
                  └──────────┘
```

### Build Order

```
alaz-core → alaz-db → alaz-vector → alaz-graph → alaz-intel → alaz-search → alaz-auth → alaz-server → alaz-cli
```

### Crate Responsibilities

| Crate | Role |
|-------|------|
| `alaz-core` | Shared models, traits (`SearchResult`, `SearchQuery`), error types (`AlazError`), helpers (`wilson_score_lower`, `estimate_tokens`, `CircuitBreaker`) |
| `alaz-db` | PostgreSQL repos (`KnowledgeRepo`, `EpisodeRepo`, `ProcedureRepo`, `CoreMemoryRepo`, `SessionRepo`, `ReflectionRepo`, `GraphRepo`, `VaultRepo`, etc.), migrations |
| `alaz-vector` | Qdrant client wrapper (`QdrantManager`), dense vector operations |
| `alaz-graph` | Multi-hop graph traversal (`explore`), causal chain following |
| `alaz-intel` | LLM client, session learning pipeline (`SessionLearner`), RAPTOR tree builder, HyDE generator, context optimizer, embedding service, ColBERT service |
| `alaz-search` | 6-signal hybrid search pipeline, weighted RRF fusion, 3-stage reranking (bi-encoder + cross-encoder + LLM), decay scoring, proactive search, query classification |
| `alaz-auth` | JWT token creation/verification, API key management with Argon2 hashing, AES-256-GCM vault encryption |
| `alaz-server` | Axum REST API, MCP (Model Context Protocol) server, background jobs, rate limiting, CORS, auth middleware |
| `alaz-cli` | Binary entry point with `clap` subcommands: `serve`, `migrate`, `learn`, `device`, `api-key` |

---

## Request Flow

```
Client (pi extension / REST / MCP)
  │
  ▼
┌─────────────────────────────────────────┐
│           HTTP / MCP Transport          │
│  (Axum router / rmcp StreamableHTTP)    │
├─────────────────────────────────────────┤
│           Rate Limiter                  │
│  (60 req/60s per IP, in-memory)         │
├─────────────────────────────────────────┤
│           Auth Middleware               │
│  REST: JWT Bearer OR X-API-Key          │
│  + X-Device-Fingerprint (optional)      │
│  MCP:  X-API-Key only                   │
├─────────────────────────────────────────┤
│           Audit Logging                 │
│  (async, non-blocking)                  │
├─────────────────────────────────────────┤
│           Handler                       │
│  (api::knowledge, api::search, etc.)    │
├─────────────────────────────────────────┤
│           Business Logic                │
│  (repos, search pipeline, learning)     │
├─────────────────────────────────────────┤
│           Data Layer                    │
│  PostgreSQL │ Qdrant │ Ollama/TEI       │
└─────────────────────────────────────────┘
```

---

## Search Pipeline: 6-Signal Hybrid Search

The search pipeline runs in 3 phases, fusing results via weighted Reciprocal Rank Fusion (RRF) with adaptive query-type weights.

```
                          Query
                            │
                  ┌─────────┴─────────┐
                  ▼                   ▼
            Query Classifier     Optional HyDE
            (factual/temporal    (hypothetical doc
             /navigational/       generation via LLM)
              exploratory)
                  │                   │
                  ▼                   ▼
           Adaptive Weights     Embed query text
                  │
    ┌─────────────┼─────────────┐
    │  Phase 1: Concurrent      │
    │  ┌─────┐ ┌──────┐ ┌─────┐│
    │  │ FTS │ │Dense │ │ColBE││
    │  │(PG) │ │(Qdra)│ │RT   ││
    │  └──┬──┘ └──┬───┘ └──┬──┘│
    └─────┼───────┼────────┼───┘
          │       │        │
          ▼       ▼        ▼
       Seed set (top 10 from FTS + dense)
          │
    ┌─────┼──────────────────────┐
    │  Phase 2: Concurrent       │
    │  ┌──────┐ ┌──────┐ ┌─────┐│
    │  │Graph │ │RAPTOR│ │ Cue ││
    │  │Expand│ │(hier)│ │Srch ││
    │  └──┬───┘ └──┬───┘ └──┬──┘│
    └─────┼────────┼────────┼───┘
          │        │        │
          ▼        ▼        ▼
    Weighted RRF Fusion (6 signals)
          │
          ▼
    Memory Decay Adjustment
    (score boosted/penalized by
     access recency & frequency)
          │
          ▼
    Optional 3-Stage Reranking
    ┌────────────────────────────┐
    │ Stage 1: Bi-encoder (Qdrant scores) │
    │ Stage 2: Cross-encoder (TEI)        │
    │ Stage 3: LLM judge (Ollama)         │
    │ Final = w_bi·S1 + w_cross·S2 + w_llm·S3 │
    └────────────────────────────┘
          │
          ▼
    Hydrated Results (full entity data from DB)
```

### Signal Details

| # | Signal | Source | Purpose |
|---|--------|--------|---------|
| 1 | **FTS** | PostgreSQL `tsvector` with `'simple'` dictionary | Exact keyword matching, language-agnostic |
| 2 | **Dense Text** | Qdrant collection `alaz_text` | Semantic similarity via TEI embeddings |
| 3 | **ColBERT** | Qdrant + ColBERT service | Token-level late interaction for precise matching |
| 4 | **Graph Expansion** | PostgreSQL `graph_edges` | Related entities via knowledge graph traversal |
| 5 | **RAPTOR** | Qdrant `alaz_text` (raptor_node entities) | Hierarchical conceptual search via clustered summaries |
| 6 | **Cue Search** | PostgreSQL array overlap (`&&`) | 5W episodic recall (who/what/where/when/why) |

---

## Background Jobs

Four periodic jobs run alongside the HTTP server:

| Job | Interval | Purpose |
|-----|----------|---------|
| **Embedding Backfill** | 5 minutes | Processes entities with `needs_embedding=true`, generates embeddings via TEI, upserts to Qdrant, marks as embedded. Batch size: 50. |
| **Graph Decay** | 6 hours | Applies exponential decay to `graph_edges.weight`. Removes edges below threshold. Half-life: 30 days. |
| **Memory Decay** | 6 hours | Decays `utility_score` for entities not accessed in 7 days. Boosts recently accessed items. Prunes items with utility below threshold (deletes from DB, Qdrant, and graph). |
| **Feedback Aggregation** | 12 hours | Aggregates search click-through rates from `search_queries` table and updates `feedback_boost` on entities. |

All jobs use graceful degradation: errors are logged but don't crash the server.

---

## Deployment Topology

```
┌──────────────────────────────────────────────────────┐
│                   Production Server                  │
│                 (your-server)                        │
│                                                      │
│  ┌────────────────┐  ┌─────────────────────────────┐ │
│  │  Alaz Binary   │  │       PostgreSQL             │ │
│  │  (Rust, :3456) │◀▶│  (:5437, alaz database)     │ │
│  │                │  │  Tables: knowledge_items,    │ │
│  │  REST API      │  │  episodes, procedures,      │ │
│  │  MCP Server    │  │  core_memories, session_logs,│ │
│  │  Background    │  │  graph_edges, vault_secrets, │ │
│  │  Jobs          │  │  search_queries, ...         │ │
│  └───────┬────────┘  └─────────────────────────────┘ │
│          │                                           │
│  ┌───────┴────────┐  ┌─────────────────────────────┐ │
│  │    Qdrant      │  │       Ollama                 │ │
│  │  (:6334)       │  │  (:11434)                    │ │
│  │                │  │  LLM for:                    │ │
│  │  Collections:  │  │  - Learning extraction       │ │
│  │  - alaz_text   │  │  - RAPTOR summarization      │ │
│  │  (dense vecs)  │  │  - HyDE generation           │ │
│  └────────────────┘  │  - LLM reranking             │ │
│                      └─────────────────────────────┘ │
│  ┌────────────────┐  ┌─────────────────────────────┐ │
│  │ TEI Embeddings │  │    ColBERT Service           │ │
│  │  (:8001)       │  │  (:8002)                     │ │
│  │  Dense vector  │  │  Token-level late            │ │
│  │  generation    │  │  interaction retrieval       │ │
│  └────────────────┘  └─────────────────────────────┘ │
└──────────────────────────────────────────────────────┘

         ▲
         │ REST/MCP (via pi extension)
         │
┌────────┴─────────┐
│  Developer       │
│  Machine         │
│  (pi + Claude)   │
└──────────────────┘
```

### Service Management

```bash
# Deploy
bash deploy.sh                                              # Build + rsync + restart

# Status
ssh your-user@your-server 'systemctl --user status alaz'

# Logs
ssh your-user@your-server 'journalctl --user -u alaz -f'
```

---

## Entity Types & Tables

### Primary Entities

| Entity | Table | Description | ID Gen |
|--------|-------|-------------|--------|
| Knowledge Item | `knowledge_items` | Code snippets, patterns, architecture decisions | CUID2 |
| Episode | `episodes` | Notable events: errors, decisions, discoveries, successes | CUID2 |
| Procedure | `procedures` | Step-by-step guides with Wilson score confidence | CUID2 |
| Core Memory | `core_memories` | Persistent facts, preferences, conventions, constraints | CUID2 |
| Session Log | `session_logs` | Claude Code session records with metrics | CUID2 |
| Reflection | `reflections` | Session analysis with granular scoring | CUID2 |

### Supporting Entities

| Entity | Table | Description |
|--------|-------|-------------|
| Graph Edge | `graph_edges` | Directed weighted relations between entities |
| RAPTOR Node | `raptor_nodes` | Hierarchical cluster summaries |
| RAPTOR Tree | `raptor_trees` | Tree metadata (level counts, timestamps) |
| Vault Secret | `vault_secrets` | AES-256-GCM encrypted key-value store |
| Owner | `owners` | Authentication principals |
| API Key | `api_keys` | Argon2-hashed API keys |
| Device | `devices` | Trusted device fingerprints |
| Audit Log | `audit_logs` | Request/event audit trail |
| Search Query | `search_queries` | Logged search queries for feedback loop |
| Session Checkpoint | `session_checkpoints` | Rich session state snapshots (JSONB) |

### Common Fields (on primary entities)

All primary entities share lifecycle management fields:

| Field | Type | Purpose |
|-------|------|---------|
| `utility_score` | `REAL` | Decay-adjusted usefulness score |
| `access_count` | `INTEGER` | Total access count |
| `last_accessed_at` | `TIMESTAMPTZ` | Last access time (drives decay) |
| `needs_embedding` | `BOOLEAN` | Flag for embedding backfill job |
| `feedback_boost` | `REAL` | CTR-derived search ranking boost |
| `superseded_by` | `TEXT` | Points to replacement entity |
| `valid_from` / `valid_until` | `TIMESTAMPTZ` | Temporal validity window |
| `source` | `TEXT` | Origin: `claude_code`, `pi-extension`, `mobile_note` |
| `source_metadata` | `JSONB` | Source-specific metadata |

### Full-Text Search

PostgreSQL `tsvector` columns use the `'simple'` dictionary (language-agnostic, required for Turkish/multilingual support). FTS is the first signal in the search pipeline and seeds graph expansion.

### Migrations

9 SQL migration files (`001_initial` through `009_wilson_score`) are compiled into the binary via `include_str!` and executed sequentially on startup. All SQL is idempotent (`CREATE TABLE IF NOT EXISTS`, `DO $$ ... END $$`). No migration tracking table — the full set runs every time.
