# AGENTS.md

Instructions for AI coding agents working in this repository. The full, maintained version is in
[CLAUDE.md](CLAUDE.md) (despite the name it applies to any agent); read it before changing anything.

Short version:

- NOCTURNE is a Rust desktop media center (local folders + Jellyfin servers), Linux first. The UI is being migrated
  from GPUI to Iced 0.14 (`crates/ui`; `crates/app` is the legacy GPUI UI). Crate map and architecture notes are in
  `CLAUDE.md` and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- Never modify, move or delete files in a user's local media folders.
- Before committing: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked` (or `scripts/ci-checks.sh`).
- Read-only by default. Never run anything that writes against a real Jellyfin server; use the throwaway server from
  `scripts/test-jellyfin.sh`.
- Never commit credentials, tokens, `.env`, server addresses, personal media names or screenshots of personal libraries.
- Focused commits, imperative messages, no force-push to shared branches.
