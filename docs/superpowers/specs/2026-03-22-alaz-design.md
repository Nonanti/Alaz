# Alaz — AI Knowledge System Design

**Date:** 2026-03-22
**Status:** Approved
**Author:** nonantiy

## Overview

Alaz is a persistent AI knowledge system. A single Rust binary with 6-signal hybrid search, dual embedding models, ColBERT token-level precision, multi-stage reranking, and an autonomous learning pipeline.

### Key Capabilities

**Search:**
- ColBERT token-level search
- Multi-stage reranking (cross-encoder + LLM)
- HyDE (Hypothetical Document Embeddings)
- High-quality 4096-dim embeddings (Qwen3-Embedding-8B)
- 6-signal RRF (FTS + text-vector + code-vector + ColBERT + graph + RAPTOR)
- Circuit breaker + caching patterns

**Learning:**
- Learning pipeline (auto-extract from sessions)
- Contradiction detection (auto-supersede)
- Context injection (priority-based, 8K token budget)
- Reflections (meta-cognitive session analysis)
- 5W episodic cues
- Temporal validity + supersession chains
- Tool sequence mining (N-gram analysis)
- Cross-project pattern promotion
- Memory decay + graph weight decay

**Architecture:**
- Dual embedding models (text + code-specific)
- PostgreSQL + Qdrant hybrid (structured data + purpose-built vector DB)
- Single Rust binary, zero-maintenance deployment

---

## Architecture

### Language & Runtime
- **Language:** Rust (2024 edition)
- **Binary:** Single monolithic binary with 9 crates
- **Server:** Axum HTTP + RMCP MCP

### Databases

| System | Purpose | Details |
|--------|---------|---------|
| PostgreSQL 16 | Structured data + FTS | pgvector removed; FTS via tsvector + GIN; pg_trgm for similarity |
| Qdrant | Vector search | 3 collections: dense-text (4096), dense-code (768), ColBERT (128 multi-vector) |

### External AI Services

| Service | Model | Port | Purpose |
|---------|-------|------|---------|
| Ollama | Qwen3-Embedding-8B | 11434 | Text embedding (4096-dim) |
| Ollama | nomic-embed-code | 11434 | Code embedding (768-dim) |
| TEI | Qwen3-Reranker-0.6B | 8001 | Cross-encoder reranking |
| ZhipuAI | glm-4.7 | API | LLM extraction, summarization, contradiction detection, LLM reranking |

---

## Data Model

### Core Entities (PostgreSQL)

**KnowledgeItem**
- type: artifact | pattern
- title, content, description
- language, file_path
- project_id (FK)
- search_vector (tsvector GENERATED)
- valid_from, valid_until, superseded_by
- access_count, last_accessed_at

**Episode**
- type: error | decision | success | discovery
- title, content, severity
- resolved (bool)
- who_cues, what_cues, where_cues, why_cues (TEXT[])
- project_id

**Procedure**
- title, content, steps (JSONB)
- times_used, times_success, times_failure
- success_rate (GENERATED)
- project_id

**CoreMemory**
- category: preference | fact | convention | constraint
- key, value
- confidence (float)
- confirmations, contradictions (counters)
- UNIQUE(category, key, project_id)

**SessionLog**
- project_id, cost, input_tokens, output_tokens
- duration_seconds, tools_used, status, summary

**Reflection**
- session_id (FK)
- what_worked, what_failed, lessons_learned
- effectiveness_score, complexity_score

### Graph (PostgreSQL)

**GraphEdge** (polymorphic)
- source_type, source_id → target_type, target_id
- relation (14+ types: references, derived_from, extends, depends_on, supersedes, related_to, led_to, caused, triggered, ...)
- weight (with decay), usage_count
- UNIQUE(source_type, source_id, target_type, target_id, relation)

### RAPTOR (PostgreSQL)

**RaptorTree** — project, status, total_nodes, max_depth
**RaptorNode** — level, parent_id, entity_type/id, summary

### Vectors (Qdrant)

| Collection | Model | Dimensions | Content |
|-----------|-------|-----------|---------|
| `alaz_text` | Qwen3-Embedding-8B | 4096 | Dialogue, descriptions, docs |
| `alaz_code` | nomic-embed-code | 768 | Code snippets, functions, patterns |
| `alaz_colbert` | Jina-ColBERT-v2 | 128 (multi-vector) | Token-level embeddings via MaxSim |

