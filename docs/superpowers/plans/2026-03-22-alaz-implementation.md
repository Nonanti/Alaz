# Alaz Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a single Rust binary AI knowledge system with 6-signal hybrid search, dual embeddings, ColBERT, and autonomous learning.

**Architecture:** Monolithic Rust workspace with 9 crates. PostgreSQL for structured data + FTS, Qdrant for vector search (3 collections). External AI services via HTTP (Ollama, TEI, ZhipuAI).

**Tech Stack:** Rust 2024, axum 0.8, sqlx 0.8, qdrant-client 1.16, rmcp 0.16, tokio 1.49, reqwest 0.13, tracing, serde, clap 4.6

**Spec:** `docs/superpowers/specs/2026-03-22-alaz-design.md`

**Note:** Git repo is already initialized. PostgreSQL runs on port 5435.

**Deferred to v0.2:** OAuth2/PKCE auth flow, device trust management. v0.1 uses JWT + API key only.

**Crate build order:** alaz-core → alaz-db → alaz-vector → alaz-graph → alaz-intel → alaz-search → alaz-auth → alaz-server → alaz-cli. Tasks follow this order.

---

## File Structure

```
alaz/
├── Cargo.toml                          # Workspace manifest
├── .env                                # Runtime config
├── docker-compose.yml                  # PostgreSQL + Qdrant
├── CLAUDE.md                           # Development guide
│
├── crates/
│   ├── alaz-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # Re-exports
│   │       ├── config.rs               # AppConfig (DB, Qdrant, Ollama, TEI, ZhipuAI URLs)
│   │       ├── error.rs                # AlazError enum (thiserror)
│   │       ├── models/
│   │       │   ├── mod.rs
│   │       │   ├── knowledge.rs        # KnowledgeItem, KnowledgeType
│   │       │   ├── episode.rs          # Episode, EpisodeType, Severity
│   │       │   ├── procedure.rs        # Procedure, ProcedureStep
│   │       │   ├── core_memory.rs      # CoreMemory, MemoryCategory
│   │       │   ├── session.rs          # SessionLog, SessionCheckpoint
│   │       │   ├── reflection.rs       # Reflection
│   │       │   ├── graph.rs            # GraphEdge, RelationType, EntityRef
│   │       │   ├── raptor.rs           # RaptorTree, RaptorNode
│   │       │   └── project.rs          # Project
│   │       └── traits.rs              # Repository traits, SearchSignal trait
│   │
│   ├── alaz-db/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pool.rs                 # PgPool setup
│   │       ├── migrations/
│   │       │   └── 001_initial.sql     # Full schema
│   │       └── repos/
│   │           ├── mod.rs
│   │           ├── knowledge.rs        # KnowledgeRepo
│   │           ├── episode.rs          # EpisodeRepo
│   │           ├── procedure.rs        # ProcedureRepo
│   │           ├── core_memory.rs      # CoreMemoryRepo
│   │           ├── session.rs          # SessionRepo
│   │           ├── reflection.rs       # ReflectionRepo
│   │           ├── graph.rs            # GraphRepo
│   │           ├── raptor.rs           # RaptorRepo
│   │           └── project.rs          # ProjectRepo
│   │
│   ├── alaz-vector/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── client.rs              # QdrantManager (connection, collection setup)
│   │       ├── dense.rs               # DenseVectorOps (text + code upsert/search)
│   │       └── colbert.rs             # ColbertOps (multi-vector upsert, MaxSim search)
│   │
│   ├── alaz-intel/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── llm.rs                 # LlmClient (ZhipuAI, OpenAI-compat)
│   │       ├── embeddings.rs          # EmbeddingService (Ollama: text + code)
│   │       ├── colbert.rs             # ColbertService (tokenize + embed)
│   │       ├── learner.rs             # SessionLearner (transcript → extract → save)
│   │       ├── contradiction.rs       # ContradictionDetector
│   │       ├── raptor.rs              # RaptorBuilder (K-Means++, clustering, summaries)
│   │       ├── hyde.rs                # HydeGenerator (hypothetical doc generation)
│   │       ├── context.rs             # ContextInjector (priority-based, 8K budget)
│   │       ├── reflection.rs          # ReflectionGenerator
│   │       └── tool_mining.rs         # ToolSequenceMiner (N-gram analysis)
│   │
│   ├── alaz-graph/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── traversal.rs           # BFS multi-hop scored traversal
│   │       ├── causal.rs              # Causal chain following
│   │       ├── promotion.rs           # Cross-project pattern promotion
│   │       ├── scoring.rs             # relevance = weight * recency * usage * maturity
│   │       └── decay.rs               # Weight decay background job
│   │
│   ├── alaz-search/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pipeline.rs            # SearchPipeline (orchestrate 6 signals)
│   │       ├── fusion.rs              # RRF fusion (k=60)
│   │       ├── decay.rs               # Memory decay (recency * usage)
│   │       ├── rerank.rs              # CrossEncoderReranker + LLM fallback
│   │       └── signals/
│   │           ├── mod.rs
│   │           ├── fts.rs             # PostgreSQL FTS signal
│   │           ├── dense_text.rs      # Qdrant text vector signal
│   │           ├── dense_code.rs      # Qdrant code vector signal
│   │           ├── colbert.rs         # ColBERT MaxSim signal
│   │           ├── graph.rs           # Graph expansion signal
│   │           └── raptor.rs          # RAPTOR collapsed tree signal
│   │
│   ├── alaz-auth/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── jwt.rs                 # JWT issue/verify
│   │       ├── apikey.rs              # API key hash/verify (SHA-256)
│   │       ├── oauth.rs               # OAuth2 + PKCE flows
│   │       ├── device.rs              # Device trust management
│   │       └── middleware.rs          # Axum auth extractors
│   │
│   ├── alaz-server/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── state.rs               # AppState (pools, clients, config)
│   │       ├── router.rs              # Axum router setup
│   │       ├── middleware.rs          # Rate limit, logging, CORS
│   │       ├── mcp/
│   │       │   ├── mod.rs             # MCP server setup (rmcp)
│   │       │   ├── knowledge.rs       # save, get, search, hybrid_search, list, update, delete
│   │       │   ├── graph.rs           # relate, unrelate, graph_explore
│   │       │   ├── episodic.rs        # episodes, procedures, core_memory
│   │       │   ├── raptor.rs          # raptor_rebuild, raptor_status
│   │       │   ├── session.rs         # sessions
│   │       │   ├── relations.rs       # relations
│   │       │   ├── orchestration.rs   # orchestrate, orchestrate_status
│   │       │   └── advanced.rs        # similar, cue_search, cross_project
│   │       └── api/
│   │           ├── mod.rs             # REST router
│   │           ├── knowledge.rs       # REST knowledge endpoints
│   │           ├── graph.rs
│   │           ├── episodic.rs
│   │           ├── raptor.rs
│   │           ├── session.rs
│   │           ├── orchestration.rs
│   │           ├── advanced.rs
│   │           ├── context.rs         # GET /api/v1/context (hook start)
│   │           └── learn.rs           # POST /api/v1/sessions/:id/learn (hook stop)
│   │
│   └── alaz-cli/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs               # clap: serve, hook start/stop, migrate, raptor rebuild
│
└── tests/
    └── integration/
        ├── mod.rs
        ├── helpers.rs                 # Test DB setup, cleanup
        ├── knowledge_test.rs
        ├── search_test.rs
        ├── graph_test.rs
        ├── learning_test.rs
        └── mcp_test.rs
```

---

## Chunk 1: Foundation (alaz-core + workspace + docker + CLI skeleton)

### Task 1: Workspace & Docker Setup

**Files:**
- Create: `Cargo.toml`
- Create: `docker-compose.yml`
- Create: `.env`
- Create: `CLAUDE.md`
- Create: `.gitignore`

- [ ] **Step 1: Create workspace Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/alaz-core",
    "crates/alaz-db",
    "crates/alaz-vector",
    "crates/alaz-intel",
    "crates/alaz-graph",
    "crates/alaz-search",
    "crates/alaz-auth",
    "crates/alaz-server",
    "crates/alaz-cli",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
authors = ["nonantiy <nonantiy1@gmail.com>"]

[workspace.dependencies]
# Async
tokio = { version = "1.49", features = ["full"] }

# Web
axum = { version = "0.8", features = ["macros"] }
tower-http = { version = "0.6", features = ["cors", "trace", "limit"] }
reqwest = { version = "0.13", features = ["json"] }

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "chrono", "uuid", "json"] }

# Vector
qdrant-client = "1.16"

