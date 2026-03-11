# Contributing

Thank you for your interest in contributing to Claude Monitor!

## Getting Started

1. Fork the repository
2. Create a feature branch from `main`
3. Make your changes
4. Open a PR against `main`

## Development Setup

```bash
# Prerequisites: Rust, Node.js, system libs (see README)
npm install
cd src-tauri && cargo build
```

## Rules

### What PRs can change

- Bug fixes
- UI improvements
- New features (discuss in an issue first)
- Documentation
- Tests

### What PRs cannot change

The following require maintainer authorship or explicit pre-approval:

- **`.github/workflows/`** — CI/CD pipelines
- **OAuth/credential handling** (`claude.rs`, `config.rs`)
- **Network endpoints** — adding new domains or API calls
- **Dependencies** — adding new crates or npm packages (discuss first)
- **Tauri capabilities** — expanding what the frontend can access
- **Release process** — tags, signing, distribution

### Code Standards

- `cargo fmt` — all Rust code formatted
- `cargo clippy -- -D warnings` — no warnings
- `cargo test` — all tests pass
- No `unsafe` without justification and maintainer approval
- No `unwrap()` in production code — use proper error handling

### Security

- Never log credentials or tokens
- Never add new network calls without discussion
- Never weaken Tauri's capability permissions
- Report vulnerabilities privately (see [SECURITY.md](SECURITY.md))

## Review Process

All PRs require maintainer approval. Security-sensitive changes may take longer to review. Please be patient.
