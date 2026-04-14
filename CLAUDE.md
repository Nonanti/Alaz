# Alaz — Project Guide for AI Assistants

Alaz is a long-term memory system for AI coding agents. This file tells any AI assistant working inside this repo how to use Alaz itself as its memory layer.

## Quick Start

```bash
cp .env.example .env          # Edit with your credentials
docker compose up -d          # PostgreSQL + Qdrant + TEI + ColBERT
cargo run -- migrate          # Apply DB migrations
cargo run -- serve            # Start server on :3456
```

## Architecture

Single Rust binary, 9 crates. See `docs/ARCHITECTURE.md` for the full design.

**Build order:**
alaz-core → alaz-db → alaz-vector → alaz-graph → alaz-intel → alaz-search → alaz-auth → alaz-server → alaz-cli

## Testing

```bash
cargo test -- --test-threads=1   # Integration tests share a DB, run single-threaded
cargo clippy --all-targets       # Zero-warning policy
cargo fmt --all -- --check       # Format check
```

## Alaz as the Primary Memory System

**Alaz IS the memory.** When working in this repo, do not use `MEMORY.md` or any file-based memory system. All knowledge, preferences, facts, decisions, and patterns live in Alaz's database and are accessed via the MCP tools it exposes.

### Memory Hierarchy

| Layer | Tool | When to use |
|-------|------|-------------|
| Core memories | `alaz_core_memory` | Persistent facts, preferences, conventions, constraints |
| Knowledge base | `alaz_save` / `alaz_search` | Code snippets, patterns, architecture decisions |
| Episodes | `alaz_episodes` | Notable events (errors, decisions, successes, discoveries) |
| Procedures | `alaz_procedures` | Step-by-step workflows with Wilson-score confidence |
| Cue recall | `alaz_cue_search` | 5W cue-based search (who / what / where / when / why) |

### During a Session — Proactive Behaviors

1. **Search before assuming.** Use `alaz_search` or `alaz_hybrid_search` for project context before answering from memory alone.
2. **Save discoveries immediately.** Use `alaz_save` as soon as something non-obvious is learned — don't wait for session end.
3. **Use cue search for recall.** `alaz_cue_search` is built for "that thing we did with X".

### Session End

The learning pipeline runs automatically on shutdown and extracts: patterns, episodes, procedures, core memories, and reflections from the session transcript. No manual action required.

### What to Save Where

| Content | Tool | Example |
|---------|------|---------|
| User preference | `alaz_core_memory` (preference) | "User prefers Turkish commit messages" |
| Project fact | `alaz_core_memory` (fact) | "DB runs on port 5435 locally" |
| Coding convention | `alaz_core_memory` (convention) | "Use CUID2 for public IDs" |
| Hard constraint | `alaz_core_memory` (constraint) | "Never commit `.env` files" |
| Code pattern | `alaz_save` (type: pattern) | Reusable templates |
| Architecture decision | `alaz_save` (type: decision) | Design docs, API contracts |

### Maintenance

- **RAPTOR rebuild**: run `alaz_raptor_rebuild` periodically (weekly, or after bulk additions)
- **Duplicate review**: the learning pipeline dedupes automatically; skim `alaz_core_memory list` occasionally to catch drift

## See Also

- `docs/ARCHITECTURE.md` — full architectural overview
- `docs/API.md` — HTTP and MCP API reference
- `docs/ROADMAP.md` — planned features
- `CONTRIBUTING.md` — how to contribute
