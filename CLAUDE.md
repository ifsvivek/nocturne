# NOCTURNE: context for coding agents

Read this first, then [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Other agents
(Codex, Cursor, Copilot, ...) see [AGENTS.md](AGENTS.md); the rules are the same.

NOCTURNE is an unofficial, native Linux desktop client for Jellyfin, written in Rust with GPUI 0.2.2, playing video
inside the app window through libmpv. Wayland first, X11 via an external mpv window. GPL-3.0-or-later. Linux only:
do not add Windows or macOS code paths. Not affiliated with the Jellyfin project.

## Crates (Cargo workspace, edition 2024, resolver 3, MSRV 1.89)

| Crate | Responsibility |
|---|---|
| `crates/jellyfin` (`nocturne-jellyfin`) | Async client, **GET only**: libraries, items, images, stream URLs, `Config` (`NOCTURNE_JF_URL`/`NOCTURNE_JF_TOKEN`, optional `.env`). |
| `crates/jellyfin-user` | User-scope calls: login, Quick Connect, playback reporting, played/favourite, own playlists, own password. `UserConfirmed` token. |
| `crates/jellyfin-admin` | Admin reads and every admin write; `Confirmed` token; `guard::TestServerGuard` for live tests. |
| `crates/session` | Token in the system keyring (Secret Service, no file fallback); server URLs in `servers.toml`. |
| `crates/player` | `EmbeddedPlayer` (libmpv in a Wayland subsurface) and `Player` (mpv child process over JSON IPC). No GPUI/Jellyfin deps. |
| `crates/app` (`nocturne-app`, binary `nocturne`) | GPUI views, navigation, settings, caches, admin UI, MPRIS. Only crate with UI code. |

Other directories: `docs/` (see below), `packaging/` (PKGBUILD, desktop/AppStream files, `install.sh`), `scripts/`,
`website/` (SvelteKit static site, own README), `vendor/xattr` (patched crate, leave alone).

## Build, run, test

```sh
cargo build                          # stable Rust 1.89+, libmpv dev files (`pkg-config --modversion mpv`)
cargo run -p nocturne-app            # reads an optional git-ignored .env (template: .env.example)
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked      # offline tests only; live tests are #[ignore]
scripts/ci-checks.sh                 # full gate: fmt, clippy, tests, cargo deny, secret scan of tree and history
```

Run all of fmt, clippy and test before committing. Unit tests live next to the code (`#[cfg(test)] mod tests`, mostly
in `crates/app/src`, where GPUI-free state/logic modules exist so they test without a window); integration tests are in
`crates/*/tests/` (`readonly.rs`, `segments.rs`, `smoke.rs`, `buffering.rs`, `windowed.rs`, `live.rs`).

### Live tests against a throwaway Jellyfin

`scripts/test-jellyfin.sh {start|stop|reset|status}` starts the installed `jellyfin` on `127.0.0.1:8097` with all state
under `/tmp/nocturne-jf-test` (needs `jellyfin`, `ffmpeg`, `curl`, `jq`). Credentials (admin API key, users `alice` and
`bob`) are generated randomly at first start and written to `test.env` (mode 600) in that directory.

```sh
scripts/test-jellyfin.sh start
NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-admin -- --ignored --test-threads=1
NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-user  -- --ignored --test-threads=1
scripts/test-jellyfin.sh stop        # `reset` deletes everything
```

To run the app against it, set `NOCTURNE_JF_URL=http://127.0.0.1:8097` plus isolated `XDG_CONFIG_HOME`,
`XDG_DATA_HOME` and `NOCTURNE_KEYRING_SERVICE` so your real settings and keyring entries are untouched.
`crates/jellyfin/tests/live.rs` and `crates/player/tests/live.rs` are read-only checks that use `NOCTURNE_JF_URL` and
`NOCTURNE_JF_TOKEN`; point them only at a server you do not mind being read.

## Safety rules (apply to humans and agents)

1. **Read-only by default.** The user setting `allow_server_writes` (default off, `crates/app/src/settings.rs`) is the
   master switch for everything that changes the server: playback progress, played/favourite, own password and all
   admin actions. `sync_progress` (default off) additionally gates progress reporting and only counts while writes are on.
   The admin UI is view-only with writes off. Developer API-key mode (`NOCTURNE_JF_TOKEN`) is strictly read-only
   regardless of the setting. Matrix: [docs/WRITES.md](docs/WRITES.md).
2. **`crates/jellyfin` never writes.** `crates/jellyfin/tests/readonly.rs` fails if a write verb appears in `src/`.
   Writes go in `jellyfin-user` or `jellyfin-admin`.
3. **Confirmation tokens.** Destructive admin calls take `Confirmed`, minted only in `Admin::on_confirm`
   (`crates/app/src/admin/mod.rs`) after a dialog. The user's own password change takes `UserConfirmed`, minted only
   after its dialog in `crates/app/src/account/mod.rs`. Never mint either elsewhere or in new UI shortcuts.
4. **Never run anything that writes against a real server.** Writing tests use `TestServerGuard::connect()`, which
   refuses unless the URL comes from the file named by `NOCTURNE_JF_TEST_ENV`, is loopback port 8097, differs from the
   configured real server, and the server's data path is under `/tmp/`. Do not weaken or bypass it. Never type real
   credentials into an app window under test.
5. **Never commit** credentials, tokens, `.env`, server addresses, personal media titles, personal paths, or
   screenshots/recordings of a personal library. Screenshots must come from a generated demo library. Never print
   token values in logs or test output (live tests report booleans and counts only).
6. No force-push to shared branches. Do not publish releases, deploy the site or change repository settings unless a
   maintainer asks.
7. Machine-local or personal notes belong in git-ignored files (`CLAUDE.local.md`, `.env`), never in tracked files.

## Architecture facts that are easy to break

- **GPUI is pinned** (`gpui = "=0.2.2"`), as are `libmpv2` and the `wayland-*` crates. The embedding reads GPUI's raw
  `wl_display`/`wl_surface` handles; re-test video on any bump.
- **Tokio bridge.** GPUI has its own executor; `crates/app/src/bridge.rs` holds one process-wide multi-thread tokio
  runtime. Use `bridge::spawn` / `bridge::run` from `cx.spawn` tasks; no `block_on`, no nested runtimes.
  Responses carry the page/epoch they were requested for and are dropped if the user moved on.
- **In-window video.** `EmbeddedPlayer` creates a `wl_subsurface` below a transparent GPUI window and renders with
  libmpv's **OpenGL render API through EGL**; GPUI draws controls on top. **Never create an mpv Vulkan / `gpu-next`
  context in the app process**: a second VkInstance next to GPUI's Vulkan renderer segfaulted on NVIDIA
  ([docs/video-embedding.md](docs/video-embedding.md)).
- **External fallback.** `Player` runs `mpv` as a child process over a private unix socket, so a driver crash cannot
  take the app down. Used on non-Wayland sessions, if embedded setup fails, or with `NOCTURNE_VIDEO=external`
  (`embedded` forces the other way; logic in `crates/app/src/video.rs`). `NOCTURNE_VO` picks the external window's
  video output (default `gpu-next`, fine there because it is a separate process).
- **Session and settings.** Tokens live only in the keyring (`crates/session`, service name overridable by
  `NOCTURNE_KEYRING_SERVICE`); settings are versioned TOML under `$XDG_CONFIG_HOME/nocturne/`, written atomically, with
  no secrets. 401 mid-session returns to sign-in; unreachable server shows a banner and retries with backoff
  (`Root::watch_connection`).
- **Playback reporting** (start/progress/stop) is driven from `Root::tick` (`crates/app/src/app.rs`, `sync_tick`
  with the pure state machine in `sync.rs`), not from the player, so there is one code path. Release profile keeps
  `panic = "unwind"` on purpose.
- **Theme.** Colours and sizes come from `crates/app/src/theme.rs`; no raw hex elsewhere ([docs/DESIGN.md](docs/DESIGN.md)).

## Conventions

- Rust 2024, `cargo fmt` defaults, clippy clean with `-D warnings`. Smallest change that works; comment non-obvious
  constraints, not what the code says. Keep pure logic in GPUI-free modules with unit tests beside them.
- Server-facing UI copy avoids hard-coding the product name where practical (Jellyfin is the first source, not the last).
- New dependencies must pass `cargo deny check licenses`; then run `scripts/gen-notices.py > THIRD_PARTY_NOTICES.md`.
- **Add an admin page:** add a `Section` variant, its label, and entries in `ALL` and `SIDEBAR` in
  `crates/app/src/admin/state.rs`; add a `Msg`/`Data` field there for its data; add the endpoint to
  `jellyfin-admin` (`client.rs` for reads, `write.rs` for writes, `model.rs` for types); build its `Model` in
  `admin/model.rs` (add a match arm in `Admin::model`) and fetch it from `Admin::load` in `admin/mod.rs`. Any write is an
  `Act` variant in `admin/act.rs` that runs only through the confirmation dialog and `on_confirm`. Add a live test in
  `crates/jellyfin-admin/tests/live.rs` (throwaway server only).
- **Add a setting:** add the field (with a serde default) to `Settings` in `crates/app/src/settings.rs`, a `Row` in
  `Settings::rows()` and a matching arm in `Settings::cycle()` (row index = match arm), bump the settings version if
  defaults change meaning, and add a unit test. Anything that writes to the server must also be gated by
  `allow_server_writes`.
- Commits: imperative, descriptive subject, one concern each (`Fix sort menu painting under the grid`); no force-push to shared branches. Keep `docs/dev/STATUS.md` and `CHANGELOG.md` current when behaviour changes.

## Documentation map

[ARCHITECTURE](docs/ARCHITECTURE.md) (crates, data flow) | [WRITES](docs/WRITES.md) (what writes and when) |
[video-embedding](docs/video-embedding.md) (why the video path looks like this) | [buffering](docs/buffering.md) |
[DESIGN](docs/DESIGN.md) | [FEATURES](docs/FEATURES.md) | [ROADMAP](docs/ROADMAP.md) | [QA_REPORT](docs/QA_REPORT.md) |
[PACKAGING](docs/PACKAGING.md) | [dev/STATUS](docs/dev/STATUS.md) (release checklist, known follow-ups) |
[SECURITY](SECURITY.md).
