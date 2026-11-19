# AGENTS.md

Instructions for AI coding agents working in this repository. The full, maintained version is in
[CLAUDE.md](CLAUDE.md) (despite the name it applies to any agent); read it before changing anything.

Short version:

- NOCTURNE is a Rust desktop media center (local folders + Jellyfin servers), Linux first (Wayland only; Windows and macOS are unsigned previews). The UI is Iced 0.14 (`crates/app`, binary `nocturne`; run with `cargo run -p nocturne-app`). Crate map and architecture notes are in `CLAUDE.md` and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- Never modify, move or delete files in a user's local media folders.
- Before committing: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` (or `scripts/ci-checks.sh`).
- Read-only by default. Never run anything that writes against a real Jellyfin server; use the throwaway server from `scripts/test-jellyfin.sh`.
- Keep OS-specific code inside the platform seams ([docs/PLATFORMS.md](docs/PLATFORMS.md)); no new macOS or Windows implementations before that phase.
- Test launches go through `scripts/run-isolated.sh target/debug/nocturne` (throwaway config, data and keyring, working directory outside the repo). Debug hooks: `NOCTURNE_SNAP=/tmp/x.png` captures the app window, `NOCTURNE_PREVIEW=<screen>` opens a screen with canned data. Never capture the whole screen. For real keyboard or clipboard input use the invisible sway from `scripts/headless-session.sh` (`scripts/headless-input-check.sh` is an example); never send synthetic input to the user's own session.
- Several worktrees share one build cache: `source scripts/build-env.sh`, and run `scripts/cache-budget.sh` before big builds.
- Never read, print or commit signing keys (`keys/`); releases are signed locally by the maintainer ([docs/PACKAGING.md](docs/PACKAGING.md#releasing)).
- Never commit credentials, tokens, `.env`, server addresses, personal media names or screenshots of personal libraries.
- Focused commits, imperative messages, no force-push to shared branches.