Each entity is embedded into relevant collections based on content type. Code-containing entities go to both `alaz_code` and `alaz_text`.

### Auth (PostgreSQL)

Owner, Device, OAuthClient, AuthorizationCode, RefreshToken, AccessToken, ApiKey, DeviceApproval, AuditLog — single-user model with device trust chain, OAuth2 + PKCE.

---

## Search Pipeline

6-signal hybrid search with RRF fusion:

```
Query
  │
  ├─[1] FTS (PostgreSQL tsvector + websearch_to_tsquery)
  │
  ├─[2] Dense Text Vector (Qdrant alaz_text, cosine)
  │
  ├─[3] Dense Code Vector (Qdrant alaz_code, cosine)
  │
  ├─[4] ColBERT MaxSim (Qdrant alaz_colbert, token-level)
  │
  ├─[5] Graph Expansion (1-hop BFS from top-10 candidates)
  │
  └─[6] RAPTOR Collapsed Tree (all levels simultaneously)
  │
  ▼
RRF Fusion (k=60): score += 1/(60+rank) per signal
  │
  ▼
Memory Decay: exp(-0.693/30 * days) * (1 + ln(1+access_count)*0.1)
  │
  ▼
Cross-Encoder Reranking (Qwen3-Reranker-0.6B via TEI)
  │
  ├─ if rerank=true → LLM Reranking (ZhipuAI glm-4.7, with explanations)
  │
  ▼
Final Results
```

**Optional HyDE:** When `hyde=true`, query goes to LLM first to generate a hypothetical ideal document, which is then embedded and used for vector search. Improves recall on ambiguous queries.

**Concurrency:** Signals 1-6 MUST execute concurrently via `tokio::join!`. Sequential execution would cause unacceptable latency (1-3s). Target: hybrid search < 500ms p95.

**Memory Decay formula:** `days` = days since `last_accessed_at` (not creation). Applied as a post-RRF score multiplier.

### ColBERT Implementation Strategy

ColBERT in Qdrant uses **named vectors** with multi-vector points. Each document produces N token vectors (128-dim each). At query time:
1. Query is tokenized into M token vectors
2. Qdrant retrieves candidate points from `alaz_colbert` collection
3. MaxSim is computed **client-side** in Rust: for each query token, find max cosine similarity across all document tokens, then sum across query tokens
4. Model: Jina-ColBERT-v2 (served via a lightweight Python FastAPI sidecar or pre-computed at index time)

This is the highest-risk technical component. If ColBERT proves too complex, it can be disabled via config without affecting the other 5 signals.

---

## Resilience & Failure Modes

| Service Down | Behavior |
|-------------|----------|
| Qdrant | Degrade to FTS-only + graph + RAPTOR (3/6 signals). Log warning. |
| Ollama | Queue embedding requests. Mark entities with `needs_embedding=true`. Background job retries every 60s. |
| TEI Reranker | Skip reranking, return RRF-scored results directly. |
| ZhipuAI | Learning pipeline queues to disk. LLM reranking skipped. HyDE disabled. |

**Embedding failure handling:** Entities are saved to PostgreSQL first (always). Embedding is async with retry. A `needs_embedding` boolean flag on each entity tracks embedding status. A background reconciliation job runs every 5 minutes, finds all `needs_embedding=true` entities, and retries. This prevents silent data loss — entities without embeddings are still findable via FTS and graph.

**Circuit breaker:** 5 consecutive failures → 60s backoff before retry. Applied per external service.

---

## Intelligence & Learning

### Learning Pipeline (session end)

```
Session JSONL Transcript
  → Parse & Chunk (~24KB boundaries on [USER]: turns)
  → LLM Extraction per chunk (ZhipuAI glm-4.7):
      - Patterns (max 5)
      - Episodes (max 3, with 5W cues)
      - Procedures (max 2)
      - Core Memories (max 3)
  → Dedup (in-memory title match + DB pg_trgm similarity)
  → Contradiction Detection:
      - Vector search for similar entities
      - LLM classification: contradiction/update/complementary/unrelated
      - Auto-supersede if confidence >= 0.8
  → Save + Embed (fire-and-forget async):
      - PostgreSQL entity save
      - Qdrant embed (text + code + ColBERT)
      - Graph enrichment (entity extraction → edges)
  → Cross-Project Promotion:
      - pg_trgm similar pattern search across projects
      - 3+ projects with similarity >= 0.5 → promote to global
  → Reflection:
      - what_worked, what_failed, lessons_learned, scores
  → Tool Sequence Mining:
      - N-gram analysis (3,4,5-grams) → save as procedures
```

