# Agent Instructions

gh-dash is a terminal UI for GitHub (PRs, issues, notifications, local branches), a single Rust
binary built on ratatui. It started as a port of the Go [dlvhdr/gh-dash](https://github.com/dlvhdr/gh-dash).

## Build & Test

```bash
cargo build
cargo test
cargo clippy --all-targets
cargo fmt --all -- --check
cargo deny check bans licenses sources
make install  # installs gh-dash to ~/.cargo/bin
```

Rust warnings and `clippy::all` are denied in `Cargo.toml`.

## Conventions

- Run `cargo fmt --all` before you commit.
- Config files keep the Go version's format; `docs/` documents them. Keep both in step when you change config.
- GitHub reads go through GraphQL (`src/github.rs`); mutations run the `gh` CLI (`src/tasks.rs`).
- Syntax highlighting uses the Chroma lexer XMLs vendored in `assets/chroma/`, compiled in by `build.rs`.

## Releasing

In a single `release: X.Y.Z` commit:

1. Bump `version` in `Cargo.toml` and run `cargo update -p gh-dash`.
2. Cut a new dated section under `[Unreleased]` in `CHANGELOG.md`.

Then push `main` and push a `vX.Y.Z` tag. The tag triggers `release.yml`, which runs cargo-dist:
it builds the shell installer and the macOS/Linux archives and publishes the homebrew formula to
`juanibiapina/homebrew-taps`.