# MCP
rmcp = { version = "0.16", features = ["server", "transport-streamable-http"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.2"

# CLI
clap = { version = "4.6", features = ["derive"] }

# Auth
jsonwebtoken = "10.3"
argon2 = "0.5"
sha2 = "0.10"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# Error handling
thiserror = "2.0"
anyhow = "1.0"

# Utils
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.22", features = ["v4", "serde"] }
cuid2 = "0.1"

# Internal crates
alaz-core = { path = "crates/alaz-core" }
alaz-db = { path = "crates/alaz-db" }
alaz-vector = { path = "crates/alaz-vector" }
alaz-intel = { path = "crates/alaz-intel" }
alaz-graph = { path = "crates/alaz-graph" }
alaz-search = { path = "crates/alaz-search" }
alaz-auth = { path = "crates/alaz-auth" }
alaz-server = { path = "crates/alaz-server" }
```

- [ ] **Step 2: Create docker-compose.yml**

```yaml
services:
  postgres:
    image: pgvector/pgvector:pg16
    container_name: alaz-pg
    environment:
      POSTGRES_USER: alaz
      POSTGRES_PASSWORD: alaz
      POSTGRES_DB: alaz
    ports:
      - "5435:5432"
    volumes:
      - alaz-pg-data:/var/lib/postgresql/data

  qdrant:
    image: qdrant/qdrant:latest
    container_name: alaz-qdrant
    ports:
      - "6333:6333"
      - "6334:6334"
    volumes:
      - alaz-qdrant-data:/qdrant/storage

  tei-reranker:
    image: ghcr.io/huggingface/text-embeddings-inference:latest
    container_name: alaz-tei
    command: --model-id Qwen/Qwen3-Reranker-0.6B
    ports:
      - "8001:80"
    volumes:
      - alaz-tei-data:/data

volumes:
  alaz-pg-data:
  alaz-qdrant-data:
  alaz-tei-data:
```

- [ ] **Step 3: Create .env**

```bash
DATABASE_URL=postgres://alaz:alaz@localhost:5435/alaz
QDRANT_URL=http://localhost:6333
OLLAMA_URL=http://localhost:11434
TEI_URL=http://localhost:8001
ZHIPUAI_API_KEY=your-key-here
ZHIPUAI_MODEL=glm-4.7
TEXT_EMBED_MODEL=qwen3-embedding-8b
CODE_EMBED_MODEL=nomic-embed-code
JWT_SECRET=change-this-in-production
LISTEN_ADDR=0.0.0.0:3456
RUST_LOG=alaz=debug,tower_http=debug
```

- [ ] **Step 4: Create .gitignore**

```
/target
.env
*.swp
*.swo
```

- [ ] **Step 5: Create CLAUDE.md**

```markdown
# Alaz — AI Knowledge System

## Quick Start
```bash
docker compose up -d          # Start PostgreSQL + Qdrant
cargo run -- migrate          # Run migrations
cargo run -- serve            # Start server on :3456
```

## Architecture
Single Rust binary, 9 crates. See `docs/superpowers/specs/2026-03-22-alaz-design.md`.

## Development
```bash
cargo build                   # Build all
cargo test                    # Run all tests
cargo run -- serve            # Dev server
```

## Crate Dependency Order
alaz-core → alaz-db → alaz-vector → alaz-graph → alaz-intel → alaz-search → alaz-auth → alaz-server → alaz-cli
```

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml docker-compose.yml .env.example .gitignore CLAUDE.md
git commit -m "chore: initialize workspace with docker and config"
```

---

### Task 2: alaz-core — Error & Config

**Files:**
- Create: `crates/alaz-core/Cargo.toml`
- Create: `crates/alaz-core/src/lib.rs`
- Create: `crates/alaz-core/src/error.rs`
- Create: `crates/alaz-core/src/config.rs`

- [ ] **Step 1: Create alaz-core Cargo.toml**

```toml
[package]
name = "alaz-core"
version.workspace = true
edition.workspace = true

[dependencies]
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
uuid.workspace = true
sqlx.workspace = true
schemars.workspace = true
```

- [ ] **Step 2: Write error.rs**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AlazError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("duplicate: {0}")]
    Duplicate(String),

    #[error("validation: {0}")]
    Validation(String),

    #[error("database: {0}")]
    Database(#[from] sqlx::Error),

    #[error("qdrant: {0}")]
    Qdrant(String),

    #[error("embedding: {0}")]
    Embedding(String),

    #[error("llm: {0}")]
    Llm(String),

    #[error("reranker: {0}")]
    Reranker(String),

    #[error("auth: {0}")]
    Auth(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
}

pub type Result<T> = std::result::Result<T, AlazError>;
```

- [ ] **Step 3: Write config.rs**

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    pub qdrant_url: String,
    pub ollama_url: String,
    pub tei_url: String,
    pub zhipuai_api_key: String,
    pub zhipuai_model: String,
    pub text_embed_model: String,
    pub code_embed_model: String,
    pub jwt_secret: String,
    pub listen_addr: String,
}

impl AppConfig {
    pub fn from_env() -> crate::Result<Self> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL")
                .map_err(|_| crate::AlazError::Validation("DATABASE_URL not set".into()))?,
            qdrant_url: std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".into()),
            ollama_url: std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into()),
            tei_url: std::env::var("TEI_URL").unwrap_or_else(|_| "http://localhost:8001".into()),
            zhipuai_api_key: std::env::var("ZHIPUAI_API_KEY").unwrap_or_default(),
            zhipuai_model: std::env::var("ZHIPUAI_MODEL").unwrap_or_else(|_| "glm-4.7".into()),
            text_embed_model: std::env::var("TEXT_EMBED_MODEL").unwrap_or_else(|_| "qwen3-embedding-8b".into()),
            code_embed_model: std::env::var("CODE_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-code".into()),
            jwt_secret: std::env::var("JWT_SECRET")
                .map_err(|_| crate::AlazError::Validation("JWT_SECRET not set".into()))?,
            listen_addr: std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3456".into()),
        })
    }
}
```

- [ ] **Step 4: Write lib.rs**

```rust
pub mod config;
pub mod error;
pub mod models;
pub mod traits;

pub use config::AppConfig;
pub use error::{AlazError, Result};
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p alaz-core`
Expected: success (models and traits modules will be empty stubs for now)

- [ ] **Step 6: Commit**

```bash
git add crates/alaz-core/
git commit -m "feat(core): add error types and config"
```

---

### Task 3: alaz-core — Models

**Files:**
- Create: `crates/alaz-core/src/models/mod.rs`
- Create: `crates/alaz-core/src/models/knowledge.rs`
- Create: `crates/alaz-core/src/models/episode.rs`
- Create: `crates/alaz-core/src/models/procedure.rs`
- Create: `crates/alaz-core/src/models/core_memory.rs`
- Create: `crates/alaz-core/src/models/session.rs`
- Create: `crates/alaz-core/src/models/reflection.rs`
- Create: `crates/alaz-core/src/models/graph.rs`
- Create: `crates/alaz-core/src/models/raptor.rs`
- Create: `crates/alaz-core/src/models/project.rs`

- [ ] **Step 1: Write knowledge.rs**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum KnowledgeType {
    Artifact,
    Pattern,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgeItem {
    pub id: String,
    pub title: String,
    pub content: String,
    pub description: Option<String>,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub language: Option<String>,
    pub file_path: Option<String>,
    pub project_id: Option<String>,
    pub tags: Vec<String>,
    pub access_count: i32,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub needs_embedding: bool,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub superseded_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateKnowledge {
    pub title: String,
    pub content: String,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub file_path: Option<String>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateKnowledge {
    pub title: Option<String>,
    pub content: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
    pub file_path: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ListKnowledgeFilter {
    pub kind: Option<String>,
    pub language: Option<String>,
    pub project: Option<String>,
    pub tag: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
```

- [ ] **Step 2: Write episode.rs**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Episode {
    pub id: String,
    pub title: String,
    pub content: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub severity: Option<String>,
    pub resolved: bool,
    pub who_cues: Vec<String>,
    pub what_cues: Vec<String>,
    pub where_cues: Vec<String>,
    pub when_cues: Vec<String>,
    pub why_cues: Vec<String>,
    pub project_id: Option<String>,
    pub needs_embedding: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEpisode {
    pub title: String,
    pub content: String,
    pub kind: String,
    pub severity: Option<String>,
    pub who_cues: Option<Vec<String>>,
    pub what_cues: Option<Vec<String>>,
    pub where_cues: Option<Vec<String>>,
    pub when_cues: Option<Vec<String>>,
    pub why_cues: Option<Vec<String>>,
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListEpisodesFilter {
    pub kind: Option<String>,
    pub project: Option<String>,
    pub resolved: Option<bool>,
    pub limit: Option<i64>,
}
```

- [ ] **Step 3: Write procedure.rs**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Procedure {
    pub id: String,
    pub title: String,
    pub content: String,
    pub steps: serde_json::Value,
    pub times_used: i32,
    pub times_success: i32,
    pub times_failure: i32,
    pub success_rate: Option<f64>,
    pub project_id: Option<String>,
    pub tags: Vec<String>,
    pub needs_embedding: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProcedure {
    pub title: String,
    pub content: String,
    pub steps: Option<serde_json::Value>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ListProceduresFilter {
    pub project: Option<String>,
    pub tag: Option<String>,
    pub limit: Option<i64>,
}
```

- [ ] **Step 4: Write core_memory.rs**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CoreMemory {
    pub id: String,
    pub category: String,
    pub key: String,
    pub value: String,
    pub confidence: f64,
    pub confirmations: i32,
    pub contradictions: i32,
    pub project_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertCoreMemory {
    pub category: String,
    pub key: String,
    pub value: String,
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListCoreMemoryFilter {
    pub category: Option<String>,
    pub project: Option<String>,
    pub limit: Option<i64>,
}
```

- [ ] **Step 5: Write session.rs, reflection.rs, graph.rs, raptor.rs, project.rs**

`session.rs`:
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SessionLog {
    pub id: String,
    pub project_id: Option<String>,
    pub cost: Option<f64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub duration_seconds: Option<i64>,
    pub tools_used: Option<serde_json::Value>,
    pub status: String,
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ListSessionsFilter {
    pub project: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
```

`reflection.rs`:
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Reflection {
    pub id: String,
    pub session_id: String,
    pub what_worked: String,
    pub what_failed: String,
    pub lessons_learned: String,
    pub effectiveness_score: Option<f64>,
    pub complexity_score: Option<f64>,
    pub project_id: Option<String>,
    pub created_at: DateTime<Utc>,
}
```

`graph.rs`:
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRef {
    pub entity_type: String,
    pub entity_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GraphEdge {
    pub id: String,
    pub source_type: String,
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub relation: String,
    pub weight: f64,
    pub usage_count: i32,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRelation {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct GraphExploreParams {
    pub entity_type: String,
    pub entity_id: String,
    pub depth: Option<u32>,
    pub min_weight: Option<f64>,
    pub relation_filter: Option<String>,
}

/// Result of graph traversal - an entity with its accumulated score
#[derive(Debug, Clone, Serialize)]
pub struct ScoredEntity {
    pub entity_type: String,
    pub entity_id: String,
    pub score: f64,
    pub path: Vec<String>,
}
```

`raptor.rs`:
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RaptorTree {
    pub id: String,
    pub project_id: Option<String>,
    pub status: String,
    pub total_nodes: i32,
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
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub summary: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub children_count: i32,
    pub created_at: DateTime<Utc>,
}
```

`project.rs`:
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}
```

- [ ] **Step 6: Write models/mod.rs**

```rust
pub mod knowledge;
pub mod episode;
pub mod procedure;
pub mod core_memory;
pub mod session;
pub mod reflection;
pub mod graph;
pub mod raptor;
pub mod project;

pub use knowledge::*;
pub use episode::*;
pub use procedure::*;
pub use core_memory::*;
pub use session::*;
pub use reflection::*;
pub use graph::*;
pub use raptor::*;
pub use project::*;
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo check -p alaz-core`
Expected: success

- [ ] **Step 8: Commit**

```bash
git add crates/alaz-core/
git commit -m "feat(core): add all entity models"
```

---

### Task 4: alaz-core — Traits

**Files:**
- Create: `crates/alaz-core/src/traits.rs`

- [ ] **Step 1: Write traits.rs**

```rust
use crate::models::*;
use crate::Result;

/// Unified search result across all entity types
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub content: String,
    pub score: f64,
    pub project: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// A single signal's contribution to search
#[derive(Debug, Clone)]
pub struct SignalResult {
    pub entity_type: String,
    pub entity_id: String,
    pub rank: usize,
}

/// Search query parameters
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub project: Option<String>,
    pub limit: Option<usize>,
    pub rerank: Option<bool>,
    pub hyde: Option<bool>,
    pub graph_expand: Option<bool>,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p alaz-core`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/alaz-core/src/traits.rs
git commit -m "feat(core): add search result types and traits"
```

---

### Task 5: alaz-cli — Skeleton

**Files:**
- Create: `crates/alaz-cli/Cargo.toml`
- Create: `crates/alaz-cli/src/main.rs`

- [ ] **Step 1: Create alaz-cli Cargo.toml**

```toml
[package]
name = "alaz-cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "alaz"
path = "src/main.rs"

[dependencies]
alaz-core.workspace = true
clap.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
dotenvy = "0.15"
```

- [ ] **Step 2: Write main.rs skeleton**

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "alaz", about = "AI Knowledge System")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the server
    Serve,
    /// Run database migrations
    Migrate,
    /// Session hooks
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Rebuild RAPTOR tree
    RaptorRebuild {
        /// Project name filter
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Session start — inject context
    Start,
    /// Session stop — trigger learning
    Stop,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve => {
            tracing::info!("starting alaz server...");
            // TODO: start server
            println!("server not yet implemented");
        }
        Commands::Migrate => {
            tracing::info!("running migrations...");
            // TODO: run migrations
            println!("migrations not yet implemented");
        }
        Commands::Hook { action } => match action {
            HookAction::Start => {
                // TODO: context injection
                println!("hook start not yet implemented");
            }
            HookAction::Stop => {
                // TODO: learning pipeline
                println!("hook stop not yet implemented");
            }
        },
        Commands::RaptorRebuild { project } => {
            tracing::info!(?project, "rebuilding RAPTOR tree...");
            // TODO: raptor rebuild
            println!("raptor rebuild not yet implemented");
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Verify it compiles and runs**

Run: `cargo build -p alaz-cli && cargo run -p alaz-cli -- --help`
Expected: Shows help text with serve, migrate, hook, raptor-rebuild commands

- [ ] **Step 4: Commit**

```bash
git add crates/alaz-cli/
git commit -m "feat(cli): add CLI skeleton with serve, migrate, hook, raptor commands"
```

---

## Chunk 2: Database Layer (alaz-db)

### Task 6: Database Migration

**Files:**
- Create: `crates/alaz-db/Cargo.toml`
- Create: `crates/alaz-db/src/lib.rs`
- Create: `crates/alaz-db/src/pool.rs`
- Create: `crates/alaz-db/src/migrations/001_initial.sql`

- [ ] **Step 1: Create alaz-db Cargo.toml**

```toml
[package]
name = "alaz-db"
version.workspace = true
edition.workspace = true

[dependencies]
alaz-core.workspace = true
sqlx.workspace = true
tokio.workspace = true
tracing.workspace = true
chrono.workspace = true
uuid.workspace = true
cuid2.workspace = true
serde.workspace = true
serde_json.workspace = true
```

- [ ] **Step 2: Write pool.rs**

```rust
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_pool(database_url: &str) -> alaz_core::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> alaz_core::Result<()> {
    let sql = include_str!("migrations/001_initial.sql");
    sqlx::raw_sql(sql).execute(pool).await?;
    tracing::info!("migrations completed");
    Ok(())
}
```

- [ ] **Step 3: Write 001_initial.sql**

```sql
-- Extensions
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS vector;

-- Projects
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    path TEXT,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Knowledge Items
CREATE TABLE IF NOT EXISTS knowledge_items (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    description TEXT,
    type TEXT NOT NULL DEFAULT 'artifact',
    language TEXT,
    file_path TEXT,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    tags TEXT[] NOT NULL DEFAULT '{}',
    access_count INT NOT NULL DEFAULT 0,
    last_accessed_at TIMESTAMPTZ,
    needs_embedding BOOLEAN NOT NULL DEFAULT TRUE,
    valid_from TIMESTAMPTZ,
    valid_until TIMESTAMPTZ,
    superseded_by TEXT REFERENCES knowledge_items(id) ON DELETE SET NULL,
    search_vector TSVECTOR GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(description, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(content, '')), 'C')
    ) STORED,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_knowledge_items_search ON knowledge_items USING GIN(search_vector);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_project ON knowledge_items(project_id);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_type ON knowledge_items(type);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_tags ON knowledge_items USING GIN(tags);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_title_trgm ON knowledge_items USING GIN(title gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_needs_embedding ON knowledge_items(needs_embedding) WHERE needs_embedding = TRUE;

-- Episodes
CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    type TEXT NOT NULL,
    severity TEXT,
    resolved BOOLEAN NOT NULL DEFAULT FALSE,
    who_cues TEXT[] NOT NULL DEFAULT '{}',
    what_cues TEXT[] NOT NULL DEFAULT '{}',
    where_cues TEXT[] NOT NULL DEFAULT '{}',
    when_cues TEXT[] NOT NULL DEFAULT '{}',
    why_cues TEXT[] NOT NULL DEFAULT '{}',
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    needs_embedding BOOLEAN NOT NULL DEFAULT TRUE,
    search_vector TSVECTOR GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(content, '')), 'C')
    ) STORED,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_episodes_search ON episodes USING GIN(search_vector);
CREATE INDEX IF NOT EXISTS idx_episodes_project ON episodes(project_id);
CREATE INDEX IF NOT EXISTS idx_episodes_type ON episodes(type);
CREATE INDEX IF NOT EXISTS idx_episodes_resolved ON episodes(resolved);
CREATE INDEX IF NOT EXISTS idx_episodes_who_cues ON episodes USING GIN(who_cues);
CREATE INDEX IF NOT EXISTS idx_episodes_what_cues ON episodes USING GIN(what_cues);
CREATE INDEX IF NOT EXISTS idx_episodes_when_cues ON episodes USING GIN(when_cues);
CREATE INDEX IF NOT EXISTS idx_episodes_needs_embedding ON episodes(needs_embedding) WHERE needs_embedding = TRUE;

-- Procedures
CREATE TABLE IF NOT EXISTS procedures (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    steps JSONB,
    times_used INT NOT NULL DEFAULT 0,
    times_success INT NOT NULL DEFAULT 0,
    times_failure INT NOT NULL DEFAULT 0,
    success_rate DOUBLE PRECISION GENERATED ALWAYS AS (
        CASE WHEN times_used > 0 THEN times_success::DOUBLE PRECISION / times_used ELSE NULL END
    ) STORED,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    tags TEXT[] NOT NULL DEFAULT '{}',
    needs_embedding BOOLEAN NOT NULL DEFAULT TRUE,
    search_vector TSVECTOR GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(content, '')), 'C')
    ) STORED,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_procedures_search ON procedures USING GIN(search_vector);
CREATE INDEX IF NOT EXISTS idx_procedures_project ON procedures(project_id);
CREATE INDEX IF NOT EXISTS idx_procedures_needs_embedding ON procedures(needs_embedding) WHERE needs_embedding = TRUE;

-- Core Memories
CREATE TABLE IF NOT EXISTS core_memories (
    id TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    confirmations INT NOT NULL DEFAULT 1,
    contradictions INT NOT NULL DEFAULT 0,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(category, key, project_id)
);

CREATE INDEX IF NOT EXISTS idx_core_memories_category ON core_memories(category);
CREATE INDEX IF NOT EXISTS idx_core_memories_project ON core_memories(project_id);

-- Session Logs
CREATE TABLE IF NOT EXISTS session_logs (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    cost DOUBLE PRECISION,
    input_tokens BIGINT,
    output_tokens BIGINT,
    duration_seconds BIGINT,
    tools_used JSONB,
    status TEXT NOT NULL DEFAULT 'completed',
    summary TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_session_logs_project ON session_logs(project_id);
CREATE INDEX IF NOT EXISTS idx_session_logs_status ON session_logs(status);
CREATE INDEX IF NOT EXISTS idx_session_logs_created ON session_logs(created_at DESC);

-- Session Checkpoints
CREATE TABLE IF NOT EXISTS session_checkpoints (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session_logs(id) ON DELETE CASCADE,
    checkpoint_data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Reflections
CREATE TABLE IF NOT EXISTS reflections (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session_logs(id) ON DELETE CASCADE,
    what_worked TEXT NOT NULL,
    what_failed TEXT NOT NULL,
    lessons_learned TEXT NOT NULL,
    effectiveness_score DOUBLE PRECISION,
    complexity_score DOUBLE PRECISION,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reflections_session ON reflections(session_id);
CREATE INDEX IF NOT EXISTS idx_reflections_project ON reflections(project_id);

-- Graph Edges (polymorphic)
CREATE TABLE IF NOT EXISTS graph_edges (
    id TEXT PRIMARY KEY,
    source_type TEXT NOT NULL,
    source_id TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    relation TEXT NOT NULL,
    weight DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    usage_count INT NOT NULL DEFAULT 1,
    description TEXT,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(source_type, source_id, target_type, target_id, relation)
);

CREATE INDEX IF NOT EXISTS idx_graph_edges_source ON graph_edges(source_type, source_id);
CREATE INDEX IF NOT EXISTS idx_graph_edges_target ON graph_edges(target_type, target_id);
CREATE INDEX IF NOT EXISTS idx_graph_edges_relation ON graph_edges(relation);

-- RAPTOR Trees
CREATE TABLE IF NOT EXISTS raptor_trees (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'building',
    total_nodes INT NOT NULL DEFAULT 0,
    max_depth INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id)
);

-- RAPTOR Nodes
CREATE TABLE IF NOT EXISTS raptor_nodes (
    id TEXT PRIMARY KEY,
    tree_id TEXT NOT NULL REFERENCES raptor_trees(id) ON DELETE CASCADE,
    level INT NOT NULL DEFAULT 0,
    parent_id TEXT REFERENCES raptor_nodes(id) ON DELETE SET NULL,
    entity_type TEXT,
    entity_id TEXT,
    summary TEXT,
    children_count INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_raptor_nodes_tree ON raptor_nodes(tree_id);
CREATE INDEX IF NOT EXISTS idx_raptor_nodes_level ON raptor_nodes(tree_id, level);
CREATE INDEX IF NOT EXISTS idx_raptor_nodes_parent ON raptor_nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_raptor_nodes_entity ON raptor_nodes(entity_type, entity_id);

-- Auth tables
CREATE TABLE IF NOT EXISTS owners (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES owners(id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL UNIQUE,
    name TEXT,
    trusted BOOLEAN NOT NULL DEFAULT FALSE,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES owners(id) ON DELETE CASCADE,
    key_hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS audit_logs (
    id TEXT PRIMARY KEY,
    owner_id TEXT,
    event TEXT NOT NULL,
    details JSONB,
    ip_address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_created ON audit_logs(created_at DESC);

-- Orchestration Jobs
CREATE TABLE IF NOT EXISTS orchestration_jobs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    tasks JSONB NOT NULL,
    results JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- [ ] **Step 4: Write lib.rs**

```rust
pub mod pool;
pub mod repos;

pub use pool::{create_pool, run_migrations};
```

- [ ] **Step 5: Start Docker and test migration**

Run: `cd /home/nonantiy/Projects/Alaz && docker compose up -d`
Then: `cargo run -p alaz-cli -- migrate` (after wiring up migrate command)

- [ ] **Step 6: Commit**

```bash
git add crates/alaz-db/
git commit -m "feat(db): add PostgreSQL schema with FTS, graph, RAPTOR tables"
```

---

### Task 7: Repository Implementations

**Files:**
- Create: `crates/alaz-db/src/repos/mod.rs`
- Create: `crates/alaz-db/src/repos/knowledge.rs`
- Create: `crates/alaz-db/src/repos/episode.rs`
- Create: `crates/alaz-db/src/repos/procedure.rs`
- Create: `crates/alaz-db/src/repos/core_memory.rs`
- Create: `crates/alaz-db/src/repos/session.rs`
- Create: `crates/alaz-db/src/repos/reflection.rs`
- Create: `crates/alaz-db/src/repos/graph.rs`
- Create: `crates/alaz-db/src/repos/raptor.rs`
- Create: `crates/alaz-db/src/repos/project.rs`

- [ ] **Step 1: Write knowledge.rs repo**

```rust
use alaz_core::models::*;
use alaz_core::Result;
use sqlx::PgPool;

pub struct KnowledgeRepo;

impl KnowledgeRepo {
    pub async fn create(pool: &PgPool, input: &CreateKnowledge, project_id: Option<&str>) -> Result<KnowledgeItem> {
        let id = cuid2::create_id();
        let kind = input.kind.as_deref().unwrap_or("artifact");
        let tags = input.tags.as_deref().unwrap_or(&[]);

        let item = sqlx::query_as::<_, KnowledgeItem>(
            r#"INSERT INTO knowledge_items (id, title, content, description, type, language, file_path, project_id, tags)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
               RETURNING *"#
        )
        .bind(&id)
        .bind(&input.title)
        .bind(&input.content)
        .bind(&input.description)
        .bind(kind)
        .bind(&input.language)
        .bind(&input.file_path)
        .bind(project_id)
        .bind(tags)
        .fetch_one(pool)
        .await?;

        Ok(item)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<KnowledgeItem> {
        let item = sqlx::query_as::<_, KnowledgeItem>(
            r#"UPDATE knowledge_items
               SET access_count = access_count + 1, last_accessed_at = NOW()
               WHERE id = $1
               RETURNING *"#
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| alaz_core::AlazError::NotFound(format!("knowledge item {id}")))?;

        Ok(item)
    }

    pub async fn update(pool: &PgPool, id: &str, input: &UpdateKnowledge) -> Result<KnowledgeItem> {
        let item = sqlx::query_as::<_, KnowledgeItem>(
            r#"UPDATE knowledge_items SET
                title = COALESCE($2, title),
                content = COALESCE($3, content),
                description = COALESCE($4, description),
                language = COALESCE($5, language),
                file_path = COALESCE($6, file_path),
                tags = COALESCE($7, tags),
                needs_embedding = TRUE,
                updated_at = NOW()
               WHERE id = $1
               RETURNING *"#
        )
        .bind(id)
        .bind(&input.title)
        .bind(&input.content)
        .bind(&input.description)
        .bind(&input.language)
        .bind(&input.file_path)
        .bind(&input.tags)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| alaz_core::AlazError::NotFound(format!("knowledge item {id}")))?;

        Ok(item)
    }

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM knowledge_items WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(alaz_core::AlazError::NotFound(format!("knowledge item {id}")));
        }
        Ok(())
    }

    pub async fn list(pool: &PgPool, filter: &ListKnowledgeFilter) -> Result<Vec<KnowledgeItem>> {
        let limit = filter.limit.unwrap_or(20);
        let offset = filter.offset.unwrap_or(0);

        let items = sqlx::query_as::<_, KnowledgeItem>(
            r#"SELECT * FROM knowledge_items
               WHERE ($1::TEXT IS NULL OR type = $1)
                 AND ($2::TEXT IS NULL OR language = $2)
                 AND ($3::TEXT IS NULL OR project_id = $3)
                 AND ($4::TEXT IS NULL OR $4 = ANY(tags))
                 AND valid_until IS NULL
                 AND superseded_by IS NULL
               ORDER BY updated_at DESC
               LIMIT $5 OFFSET $6"#
        )
        .bind(&filter.kind)
        .bind(&filter.language)
        .bind(&filter.project)
        .bind(&filter.tag)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(items)
    }

    pub async fn fts_search(pool: &PgPool, query: &str, project: Option<&str>, limit: i64) -> Result<Vec<(KnowledgeItem, f32)>> {
        let results = sqlx::query_as::<_, (KnowledgeItem, f32)>(
            r#"SELECT ki.*, ts_rank(search_vector, websearch_to_tsquery('english', $1)) AS rank
               FROM knowledge_items ki
               WHERE search_vector @@ websearch_to_tsquery('english', $1)
                 AND ($2::TEXT IS NULL OR project_id = $2)
                 AND valid_until IS NULL
                 AND superseded_by IS NULL
               ORDER BY rank DESC
               LIMIT $3"#
        )
        .bind(query)
        .bind(project)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(results)
    }

    pub async fn find_needing_embedding(pool: &PgPool, limit: i64) -> Result<Vec<KnowledgeItem>> {
        let items = sqlx::query_as::<_, KnowledgeItem>(
            "SELECT * FROM knowledge_items WHERE needs_embedding = TRUE LIMIT $1"
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(items)
    }

    pub async fn mark_embedded(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("UPDATE knowledge_items SET needs_embedding = FALSE WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn find_similar_by_title(pool: &PgPool, title: &str, threshold: f64) -> Result<Vec<KnowledgeItem>> {
        let items = sqlx::query_as::<_, KnowledgeItem>(
            r#"SELECT * FROM knowledge_items
               WHERE similarity(title, $1) >= $2
                 AND valid_until IS NULL
               ORDER BY similarity(title, $1) DESC
               LIMIT 5"#
        )
        .bind(title)
        .bind(threshold as f32)
        .fetch_all(pool)
        .await?;

        Ok(items)
    }

    pub async fn supersede(pool: &PgPool, old_id: &str, new_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE knowledge_items SET superseded_by = $2, valid_until = NOW(), updated_at = NOW() WHERE id = $1"
        )
        .bind(old_id)
        .bind(new_id)
        .execute(pool)
        .await?;
        Ok(())
    }
}
```

- [ ] **Step 2: Write episode.rs, procedure.rs, core_memory.rs, session.rs, reflection.rs repos**

Each follows the same CRUD pattern as knowledge.rs. Key differences:

`episode.rs`: includes `cue_search` method using `&& (array overlap)` on 5W cue columns.
`procedure.rs`: includes `record_outcome(id, success: bool)` that increments times_used + times_success/failure.
`core_memory.rs`: uses `ON CONFLICT (category, key, project_id) DO UPDATE SET value = $4, confirmations = confirmations + 1` for upsert.
`session.rs`: standard CRUD with filter by project/status.
`reflection.rs`: standard CRUD, linked to session_id.

- [ ] **Step 3: Write graph.rs repo**

```rust
use alaz_core::models::*;
use alaz_core::Result;
use sqlx::PgPool;

pub struct GraphRepo;

impl GraphRepo {
    pub async fn create_edge(pool: &PgPool, input: &CreateRelation, source_type: &str, target_type: &str) -> Result<GraphEdge> {
        let id = cuid2::create_id();
        let edge = sqlx::query_as::<_, GraphEdge>(
            r#"INSERT INTO graph_edges (id, source_type, source_id, target_type, target_id, relation, description, metadata)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               ON CONFLICT (source_type, source_id, target_type, target_id, relation)
               DO UPDATE SET
                   weight = GREATEST(graph_edges.weight, 1.0),
                   usage_count = graph_edges.usage_count + 1,
                   last_used_at = NOW()
               RETURNING *"#
        )
        .bind(&id)
        .bind(source_type)
        .bind(&input.source_id)
        .bind(target_type)
        .bind(&input.target_id)
        .bind(&input.relation)
        .bind(&input.description)
        .bind(&input.metadata)
        .fetch_one(pool)
        .await?;

        Ok(edge)
    }

    pub async fn delete_edge(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM graph_edges WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(alaz_core::AlazError::NotFound(format!("edge {id}")));
        }
        Ok(())
    }

    pub async fn get_edges(pool: &PgPool, entity_type: &str, entity_id: &str, direction: &str) -> Result<Vec<GraphEdge>> {
        let edges = match direction {
            "outgoing" => {
                sqlx::query_as::<_, GraphEdge>(
                    "SELECT * FROM graph_edges WHERE source_type = $1 AND source_id = $2 ORDER BY weight DESC"
                )
                .bind(entity_type)
                .bind(entity_id)
                .fetch_all(pool)
                .await?
            }
            "incoming" => {
                sqlx::query_as::<_, GraphEdge>(
                    "SELECT * FROM graph_edges WHERE target_type = $1 AND target_id = $2 ORDER BY weight DESC"
                )
                .bind(entity_type)
                .bind(entity_id)
                .fetch_all(pool)
                .await?
            }
            _ => {
                sqlx::query_as::<_, GraphEdge>(
                    r#"SELECT * FROM graph_edges
                       WHERE (source_type = $1 AND source_id = $2)
                          OR (target_type = $1 AND target_id = $2)
                       ORDER BY weight DESC"#
                )
                .bind(entity_type)
                .bind(entity_id)
                .fetch_all(pool)
                .await?
            }
        };
        Ok(edges)
    }

    pub async fn decay_weights(pool: &PgPool) -> Result<u64> {
        let result = sqlx::query(
            r#"UPDATE graph_edges
               SET weight = weight * exp(-0.03 * EXTRACT(EPOCH FROM (NOW() - last_used_at)) / 86400.0)
               WHERE last_used_at < NOW() - INTERVAL '6 hours'"#
        )
        .execute(pool)
        .await?;

        // Prune edges with very low weight
        sqlx::query("DELETE FROM graph_edges WHERE weight < 0.05")
            .execute(pool)
            .await?;

        Ok(result.rows_affected())
    }
}
```

- [ ] **Step 4: Write raptor.rs and project.rs repos**

`project.rs`: get_or_create(name, path) — upserts by name.
`raptor.rs`: CRUD for trees and nodes, plus `get_collapsed_tree(tree_id)` that fetches all nodes, `delete_tree_nodes(tree_id)` for rebuild.

- [ ] **Step 5: Write repos/mod.rs**

```rust
pub mod knowledge;
pub mod episode;
pub mod procedure;
pub mod core_memory;
pub mod session;
pub mod reflection;
pub mod graph;
pub mod raptor;
pub mod project;

pub use knowledge::KnowledgeRepo;
pub use episode::EpisodeRepo;
pub use procedure::ProcedureRepo;
pub use core_memory::CoreMemoryRepo;
pub use session::SessionRepo;
pub use reflection::ReflectionRepo;
pub use graph::GraphRepo;
pub use raptor::RaptorRepo;
pub use project::ProjectRepo;
```

- [ ] **Step 6: Wire up migrate command in CLI**

Update `crates/alaz-cli/src/main.rs` — `Commands::Migrate` body:
```rust
let config = alaz_core::AppConfig::from_env()?;
let pool = alaz_db::create_pool(&config.database_url).await?;
alaz_db::run_migrations(&pool).await?;
println!("migrations completed successfully");
```

- [ ] **Step 7: Test migration**

Run: `docker compose up -d && cargo run -- migrate`
Expected: "migrations completed successfully"

- [ ] **Step 8: Commit**

```bash
git add crates/alaz-db/ crates/alaz-cli/src/main.rs
git commit -m "feat(db): add all repositories and migration"
```

---

## Chunk 3: Vector & Graph Layer (alaz-vector + alaz-graph)

### Task 8: Qdrant Client (alaz-vector)

**Files:**
- Create: `crates/alaz-vector/Cargo.toml`
- Create: `crates/alaz-vector/src/lib.rs`
- Create: `crates/alaz-vector/src/client.rs`
- Create: `crates/alaz-vector/src/dense.rs`
- Create: `crates/alaz-vector/src/colbert.rs`

- [ ] **Step 1: Create Cargo.toml and client.rs**

`Cargo.toml`:
```toml
[package]
name = "alaz-vector"
version.workspace = true
edition.workspace = true

[dependencies]
alaz-core.workspace = true
qdrant-client.workspace = true
tokio.workspace = true
tracing.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
```

`client.rs` — QdrantManager that creates 3 collections on init:
- `alaz_text` (4096-dim, cosine, HNSW)
- `alaz_code` (768-dim, cosine, HNSW)
- `alaz_colbert` (128-dim multi-vector, cosine)

- [ ] **Step 2: Write dense.rs**

Upsert and search operations for `alaz_text` and `alaz_code` collections. Methods:
- `upsert_text(entity_type, entity_id, project_id, embedding)` — upserts point with payload
- `upsert_code(entity_type, entity_id, project_id, embedding)` — same for code
- `search_text(embedding, project, limit)` → Vec<(entity_type, entity_id, score)>
- `search_code(embedding, project, limit)` → Vec<(entity_type, entity_id, score)>
- `delete(collection, entity_type, entity_id)` — removes point

- [ ] **Step 3: Write colbert.rs**

ColBERT multi-vector operations:
- `upsert_colbert(entity_type, entity_id, token_embeddings: Vec<Vec<f32>>)` — stores multi-vector point
- `search_colbert(query_tokens: Vec<Vec<f32>>, limit)` — retrieves candidates, computes MaxSim client-side
- MaxSim: for each query token, max cosine over all doc tokens, then sum

- [ ] **Step 4: Write lib.rs**

```rust
pub mod client;
pub mod dense;
pub mod colbert;

pub use client::QdrantManager;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p alaz-vector`

- [ ] **Step 6: Commit**

```bash
git add crates/alaz-vector/
git commit -m "feat(vector): add Qdrant client with dense and ColBERT support"
```

---

### Task 9: Graph Operations (alaz-graph)

> **Build before alaz-intel** — alaz-intel depends on alaz-graph for graph enrichment in the learning pipeline.

**Files:**
- Create: `crates/alaz-graph/Cargo.toml`
- Create: `crates/alaz-graph/src/lib.rs`
- Create: `crates/alaz-graph/src/traversal.rs`
- Create: `crates/alaz-graph/src/causal.rs`
- Create: `crates/alaz-graph/src/promotion.rs`
- Create: `crates/alaz-graph/src/scoring.rs`
- Create: `crates/alaz-graph/src/decay.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "alaz-graph"
version.workspace = true
edition.workspace = true

[dependencies]
alaz-core.workspace = true
alaz-db.workspace = true
sqlx.workspace = true
tokio.workspace = true
tracing.workspace = true
chrono.workspace = true
```

- [ ] **Step 2: Write traversal.rs — BFS multi-hop scored traversal**

Implements `explore(pool, entity_type, entity_id, max_depth, min_weight, relation_filter) -> Vec<ScoredEntity>`. Uses BFS with accumulated weight (product of edge weights along path). See Task 11 in Chunk 4 for full code.

- [ ] **Step 3: Write causal.rs**

Follows only causal relations (led_to, caused, triggered). Returns linear chain following highest-weight path at branch points. Max depth 5.

- [ ] **Step 4: Write promotion.rs**

When a pattern is saved, checks for similar patterns in other projects via `KnowledgeRepo::find_similar_by_title`. If found in 3+ projects with similarity >= 0.5, creates a global (project_id = NULL) copy and `derived_from` graph edges.

- [ ] **Step 5: Write scoring.rs**

```rust
pub fn relevance_score(weight: f64, last_used: DateTime<Utc>, usage_count: i32, created_at: DateTime<Utc>) -> f64 {
    let days_since_used = (Utc::now() - last_used).num_seconds() as f64 / 86400.0;
    let recency = (-0.693_f64 / 23.0 * days_since_used).exp();
    let usage = (1.0 + usage_count as f64).ln();
    let age_days = (Utc::now() - created_at).num_seconds() as f64 / 86400.0;
    let maturity = (1.0 + age_days / 30.0).min(2.0);
    weight * recency * usage * maturity
}
```

- [ ] **Step 6: Write decay.rs**

Background job function: `pub async fn run_decay(pool: &PgPool)` calls `GraphRepo::decay_weights()`.

- [ ] **Step 7: Verify and commit**

```bash
cargo check -p alaz-graph
git add crates/alaz-graph/
git commit -m "feat(graph): add traversal, causal chains, promotion, scoring, decay"
```

---

## Chunk 4: Intelligence & Search (alaz-intel + alaz-search)

### Task 10: Intelligence — LLM & Embeddings (alaz-intel)

**Files:**
- Create: `crates/alaz-intel/Cargo.toml`
- Create: `crates/alaz-intel/src/lib.rs`
- Create: `crates/alaz-intel/src/llm.rs`
- Create: `crates/alaz-intel/src/embeddings.rs`
- Create: `crates/alaz-intel/src/colbert.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "alaz-intel"
version.workspace = true
edition.workspace = true

[dependencies]
alaz-core.workspace = true
alaz-db.workspace = true
alaz-vector.workspace = true
alaz-graph.workspace = true
reqwest.workspace = true
tokio.workspace = true
tracing.workspace = true
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
uuid.workspace = true
cuid2.workspace = true
```

- [ ] **Step 2: Write llm.rs — ZhipuAI client**

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};
use alaz_core::Result;

pub struct LlmClient {
    client: Client,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

impl LlmClient {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    pub async fn chat(&self, system: &str, user: &str, temperature: f32) -> Result<String> {
        let req = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage { role: "system".into(), content: system.into() },
                ChatMessage { role: "user".into(), content: user.into() },
            ],
            temperature,
        };

        let resp = self.client
            .post("https://open.bigmodel.cn/api/paas/v4/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| alaz_core::AlazError::Llm(e.to_string()))?;

        let body: ChatResponse = resp.json().await
            .map_err(|e| alaz_core::AlazError::Llm(e.to_string()))?;

        body.choices.first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| alaz_core::AlazError::Llm("no response".into()))
    }
}
```

- [ ] **Step 3: Write embeddings.rs — Ollama embedding client**

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};
use alaz_core::Result;

pub struct EmbeddingService {
    client: Client,
    ollama_url: String,
    text_model: String,
    code_model: String,
}

#[derive(Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

impl EmbeddingService {
    pub fn new(ollama_url: &str, text_model: &str, code_model: &str) -> Self {
        Self {
            client: Client::new(),
            ollama_url: ollama_url.to_string(),
            text_model: text_model.to_string(),
            code_model: code_model.to_string(),
        }
    }

    pub async fn embed_text(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.embed(&self.text_model, texts).await
    }

    pub async fn embed_code(&self, codes: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.embed(&self.code_model, codes).await
    }

    async fn embed(&self, model: &str, inputs: &[&str]) -> Result<Vec<Vec<f32>>> {
        let req = EmbedRequest {
            model: model.to_string(),
            input: inputs.iter().map(|s| s.to_string()).collect(),
        };

        let resp = self.client
            .post(format!("{}/v1/embeddings", self.ollama_url))
            .json(&req)
            .send()
            .await
            .map_err(|e| alaz_core::AlazError::Embedding(e.to_string()))?;

        let body: EmbedResponse = resp.json().await
            .map_err(|e| alaz_core::AlazError::Embedding(e.to_string()))?;

        Ok(body.data.into_iter().map(|d| d.embedding).collect())
    }
}
```

- [ ] **Step 4: Write colbert.rs — ColBERT service stub**

ColBERT tokenization + embedding via a Python sidecar or pre-computed. For now, stub the interface:

```rust
use alaz_core::Result;

pub struct ColbertService {
    endpoint: String,
    client: reqwest::Client,
}

impl ColbertService {
    pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Tokenize and embed a document into N token vectors (128-dim each)
    pub async fn embed_document(&self, text: &str) -> Result<Vec<Vec<f32>>> {
        // TODO: call ColBERT model endpoint
        // For now, return empty — ColBERT is the highest-risk component
        // and can be disabled via config
        tracing::warn!("ColBERT service not configured, returning empty");
        Ok(vec![])
    }

    /// Tokenize and embed a query into M token vectors (128-dim each)
    pub async fn embed_query(&self, query: &str) -> Result<Vec<Vec<f32>>> {
        tracing::warn!("ColBERT service not configured, returning empty");
        Ok(vec![])
    }
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p alaz-intel`

- [ ] **Step 6: Commit**

```bash
git add crates/alaz-intel/
git commit -m "feat(intel): add LLM, embedding, and ColBERT service clients"
```

---

### Task 10: Intelligence — Learner, Contradiction, HyDE, Context

**Files:**
- Create: `crates/alaz-intel/src/learner.rs`
- Create: `crates/alaz-intel/src/contradiction.rs`
- Create: `crates/alaz-intel/src/hyde.rs`
- Create: `crates/alaz-intel/src/context.rs`
- Create: `crates/alaz-intel/src/reflection.rs`
- Create: `crates/alaz-intel/src/tool_mining.rs`
- Create: `crates/alaz-intel/src/raptor.rs`

- [ ] **Step 1: Write learner.rs**

Core learning pipeline: parse JSONL transcript → chunk → LLM extraction → dedup → save.
Key method: `pub async fn learn_from_session(pool, qdrant, llm, embedding, session_id, transcript: &str) -> Result<LearningSummary>`

The LLM extraction prompt asks for JSON output with patterns, episodes, procedures, and core memories per chunk.

- [ ] **Step 2: Write contradiction.rs**

On save, find similar items via title similarity + vector search. Ask LLM to classify: contradiction/update/complementary/unrelated. Auto-supersede if confidence >= 0.8.

Key method: `pub async fn check_contradiction(pool, qdrant, llm, embedding, new_item: &KnowledgeItem) -> Result<Option<ContradictionResult>>`

- [ ] **Step 3: Write hyde.rs**

Simple: send query to LLM asking "Write a short document that would perfectly answer this query", then embed the hypothetical document.

```rust
pub struct HydeGenerator {
    llm: Arc<LlmClient>,
}

impl HydeGenerator {
    pub async fn generate(&self, query: &str) -> Result<String> {
        self.llm.chat(
            "Generate a short, factual document that would perfectly answer the following search query. Write only the document content, nothing else.",
            query,
            0.3,
        ).await
    }
}
```

- [ ] **Step 4: Write context.rs**

Priority-based context injection with ~8K token budget. Reads core memories (P0), unresolved episodes (P1), project patterns (P2), recent sessions (P3).

Key method: `pub async fn build_context(pool, project_path: &str) -> Result<String>`

- [ ] **Step 5: Write reflection.rs and tool_mining.rs**

`reflection.rs`: Post-session reflection via LLM (what worked, what failed, lessons learned).
`tool_mining.rs`: N-gram analysis (3,4,5-grams) of tool usage sequences from session data.

- [ ] **Step 6: Write raptor.rs**

RAPTOR builder: K-Means++ clustering with silhouette score, AI-generated summaries, hierarchical tree construction.

Key method: `pub async fn rebuild_tree(pool, qdrant, llm, embedding, project_id: Option<&str>) -> Result<RaptorTree>`

- [ ] **Step 7: Verify it compiles**

Run: `cargo check -p alaz-intel`

- [ ] **Step 8: Commit**

```bash
git add crates/alaz-intel/
git commit -m "feat(intel): add learner, contradiction, HyDE, context injection, RAPTOR"
```

---

### Task 12: Search Pipeline (alaz-search)

**Files:**
- Create: `crates/alaz-search/Cargo.toml`
- Create: `crates/alaz-search/src/lib.rs`
- Create: `crates/alaz-search/src/pipeline.rs`
- Create: `crates/alaz-search/src/fusion.rs`
- Create: `crates/alaz-search/src/decay.rs`
- Create: `crates/alaz-search/src/rerank.rs`
- Create: `crates/alaz-search/src/signals/mod.rs`
- Create: `crates/alaz-search/src/signals/fts.rs`
- Create: `crates/alaz-search/src/signals/dense_text.rs`
- Create: `crates/alaz-search/src/signals/dense_code.rs`
- Create: `crates/alaz-search/src/signals/colbert.rs`
- Create: `crates/alaz-search/src/signals/graph.rs`
- Create: `crates/alaz-search/src/signals/raptor.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "alaz-search"
version.workspace = true
edition.workspace = true

[dependencies]
alaz-core.workspace = true
alaz-db.workspace = true
alaz-vector.workspace = true
alaz-intel.workspace = true
alaz-graph.workspace = true
tokio.workspace = true
tracing.workspace = true
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
```

- [ ] **Step 2: Write each signal module**

Each signal implements:
```rust
pub async fn execute(/* deps */, query: &str, project: Option<&str>, limit: usize) -> Result<Vec<SignalResult>>
```

- `fts.rs`: queries knowledge_items, episodes, procedures via tsvector
- `dense_text.rs`: embeds query → search alaz_text collection
- `dense_code.rs`: embeds query → search alaz_code collection
- `colbert.rs`: ColBERT embed query → search alaz_colbert → MaxSim
- `graph.rs`: takes top-10 from FTS+vector, does 1-hop BFS expansion
- `raptor.rs`: searches RAPTOR nodes across all levels, maps to leaf entities

- [ ] **Step 3: Write fusion.rs — RRF**

```rust
use alaz_core::traits::SignalResult;
use std::collections::HashMap;

const RRF_K: f64 = 60.0;

pub fn reciprocal_rank_fusion(signals: Vec<Vec<SignalResult>>) -> Vec<(String, String, f64)> {
    let mut scores: HashMap<(String, String), f64> = HashMap::new();

    for signal in signals {
        for (rank, result) in signal.iter().enumerate() {
            let key = (result.entity_type.clone(), result.entity_id.clone());
            *scores.entry(key).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }

    let mut results: Vec<_> = scores.into_iter()
        .map(|((et, eid), score)| (et, eid, score))
        .collect();
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    results
}
```

- [ ] **Step 4: Write decay.rs — memory decay scoring**

```rust
use chrono::{DateTime, Utc};

pub fn apply_decay(score: f64, last_accessed: Option<DateTime<Utc>>, access_count: i32) -> f64 {
    let days = last_accessed
        .map(|la| (Utc::now() - la).num_seconds() as f64 / 86400.0)
        .unwrap_or(30.0);

    let recency = (-0.693_f64 / 30.0 * days).exp();
    let usage = 1.0 + (1.0 + access_count as f64).ln() * 0.1;

    score * recency * usage
}
```

- [ ] **Step 5: Write rerank.rs — cross-encoder + LLM fallback**

```rust
pub struct Reranker {
    tei_url: String,
    llm: Option<Arc<LlmClient>>,
    client: reqwest::Client,
}

impl Reranker {
    /// Cross-encoder reranking via TEI
    pub async fn rerank_cross_encoder(&self, query: &str, docs: &[(String, String)]) -> Result<Vec<f64>> {
        // POST to TEI /rerank endpoint
        // Returns relevance scores
    }

    /// Optional LLM reranking with explanations
    pub async fn rerank_llm(&self, query: &str, docs: &[(String, String)]) -> Result<Vec<(f64, String)>> {
        // Ask LLM to rate each doc 0-10 with explanation
    }
}
```

- [ ] **Step 6: Write pipeline.rs — orchestrate everything**

```rust
pub struct SearchPipeline {
    pool: PgPool,
    qdrant: Arc<QdrantManager>,
    embedding: Arc<EmbeddingService>,
    colbert: Arc<ColbertService>,
    reranker: Reranker,
    hyde: HydeGenerator,
}

impl SearchPipeline {
    pub async fn hybrid_search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        // 1. Optional HyDE: generate hypothetical doc, embed it
        // 2. Embed query for text + code vectors
        // 3. Run all 6 signals concurrently via tokio::join!
        // 4. RRF fusion
        // 5. Apply memory decay
        // 6. Cross-encoder reranking
        // 7. Optional LLM reranking
        // 8. Hydrate results with full entity data
    }
}
```

The key `tokio::join!` call:
```rust
let (fts_results, text_results, code_results, colbert_results, graph_results, raptor_results) = tokio::join!(
    signals::fts::execute(&self.pool, &query.query, query.project.as_deref(), limit),
    signals::dense_text::execute(&self.qdrant, &text_embedding, query.project.as_deref(), limit),
    signals::dense_code::execute(&self.qdrant, &code_embedding, query.project.as_deref(), limit),
    signals::colbert::execute(&self.qdrant, &self.colbert, &query.query, limit),
    signals::graph::execute(&self.pool, &fts_candidates, &vector_candidates),
    signals::raptor::execute(&self.pool, &self.qdrant, &text_embedding, query.project.as_deref(), limit),
);
```

Note: graph and raptor signals depend on other signal results. In practice, run FTS + text + code + ColBERT first (parallel), then graph + RAPTOR with candidates (parallel).

- [ ] **Step 7: Verify and commit**

```bash
cargo check -p alaz-search
git add crates/alaz-search/
git commit -m "feat(search): add 6-signal hybrid pipeline with RRF, decay, reranking"
```

---

## Chunk 5: Auth & Server (alaz-auth + alaz-server)

### Task 13: Auth (alaz-auth)

**Files:**
- Create: `crates/alaz-auth/Cargo.toml`
- Create: `crates/alaz-auth/src/lib.rs`
- Create: `crates/alaz-auth/src/jwt.rs`
- Create: `crates/alaz-auth/src/apikey.rs`
- Create: `crates/alaz-auth/src/middleware.rs`

- [ ] **Step 1: Create Cargo.toml and implement JWT + API key auth**

JWT: issue/verify using `jsonwebtoken` crate with HS256.
API Key: SHA-256 hash comparison.
Middleware: Axum extractor that checks `Authorization: Bearer <token>` or `X-API-Key: <key>`.

- [ ] **Step 2: Verify and commit**

```bash
cargo check -p alaz-auth
git add crates/alaz-auth/
git commit -m "feat(auth): add JWT and API key authentication"
```

---

### Task 14: Server — MCP Tools (alaz-server)

**Files:**
- Create: `crates/alaz-server/Cargo.toml`
- Create: `crates/alaz-server/src/lib.rs`
- Create: `crates/alaz-server/src/state.rs`
- Create: `crates/alaz-server/src/router.rs`
- Create: `crates/alaz-server/src/middleware.rs`
- Create: `crates/alaz-server/src/mcp/mod.rs`
- Create: `crates/alaz-server/src/mcp/knowledge.rs`
- Create: `crates/alaz-server/src/mcp/graph.rs`
- Create: `crates/alaz-server/src/mcp/episodic.rs`
- Create: `crates/alaz-server/src/mcp/raptor.rs`
- Create: `crates/alaz-server/src/mcp/session.rs`
- Create: `crates/alaz-server/src/mcp/relations.rs`
- Create: `crates/alaz-server/src/mcp/orchestration.rs`
- Create: `crates/alaz-server/src/mcp/advanced.rs`

- [ ] **Step 1: Write state.rs — AppState**

```rust
use std::sync::Arc;
use sqlx::PgPool;
use alaz_vector::QdrantManager;
use alaz_intel::{LlmClient, EmbeddingService, ColbertService};
use alaz_search::SearchPipeline;
use alaz_core::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub qdrant: Arc<QdrantManager>,
    pub llm: Arc<LlmClient>,
    pub embedding: Arc<EmbeddingService>,
    pub colbert: Arc<ColbertService>,
    pub search: Arc<SearchPipeline>,
    pub config: Arc<AppConfig>,
}
```

- [ ] **Step 2: Write MCP tool handlers using rmcp**

Each MCP tool is a method on a struct that implements the rmcp `ServerHandler` trait. Use `#[tool]` macro from rmcp.

Example for `alaz_save`:
```rust
#[tool(description = "Save a knowledge item to Alaz")]
async fn alaz_save(
    &self,
    #[arg(description = "Short descriptive title")] title: String,
    #[arg(description = "The actual content")] content: String,
    #[arg(description = "Optional description")] description: Option<String>,
    #[arg(description = "Type: artifact or pattern")] r#type: Option<String>,
    #[arg(description = "Programming language")] language: Option<String>,
    #[arg(description = "Original file path")] file_path: Option<String>,
    #[arg(description = "Project name")] project: Option<String>,
    #[arg(description = "Tags for categorization")] tags: Option<Vec<String>>,
) -> Result<CallToolResult, McpError> {
    // 1. Resolve project_id
    // 2. Create knowledge item
    // 3. Fire-and-forget: embed (text + code + ColBERT)
    // 4. Fire-and-forget: contradiction check
    // 5. Fire-and-forget: graph enrichment
    // 6. Return saved item as JSON
}
```

Implement all 22 tools across the mcp/ modules.

- [ ] **Step 3: Write REST API endpoints**

Mirror all MCP tools as REST endpoints under `/api/v1/`. Plus:
- `GET /api/v1/context?path=...` — context injection for hooks
- `POST /api/v1/sessions/:id/learn` — learning pipeline trigger

- [ ] **Step 4: Write router.rs — combine MCP + REST + middleware**

```rust
pub async fn build_router(state: AppState) -> Router {
    let mcp_router = // rmcp StreamableHTTP transport at /mcp
    let api_router = api::router(state.clone());

    Router::new()
        .nest("/mcp", mcp_router)
        .nest("/api/v1", api_router)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
```

- [ ] **Step 5: Wire up serve command in CLI**

```rust
Commands::Serve => {
    let config = AppConfig::from_env()?;
    let pool = alaz_db::create_pool(&config.database_url).await?;
    alaz_db::run_migrations(&pool).await?;
    let qdrant = Arc::new(QdrantManager::new(&config.qdrant_url).await?);
    // ... build AppState ...
    let app = alaz_server::build_router(state).await;
    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    tracing::info!("listening on {}", config.listen_addr);
    axum::serve(listener, app).await?;
}
```

- [ ] **Step 6: Wire up hook commands in CLI**

`hook start`: reads cwd from stdin, calls `GET /api/v1/context?path=...` on local server, outputs JSON to stdout.
`hook stop`: reads session data from stdin, calls `POST /api/v1/sessions/:id/learn`.

- [ ] **Step 7: Full build and smoke test**

Run: `cargo build`
Expected: Successful build of all 9 crates

Run: `docker compose up -d && cargo run -- migrate && cargo run -- serve`
Expected: Server starts on port 3456

- [ ] **Step 8: Commit**

```bash
git add crates/alaz-server/ crates/alaz-auth/ crates/alaz-cli/src/main.rs
git commit -m "feat(server): add MCP tools, REST API, and CLI wiring"
```

---

## Chunk 6: Integration, Background Jobs & Deployment

### Task 15: Background Jobs

**Files:**
- Modify: `crates/alaz-server/src/lib.rs`

- [ ] **Step 1: Add background job spawning on server start**

```rust
// Spawn background jobs
tokio::spawn(embedding_backfill_job(pool.clone(), qdrant.clone(), embedding.clone()));
tokio::spawn(graph_decay_job(pool.clone()));
```

`embedding_backfill_job`: every 5 minutes, finds entities with `needs_embedding=true`, embeds them, marks as done.

`graph_decay_job`: every 6 hours, calls `GraphRepo::decay_weights()`.

- [ ] **Step 2: Commit**

```bash
git add crates/alaz-server/
git commit -m "feat(server): add embedding backfill and graph decay background jobs"
```

---

### Task 16: Docker & Deployment Config

**Files:**
- Modify: `docker-compose.yml` (add Qdrant if not present)
- Create: `alaz.service` (systemd unit file)

- [ ] **Step 1: Create systemd service file**

```ini
[Unit]
Description=Alaz AI Knowledge System
After=network.target

[Service]
Type=simple
ExecStart=/home/your-user/alaz/alaz serve
WorkingDirectory=/home/your-user/alaz
EnvironmentFile=/home/your-user/alaz/.env
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

- [ ] **Step 2: Add deploy script**

```bash
#!/bin/bash
# deploy.sh — build and deploy to server
cargo build --release
rsync -avz target/release/alaz your-user@your-server:~/alaz/
rsync -avz .env your-user@your-server:~/alaz/
rsync -avz docker-compose.yml your-user@your-server:~/alaz/
ssh your-user@your-server "systemctl --user restart alaz"
```

- [ ] **Step 3: Commit**

```bash
git add alaz.service deploy.sh docker-compose.yml
git commit -m "feat: add systemd service and deploy script"
```

---

### Task 17: Claude Code MCP Configuration

**Files:**
- Create docs with MCP config example

- [ ] **Step 1: Document MCP config for Claude Code**

Add to CLAUDE.md the MCP server configuration:

```json
{
  "mcpServers": {
    "alaz": {
      "type": "streamable-http",
      "url": "http://localhost:3456/mcp",
      "headers": {
        "X-API-Key": "your-api-key"
      }
    }
  }
}
```

And hook configuration:
```json
{
  "hooks": {
    "SessionStart": [{
      "type": "command",
      "command": "alaz hook start"
    }],
    "SessionStop": [{
      "type": "command",
      "command": "alaz hook stop"
    }]
  }
}
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add MCP and hook configuration guide"
```

---

### Task 18: Integration Tests

**Files:**
- Create: `tests/integration/mod.rs`
- Create: `tests/integration/helpers.rs`
- Create: `tests/integration/knowledge_test.rs`
- Create: `tests/integration/search_test.rs`

- [ ] **Step 1: Write test helpers**

```rust
// helpers.rs — test database setup
pub async fn setup_test_db() -> PgPool {
    let pool = create_pool(&std::env::var("TEST_DATABASE_URL").unwrap()).await.unwrap();
    run_migrations(&pool).await.unwrap();
    pool
}
```

- [ ] **Step 2: Write knowledge CRUD tests**

Test: create, get (with access count bump), update (sets needs_embedding), delete, list with filters, FTS search.

- [ ] **Step 3: Write search pipeline tests**

Test: hybrid search returns results from multiple signals, RRF fusion scoring, memory decay application.

- [ ] **Step 4: Run tests**

Run: `cargo test --test integration`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add tests/
git commit -m "test: add integration tests for knowledge CRUD and search"
```

---

## Chunk 7: Deploy

### Task 19: Deploy & Launch

- [ ] **Step 1: Build release binary**

```bash
cargo build --release
```

- [ ] **Step 2: Deploy to server**

```bash
./deploy.sh
```

- [ ] **Step 3: Start services on server**

```bash
ssh your-user@your-server "cd ~/alaz && docker compose up -d && systemctl --user enable alaz && systemctl --user start alaz"
```

- [ ] **Step 4: Verify**

Test MCP connection, run a hybrid search, verify context injection on session start.
