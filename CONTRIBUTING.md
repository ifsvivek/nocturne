# Contributing

Thanks for helping. NOCTURNE is an unofficial desktop client for Jellyfin and local media (Linux first); read
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/WRITES.md](docs/WRITES.md) first.

## Ground rules

- **Never test against a server that holds data you care about.** Anything that writes (progress, played, favourites, admin actions, passwords) must be tested only on a throwaway server (below).
- `crates/jellyfin` stays GET-only. A test (`crates/jellyfin/tests/readonly.rs`) fails if a write verb appears. Writes belong in `crates/jellyfin-user` or `crates/jellyfin-admin` and must respect the `allow_server_writes` switch; destructive admin calls need the `Confirmed` token minted by the confirmation dialog.
- No secrets, tokens, server addresses, media titles or personal paths in code, tests, logs or issues.
- Linux first. macOS and Windows are planned ([docs/PLATFORMS.md](docs/PLATFORMS.md)): keep OS-specific code inside the platform seams listed there and do not add macOS/Windows implementations before that phase starts.
- Keep changes small and focused; one concern per pull request.

## Setup

```sh
git clone https://github.com/ifsvivek/nocturne && cd nocturne
cargo build                      # needs stable Rust 1.89+; libmpv is only needed at run time (loaded with dlopen)
cargo run -p nocturne-app        # the app (binary: nocturne); debug builds read an optional git-ignored .env (see .env.example)
cargo test --workspace           # offline tests
```

Before opening a pull request run `scripts/ci-checks.sh` (fmt, clippy `-D warnings`, tests, `cargo deny`, secret scan). At minimum `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` must be clean.

## Throwaway Jellyfin for live tests

`scripts/test-jellyfin.sh {start|stop|reset|status}` runs the installed `jellyfin` on `127.0.0.1:8097` (set `NOCTURNE_JF_TEST_PORT` and `NOCTURNE_JF_TEST_DIR` to run a second instance; the live tests read the same variable) with all state under `/tmp/nocturne-jf-test`, loopback only, discovery off. It creates an admin with a random password, an API key, users `alice` and `bob`, and a library of generated ffmpeg clips. URL and token go to `/tmp/nocturne-jf-test/test.env` (mode 600). Needs `jellyfin`, `ffmpeg`, `curl` and `jq`.

```sh
scripts/test-jellyfin.sh start
NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-admin -- --ignored
scripts/test-jellyfin.sh stop     # or `reset` to delete everything
```

The admin and user crates' live tests refuse to run unless the URL is the loopback throwaway server (`guard::TestServerGuard`). When running the app against it, use `scripts/run-isolated.sh target/debug/nocturne` (add `NOCTURNE_JF_URL=http://127.0.0.1:8097`): it runs from a directory outside the repository (debug builds read `.env` from the current directory or any ancestor) with throwaway `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `NOCTURNE_KEYRING_SERVICE`, so nothing touches your real settings, local index or keyring entries.

Debug builds have two hooks for checking the UI: `NOCTURNE_SNAP=/tmp/x.png` (and `NOCTURNE_SNAP_AFTER_MS=1500,4000` for several) saves the app window's own pixels, and `NOCTURNE_PREVIEW=<screen>` opens a screen with canned or throwaway-server data. Capture only the app window, never the whole screen, and never commit captures of a personal library.

## Code style

Rust 2024 edition, `cargo fmt` defaults. Colours and sizes come from `crates/app/src/theme.rs`; do not add raw hex elsewhere (see [docs/DESIGN.md](docs/DESIGN.md)). Prefer the smallest change that works; comment non-obvious constraints (for example window-handle details the video embedding depends on). Keep pure logic in `crates/core` or `crates/local` with unit tests; `crates/app` is the only crate with UI code. Local media files are never modified: scanning only reads.

## Dependencies and licences

The project is GPL-3.0-or-later. New dependencies must pass `cargo deny check licenses`; regenerate `THIRD_PARTY_NOTICES.md` with `scripts/gen-notices.py > THIRD_PARTY_NOTICES.md`.

## Commits and pull requests

Imperative, descriptive commit messages (`Fix sort menu painting under the grid`). Describe what you tested and on which session type (Wayland or X11). Contributions are licensed under GPL-3.0-or-later. Security problems: see [SECURITY.md](SECURITY.md), not a public issue. Everyone is expected to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
