# NOCTURNE: context for coding agents

Read this first, then [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Other agents
(Codex, Cursor, Copilot, ...) see [AGENTS.md](AGENTS.md); the rules are the same.

NOCTURNE is a native desktop media center written in Rust: one cinematic, searchable library over local media
folders and any number of Jellyfin servers, playing video inside the app window through libmpv. Jellyfin is a
first-class source but never required. GPL-3.0-or-later. Not affiliated with the Jellyfin project.

## Direction (in progress on branch `iced-migration`; see `docs/dev/ICED_MIGRATION.md`)

- **UI framework: Iced 0.14** (wgpu, winit), replacing GPUI. The new UI is `crates/ui`; the GPUI crate `crates/app`
  is the legacy reference and is deleted at the cut-over (then `crates/ui` becomes `crates/app`, binary `nocturne`).
  Do not add features to the GPUI crate. Iced APIs: [docs/dev/ICED_NOTES.md](docs/dev/ICED_NOTES.md) (verified
  against 0.14 sources; never use older Iced APIs from memory).
- **Sources**: local library + several Jellyfin servers aggregated into one catalog
  ([docs/SOURCES.md](docs/SOURCES.md)).
- **Platforms**: Linux (Arch first) is primary and the only one built today; other Linux formats, macOS and Windows
  follow ([docs/PLATFORMS.md](docs/PLATFORMS.md)). Keep OS-specific code inside the platform seams listed there,
  `#[cfg(target_os = ...)]` only within them. Do not add macOS/Windows implementations before that phase starts.

## Crates (Cargo workspace, edition 2024, resolver 3, MSRV 1.89)

| Crate | Responsibility |
|---|---|
| `crates/jellyfin` (`nocturne-jellyfin`) | Async client, **GET only**: libraries, items, images, stream URLs, `Config` (`NOCTURNE_JF_URL`/`NOCTURNE_JF_TOKEN`, optional `.env`). |
| `crates/jellyfin-user` | User-scope calls: login, Quick Connect, playback reporting, played/favourite, own playlists, own password, SyncPlay. `UserConfirmed` token. |
| `crates/jellyfin-admin` | Admin reads and every admin write; `Confirmed` token; `guard::TestServerGuard` for live tests. |
| `crates/session` | Tokens in the system keyring (Secret Service, no file fallback); server URLs and accounts in TOML. |
| `crates/player` | `EmbeddedPlayer` (libmpv in a Wayland subsurface) and `Player` (mpv child process over JSON IPC). libmpv is **loaded at run time** (`mpv_ffi.rs`); `deps.rs` probes libmpv / the mpv binary and gives per-distro install hints. No UI/Jellyfin deps. |
| `crates/core` (`nocturne-core`) | UI-framework-free app logic: settings, navigation, media model (`source.rs`), caches, sync/reporting rules, player logic, admin/account state. Unit-tested without a window. |
| `crates/local` (`nocturne-local`) | Local library: scanner, naming rules, index, metadata from `.nfo`/sidecars, overrides, local resume state. Never writes to media folders. (milestone 1, in progress) |
| `crates/ui` (`nocturne-ui`, bin `nocturne-iced`) | The Iced UI (in progress). Only crate with Iced code. |
| `crates/app` (`nocturne-app`, bin `nocturne`) | Legacy GPUI UI, removed at the cut-over. |

Other directories: `docs/` (see below), `packaging/` (PKGBUILD, desktop/AppStream files, `install.sh`), `scripts/`,
`website/` (SvelteKit static site, own README), `spikes/` (throwaway experiments, not workspace members),
`vendor/xattr` (patched crate needed only by GPUI; removed with it).

## Build, run, test

```sh
cargo build                          # stable Rust 1.89+; libmpv only needed at run time (dlopen)
cargo run -p nocturne-ui             # the Iced UI; `-p nocturne-app` runs the legacy GPUI UI
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked      # offline tests only; live tests are #[ignore]
scripts/ci-checks.sh                 # full gate: fmt, clippy, tests, cargo deny, secret scan of tree and history
```

Run all of fmt, clippy and test before committing. Unit tests live next to the code (`#[cfg(test)] mod tests`);
integration tests are in `crates/*/tests/`.

### Live tests against a throwaway Jellyfin

`scripts/test-jellyfin.sh {start|stop|reset|status}` starts the installed `jellyfin` on `127.0.0.1:8097` with all state
under `/tmp/nocturne-jf-test` (needs `jellyfin`, `ffmpeg`, `curl`, `jq`). Credentials (admin API key, users `alice` and
`bob`) are generated randomly at first start and written to `test.env` (mode 600) in that directory.
`scripts/make-demo-library.sh build` creates a synthetic demo library for screenshots.

```sh
scripts/test-jellyfin.sh start
NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-admin -- --ignored --test-threads=1
NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-user  -- --ignored --test-threads=1
scripts/test-jellyfin.sh stop        # `reset` deletes everything
```

To run the app for testing, ALWAYS use `scripts/run-isolated.sh target/debug/nocturne-iced` (add
`NOCTURNE_JF_URL=http://127.0.0.1:8097` for the throwaway server). It runs from a directory outside the repo with
blank server variables, throwaway XDG dirs and a test keyring service. Never launch a debug build from inside the repo
tree: debug builds read `.env` from the current directory or any ancestor, and the maintainer's `.env` points at a
real server. Screenshots: capture the app window only, never the whole screen.

## Safety rules (apply to humans and agents)

1. **Read-only by default.** The user setting `allow_server_writes` (default off, `crates/core/src/settings.rs`) is the
   master switch for everything that changes a server: playback progress, played/favourite, own password and all
   admin actions. `sync_progress` (default off) additionally gates progress reporting and only counts while writes are on.
   The admin UI is view-only with writes off. Developer API-key mode (`NOCTURNE_JF_TOKEN`) is strictly read-only
   regardless of the setting. With several servers, a write goes only to the item's own server. Matrix:
   [docs/WRITES.md](docs/WRITES.md).
2. **`crates/jellyfin` never writes.** `crates/jellyfin/tests/readonly.rs` fails if a write verb appears in `src/`.
   Writes go in `jellyfin-user` or `jellyfin-admin`.
3. **Local media is never modified.** `crates/local` opens media files read-only and never renames, moves or deletes
   them; it writes only its own index/state files under `$XDG_DATA_HOME/nocturne/`.
4. **Confirmation tokens.** Destructive admin calls take `Confirmed`, minted only in the admin confirmation handler
   (`on_confirm`) after a dialog. The user's own password change takes `UserConfirmed`, minted only after its dialog in
   the account module. Never mint either elsewhere or in new UI shortcuts.
5. **Never run anything that writes against a real server.** Writing tests use `TestServerGuard::connect()`, which
   refuses unless the URL comes from the file named by `NOCTURNE_JF_TEST_ENV`, is loopback port 8097, differs from the
   configured real server, and the server's data path is under `/tmp/`. Do not weaken or bypass it. Never type real
   credentials into an app window under test.
6. **Never commit** credentials, tokens, `.env`, server addresses, personal media titles, personal paths, or
   screenshots/recordings of a personal library or desktop. Screenshots must come from a generated demo library.
   Never print token values in logs or test output (live tests report booleans and counts only).
7. No force-push to shared branches. Do not publish releases, deploy the site, push, or change repository settings
   unless a maintainer asks.
8. Machine-local or personal notes belong in git-ignored files (`CLAUDE.local.md`, `.env`, `nocturne.local.*`),
   never in tracked files.

## Architecture facts that are easy to break

- **Pins.** `iced = 0.14`, `wayland-*` crates are `=`-pinned in `crates/player`; the embedding reads the window's raw
  `wl_display`/`wl_surface`. Re-test in-window video on any bump.
- **Tokio bridge.** `nocturne_core::bridge` holds one process-wide multi-thread tokio runtime that owns every reqwest
  client. All network futures go through `bridge::spawn` (wrapped in an Iced `Task::perform`); no `block_on`, no
  nested runtimes. Responses carry the page/epoch they were requested for and are dropped if the user moved on.
- **In-window video.** `EmbeddedPlayer` creates a `wl_subsurface` below the transparent app window and renders with
  libmpv's **OpenGL render API through EGL**; the UI draws controls on top and leaves a transparent hole whose
  bounds drive `set_bounds`. **Never create an mpv Vulkan / `gpu-next` context in the app process**: a second
  VkInstance next to the UI's Vulkan renderer segfaulted on NVIDIA ([docs/video-embedding.md](docs/video-embedding.md)).
  Do not force wgpu's GL backend (it darkens the video under the hole).
- **libmpv is optional at run time.** Missing libmpv -> external `mpv`; missing both -> an actionable message with
  `nocturne_player::install_hint()`; browsing keeps working. Never re-add a link-time libmpv dependency.
- **External fallback.** `Player` runs `mpv` as a child process over a private unix socket, so a driver crash cannot
  take the app down. Used on non-Wayland sessions, if embedded setup fails, or with `NOCTURNE_VIDEO=external`
  (`embedded` forces the other way; logic in `crates/core/src/video.rs`). `NOCTURNE_VO` picks the external window's
  video output (default `gpu-next`, fine there because it is a separate process).
- **Session and settings.** Tokens live only in the keyring (`crates/session`, service name overridable by
  `NOCTURNE_KEYRING_SERVICE`); settings are versioned TOML under `$XDG_CONFIG_HOME/nocturne/`, written atomically,
  with no secrets. 401 mid-session returns that source to sign-in; an unreachable server shows a banner and retries
  with backoff while other sources keep working.
- **Playback reporting** (start/progress/stop) is driven from the root tick with the pure state machine in
  `crates/core/src/sync.rs`, not from the player, so there is one code path. Release profile keeps
  `panic = "unwind"` on purpose.
- **Theme.** Colours and sizes come from the UI crate's `theme.rs`; no raw hex elsewhere ([docs/DESIGN.md](docs/DESIGN.md)).

## Conventions

- Rust 2024, `cargo fmt` defaults, clippy clean with `-D warnings`. Smallest change that works; comment non-obvious
  constraints, not what the code says. Keep pure logic in `crates/core` (or `crates/local`) with unit tests beside it;
  view code stays thin.
- UI copy avoids hard-coding "Jellyfin" where a source-neutral word works.
- New dependencies must pass `cargo deny check licenses`; then run `scripts/gen-notices.py > THIRD_PARTY_NOTICES.md`.
  Weigh binary size and idle cost before adding one ([docs/PLATFORMS.md](docs/PLATFORMS.md) footprint rules).
- **Add an admin page:** add a `Section` variant, its label, and entries in `ALL` and `SIDEBAR` in
  `crates/core/src/admin/state.rs`; add a `Msg`/`Data` field there for its data; add the endpoint to
  `jellyfin-admin` (`client.rs` for reads, `write.rs` for writes, `model.rs` for types); build its `Model` in
  `crates/core/src/admin/model.rs` and fetch it from the admin screen's load. Any write is an `Act` variant in
  `crates/core/src/admin/act.rs` that runs only through the confirmation dialog and `on_confirm`. Add a live test in
  `crates/jellyfin-admin/tests/live.rs` (throwaway server only).
- **Add a setting:** add the field (with a serde default) to `Settings` in `crates/core/src/settings.rs`, a `Row` in
  `Settings::rows()` and a matching arm in `Settings::cycle()` (row index = match arm), bump the settings version if
  defaults change meaning, and add a unit test. Anything that writes to a server must also be gated by
  `allow_server_writes`.
- Commits: imperative, descriptive subject, one concern each; no AI attribution trailers. Keep `docs/dev/STATUS.md`
  and `CHANGELOG.md` current when behaviour changes.

## Agent workflow (maintainer's setup)

- **Master agent: Claude Opus.** Owns architecture and technical decisions, planning, delegation, integration, code
  review and final verification. It does not routinely implement features.
- **Workers: Claude Sonnet**, at most three active (maintainer raised the cap from two on 2026-11-16; temporarily five until 0.3 ships). Each gets a bounded task with explicit file
  ownership (no overlapping files; parallel workers use separate git worktrees), implements and tests it, and reports
  results. Workers do not change the architecture; they report architectural problems to the master.
- The master reviews and validates every worker change before it is committed to the shared branch. Keep usage
  economical: no redundant research (reuse `docs/dev/*`), narrow tasks, no agents for trivial work.

## Documentation map

[ARCHITECTURE](docs/ARCHITECTURE.md) (crates, data flow) | [DECISIONS](docs/DECISIONS.md) (why things are the way they
are) | [SOURCES](docs/SOURCES.md) (local + multi-server catalog) | [PLATFORMS](docs/PLATFORMS.md) (targets, packaging,
footprint) | [WRITES](docs/WRITES.md) | [video-embedding](docs/video-embedding.md) | [buffering](docs/buffering.md) |
[DESIGN](docs/DESIGN.md) | [FEATURES](docs/FEATURES.md) | [ROADMAP](docs/ROADMAP.md) | [QA_REPORT](docs/QA_REPORT.md) |
[PACKAGING](docs/PACKAGING.md) | [dev/STATUS](docs/dev/STATUS.md) | [dev/ICED_MIGRATION](docs/dev/ICED_MIGRATION.md) |
[SECURITY](SECURITY.md).
