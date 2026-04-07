# Alaz — AI Knowledge System

## Quick Start
```bash
cp .env.example .env  # Edit with your credentials
docker compose up -d
cargo run -- migrate
cargo run -- serve
```

## Architecture
Single Rust binary, 9 crates. See docs/superpowers/specs/2026-03-22-alaz-design.md.

## Crate Build Order
alaz-core → alaz-db → alaz-vector → alaz-graph → alaz-intel → alaz-search → alaz-auth → alaz-server → alaz-cli

## Testing
```bash
cargo test -- --test-threads=1   # Integration tests need single-threaded (shared DB)
cargo clippy --all-targets       # Zero warnings policy
```

## Deployment
```bash
bash deploy.sh                   # Build + rsync + restart (configure DEPLOY_HOST first)
```

## Alaz as the Primary Memory System

**CRITICAL: Alaz IS the memory. Do NOT use MEMORY.md or any file-based memory.**

All knowledge, preferences, facts, decisions, and patterns are stored in Alaz's database and accessed via MCP tools or the pi extension.

### Memory Hierarchy

| Layer | Tool | When |
|-------|------|------|
| Core memories | `alaz_memory` | Persistent facts, preferences, conventions, constraints |
| Knowledge base | `alaz_save` / `alaz_search` | Code snippets, patterns, architecture decisions |
| Episodes | `alaz_episodes` | Notable events (errors, decisions, successes, discoveries) |
| Procedures | `alaz_procedures` | Step-by-step guides with Wilson score confidence |
| Episodic recall | `alaz_recall` | 5W cue-based search (who/what/where/when/why) |

### During Session — Proactive Behaviors

1. **Search before assuming**: Use `alaz_search` first when you need project context.
2. **Save important discoveries immediately**: Use `alaz_save` for significant findings.
3. **Use cue search for recall**: Use `alaz_recall` with 5W cues for "that thing we did with X".

### Session End
The learning pipeline auto-extracts: patterns, episodes, procedures, core memories, reflections.
