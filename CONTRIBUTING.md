# Contributing to Alaz

Thanks for your interest in contributing! Whether it's a bug report, feature request, documentation improvement, or code contribution — all are welcome.

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (2024 edition)
- [Docker](https://docs.docker.com/get-docker/) & Docker Compose

### Development Setup

```bash
# Clone and enter the repo
git clone https://github.com/Nonanti/Alaz.git
cd alaz

# Start dependencies (PostgreSQL, Qdrant, TEI, ColBERT)
docker compose up -d

# Configure environment
cp .env.example .env
# Edit .env — at minimum set JWT_SECRET

# Run migrations
cargo run -- migrate

# Run tests
cargo test --workspace

# Start development server
cargo run -- serve
```

### Crate Build Order

Changes to a crate may require rebuilding downstream crates:

```
alaz-core → alaz-db → alaz-vector → alaz-graph → alaz-intel → alaz-search → alaz-auth → alaz-server → alaz-cli
```

## Making Changes

### Before You Start

1. Check [existing issues](https://github.com/Nonanti/Alaz/issues) to avoid duplicate work
2. For large changes, open an issue first to discuss the approach
3. For small fixes, feel free to submit a PR directly

### Code Quality

All PRs must pass:

```bash
cargo check --workspace       # Compilation
cargo test --workspace        # 456 tests
cargo clippy --workspace -- -D warnings  # Zero warnings
cargo fmt --all -- --check    # Formatting
```

### Code Style

- Follow existing patterns in the codebase
- Use `snake_case` for all identifiers
- Errors use `AlazError` variants from `alaz-core`
- New MCP tools go in `crates/alaz-server/src/mcp/mod.rs`
- Database migrations go in `crates/alaz-db/src/migrations/`
- Unit tests go in `#[cfg(test)] mod tests` at the end of each file
- Unit tests must NOT use async or DB connections

### Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add new MCP tool for tag management
fix: handle empty search results in RRF fusion
refactor: simplify circuit breaker state machine
docs: add ColBERT architecture diagram
chore: update dependencies
```

## Pull Request Process

1. Fork the repository
2. Create a feature branch from `main`
3. Make your changes with tests
4. Ensure all CI checks pass
5. Open a PR against `main`
6. Describe what changed and why

## Good First Issues

Look for issues labeled [`good first issue`](https://github.com/Nonanti/Alaz/labels/good%20first%20issue) — these are scoped, well-documented tasks ideal for new contributors.

## Areas Where Help is Needed

- 🌐 **Integrations**: Cursor, Windsurf, VS Code extensions
- 📖 **Documentation**: Tutorials, guides, examples
- 🧪 **Testing**: Integration tests, benchmarks
- 🐛 **Bug fixes**: Check the issue tracker
- 🌍 **Internationalization**: README translations

## Questions?

Open a [Discussion](https://github.com/Nonanti/Alaz/discussions) or an issue — happy to help!
