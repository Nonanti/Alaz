# Alaz API Reference

> Updated: 2026-03-31

## REST Endpoints

All endpoints require authentication via `Authorization: Bearer <jwt>` or `X-API-Key: <key>`.

### Knowledge

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/knowledge` | Create knowledge item |
| POST | `/api/v1/knowledge/bulk` | Bulk create (max 100) |
| GET | `/api/v1/knowledge` | List with filters |
| GET | `/api/v1/knowledge/:id` | Get by ID |
| PUT | `/api/v1/knowledge/:id` | Update |
| DELETE | `/api/v1/knowledge/:id` | Delete |
| DELETE | `/api/v1/knowledge/bulk` | Bulk delete |
| POST | `/api/v1/knowledge/:id/usage` | Record pattern usage |

### Episodes

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/episodes` | Create episode |
| GET | `/api/v1/episodes` | List with filters |
| GET | `/api/v1/episodes/:id` | Get by ID |
| PUT | `/api/v1/episodes/:id/resolve` | Mark resolved |
| DELETE | `/api/v1/episodes/:id` | Delete |
| POST | `/api/v1/episodes/by-files` | Find by file paths |
| POST | `/api/v1/episodes/recall` | 5W cue search |

### Search

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/search` | 6-signal hybrid search |
| POST | `/api/v1/search/feedback` | Record click feedback |

### Graph

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/graph/edges` | Create edge |
| POST | `/api/v1/graph/explore` | Multi-hop traversal |
| POST | `/api/v1/graph/causal-chain` | Follow causal chain |

### Sessions

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/sessions` | Create session |
| GET | `/api/v1/sessions` | List sessions |
| GET | `/api/v1/sessions/:id` | Get session |
| POST | `/api/v1/sessions/:id/checkpoint` | Save checkpoint |
| GET | `/api/v1/sessions/:id/checkpoints` | List checkpoints |

### Context & Learning

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/context` | Build context for project |
| POST | `/api/v1/learn` | Trigger learning pipeline |
| POST | `/api/v1/ingest` | Universal content ingestion |

### Git Integration

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/git/ingest` | Ingest git commit + diff |

### Codebase

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/code/index` | Index file symbols |
| POST | `/api/v1/code/impact` | Impact analysis for a symbol |
| GET | `/api/v1/code/hot-files` | Most changed files |
| GET | `/api/v1/code/coupling` | Temporally coupled files |

### System

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check (all services) |
| POST | `/api/proactive-context` | Proactive context injection |
| GET | `/api/v1/system/stats` | System statistics |
| GET | `/api/v1/projects` | List projects |
| GET | `/api/v1/vault` | List vault secrets |
| POST | `/api/v1/vault` | Store encrypted secret |

---

## MCP Tools (29 total)

### Core CRUD
| Tool | Description |
|------|-------------|
| `alaz_search` | 6-signal hybrid search with reranking |
| `alaz_save` | Save knowledge (pattern, snippet, decision, artifact) |
| `alaz_save_episode` | Record an event (error, decision, discovery, success) |
| `alaz_save_procedure` | Save step-by-step procedure |
| `alaz_memory` | CRUD core memories (facts, preferences, conventions) |
| `alaz_delete` | Delete knowledge, episode, or procedure |

### Retrieval & Analysis
| Tool | Description |
|------|-------------|
| `alaz_episodes` | List/filter episodes |
| `alaz_procedures` | List procedures with Wilson scores |
| `alaz_recall` | 5W episodic recall |
| `alaz_graph` | Explore knowledge graph |
| `alaz_cross_project` | Cross-project search |
| `alaz_explain` | Explain why a search result was returned |
| `alaz_impact` | Code symbol impact analysis |

### Session & Context
| Tool | Description |
|------|-------------|
| `alaz_sessions` | Query session history |
| `alaz_checkpoint_save` | Save rich session checkpoint |
| `alaz_checkpoint_list` | List checkpoints |
| `alaz_checkpoint_restore` | Restore latest checkpoint |
| `alaz_context_budget` | Check context window usage |
| `alaz_optimize_context` | Compress context for efficiency |
| `alaz_compact_restore` | Build restore context for new session |

### Knowledge Management
| Tool | Description |
|------|-------------|
| `alaz_supersede` | Replace old entity with new version |
| `alaz_search_feedback` | Record click feedback |
| `alaz_procedure_outcome` | Record procedure success/failure |
| `alaz_ingest` | Universal content ingestion |
| `alaz_health` | Knowledge health report with gap detection |
| `alaz_evolution` | Trace knowledge evolution through supersede chain |
| `alaz_review` | Record spaced repetition review (SM-2) |
| `alaz_review_list` | Items due for review |

### Reflections & Timeline
| Tool | Description |
|------|-------------|
| `alaz_create_reflection` | Create granular reflection |
| `alaz_reflections` | List/filter reflections |
| `alaz_reflection_insights` | Score trends over time |
| `alaz_timeline` | Chronological event timeline |
| `alaz_episode_chain` | Follow causal episode chains |
| `alaz_episode_link` | Link episodes causally |
| `alaz_vault_store` | Store encrypted secret |
| `alaz_vault_get` | Retrieve decrypted secret |
| `alaz_vault_list` | List secret names |
| `alaz_vault_delete` | Delete secret |

---

## Background Jobs (6 total)

| Job | Interval | Description |
|-----|----------|-------------|
| Embedding Backfill | 5 min | Embed entities with `needs_embedding=true` |
| Graph Decay | 6 hours | Exponential decay on graph edge weights |
| Memory Decay | 6 hours | Decay/boost/prune entity utility scores |
| Feedback Aggregation | 12 hours | CTR-based feedback_boost updates |
| Weight Learning | 7 days | Learn optimal signal weights from clicks |
| Consolidation | 7 days | Merge similar knowledge items via LLM |