### Context Injection (session start)

Priority-based, ~8K token budget:

| Priority | Section |
|----------|---------|
| 0 (mandatory) | Core Memories |
| 1 (high) | Unresolved episodes + recent errored sessions |
| 2 (medium) | Project patterns, proven procedures (>70% success), global patterns, recent reflections, cross-project intelligence |
| 3 (low) | Recent sessions, recent code snippets |

### RAPTOR (Hierarchical Clustering)

- K-Means++ with silhouette score optimization
- AI-generated cluster summaries (cached)
- Top-down cluster routing + collapsed tree search
- Per-project and global trees

---

## MCP Tools (22)

### Knowledge (7)
| Tool | Description |
|------|------------|
| `alaz_save` | Save item (auto-embed text+code+colbert, contradiction check, graph enrichment) |
| `alaz_get` | Get by ID (bump access count) |
| `alaz_search` | FTS search |
| `alaz_hybrid_search` | 6-signal hybrid + RRF + reranking + optional HyDE |
| `alaz_list` | List with filters |
| `alaz_update` | Update item |
| `alaz_delete` | Delete item |

### Graph (3)
| Tool | Description |
|------|------------|
| `alaz_relate` | Create directed relation |
| `alaz_unrelate` | Remove relation |
| `alaz_graph_explore` | Multi-hop scored traversal (causal chains included) |

### Episodic & Procedural (3)
| Tool | Description |
|------|------------|
| `alaz_episodes` | List/filter episodes |
| `alaz_procedures` | List procedures with success rates |
| `alaz_core_memory` | Read persistent facts/preferences/conventions |

### RAPTOR (2)
| Tool | Description |
|------|------------|
| `alaz_raptor_rebuild` | Rebuild hierarchical clustering tree |
| `alaz_raptor_status` | Check RAPTOR tree status |

### Session (1)
| Tool | Description |
|------|------------|
| `alaz_sessions` | Query session history |

### Relations (1)
| Tool | Description |
|------|------------|
| `alaz_relations` | Get relations with depth-based traversal |

### Orchestration (2)
| Tool | Description |
|------|------------|
| `alaz_orchestrate` | Create parallel Claude Code agent jobs |
| `alaz_orchestrate_status` | Check orchestration job status |

### Advanced Search (3)
| Tool | Description |
|------|------------|
| `alaz_similar` | Find similar entities by vector similarity |
| `alaz_cue_search` | 5W cue-based episodic search |
| `alaz_cross_project` | Cross-project knowledge retrieval |

### REST API

All MCP tools are also exposed as `GET/POST /api/v1/...` REST endpoints. Dual interface pattern.

### CLI Hooks

```bash
alaz hook start    # Context injection at session start
alaz hook stop     # Learning pipeline trigger at session end
```

---

## Crate Structure

```
alaz/
├── Cargo.toml                 (workspace)
├── crates/
│   ├── alaz-core/             # Entity types, traits, error enum, config
│   ├── alaz-db/               # PostgreSQL data access (sqlx, migrations)
│   ├── alaz-vector/           # Qdrant client (dense text/code + ColBERT)
│   ├── alaz-search/           # 6-signal hybrid search + RRF + reranking
│   ├── alaz-graph/            # Polymorphic graph, causal chains, promotion
│   ├── alaz-intel/            # LLM, embeddings, extraction, RAPTOR, HyDE, learner
│   ├── alaz-server/           # Axum HTTP + RMCP MCP server
│   ├── alaz-auth/             # JWT + API key + OAuth2/PKCE
│   └── alaz-cli/              # CLI binary (serve, hook, migrate, raptor)
├── docker-compose.yml         # PostgreSQL + Qdrant
└── .env                       # Configuration
```

---

## Deployment

```
Deploy Server (your-server)
├── alaz binary          → systemd user service, port 3456
├── PostgreSQL 16        → Docker, port 5434
├── Qdrant               → Docker, port 6333/6334
├── Ollama               → native, port 11434
│   ├── qwen3-embedding-8b     (text embedding, 4096-dim)
│   └── nomic-embed-code       (code embedding, 768-dim)
├── TEI Reranker         → Docker, port 8001
│   └── Qwen3-Reranker-0.6B
└── ZhipuAI API          → external (glm-4.7)
```

