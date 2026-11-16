# Architecture

NOCTURNE is a Cargo workspace. The app is the only shipped binary (`nocturne`; today the legacy GPUI `crates/app`, the Iced `crates/ui` after the cut-over).

> **In transition (branch `iced-migration`).** The UI is moving from GPUI to Iced 0.14 (`crates/ui`), and the
> single-server model is growing into one catalog over local folders and several Jellyfin servers. Target design:
> [SOURCES.md](SOURCES.md) and [DECISIONS.md](DECISIONS.md); plan and progress: [dev/ICED_MIGRATION.md](dev/ICED_MIGRATION.md).
> Sections below that mention GPUI describe the legacy `crates/app` until the cut-over.

| Crate | Role | Writes to server? |
|---|---|---|
| `crates/jellyfin` | Async read-only Jellyfin client: libraries, items, images, stream URLs. GET only, enforced by `tests/readonly.rs` | never |
| `crates/jellyfin-user` | User-scope calls: login, Quick Connect, logout, playback reporting, played / favourite, own password, own profile image, display language, own playlists, SyncPlay (REST, WebSocket, clock sync, state machine) | yes, user scope |
| `crates/jellyfin-admin` | Admin reads and writes. Destructive calls need a `Confirmed` token; live tests guarded by `TestServerGuard` | yes, admin |
| `crates/session` | Tokens in the system keyring (Secret Service): the active login per server and one per remembered account; server URLs in `servers.toml`, account names in `accounts.toml`; atomic writes | n/a |
| `crates/player` | mpv playback: `EmbeddedPlayer` (libmpv in a Wayland subsurface) and `Player` (mpv child process over JSON IPC). libmpv is loaded at run time (`mpv_ffi.rs`); `deps.rs` probes libmpv / the mpv binary | n/a |
| `crates/core` | UI-framework-free logic (settings, navigation, list/detail/admin/account state, playback logic, source layer, caches, bridge) shared by front ends | via the crates above |
| `crates/local` | Local library: scan, naming rules, index, `.nfo`/sidecar metadata, overrides, local resume state; never writes to media folders (in progress) | never |
| `crates/ui` | Iced application (in progress; becomes `crates/app` at the cut-over) | via the crates above |
| `crates/app` | Legacy GPUI application: views, navigation, admin UI; removed at the cut-over | via the crates above |

`player` has no UI-framework or Jellyfin dependencies; `core`, `local` and the Jellyfin crates have no UI dependencies.

## Data flow

1. The UI (on the main thread; an Iced `update`, or a GPUI view in the legacy crate) asks for data and does not block.
2. The request runs on the tokio bridge (below), which calls `nocturne-jellyfin` over HTTPS (reqwest, rustls).
3. The result is applied back on the UI thread and the view re-renders.
4. Images go through `imgcache.rs`: blurhash placeholder first, then bytes from a size-capped disk LRU cache,
   fetched with retry on a miss. Item data is kept in `cache.rs`.
5. Navigation state (`nav.rs`) is a history stack of pages, which gives back / forward and breadcrumbs.

## Tokio bridge

`nocturne-jellyfin` is async; the UI framework runs its own executor on the main thread. `crates/core/src/bridge.rs` keeps one
process-wide multi-thread tokio runtime (two worker threads) and hands futures to it with `bridge::spawn`. The
returned handle is a plain future, so an Iced `Task::perform` (a GPUI `cx.spawn` task in the legacy crate) awaits it and
updates the state on the UI thread (`bridge::run`). No channels, no `block_on`, no nested runtimes.

## Playback and in-window video

Neither UI framework has a native video surface. `EmbeddedPlayer` creates a `wl_subsurface` placed below the (transparent) app
window, renders with libmpv's OpenGL render API through EGL, and the UI draws the controls on top. Hardware decoding
is on by default. With Iced the player page leaves a transparent hole whose layout bounds drive `set_bounds` (spike:
`spikes/iced-embed`). libmpv is `dlopen`ed, so when it is missing the app still starts: playback falls back to the
external `mpv` binary, and if that is missing too the user gets an install hint. This uses raw Wayland handles and about
300 lines of `unsafe` glue; in the legacy GPUI build it depends on GPUI internals, which is why GPUI is version-pinned.
libmpv's Vulkan context is deliberately not used: a second Vulkan instance next to the UI's crashed on NVIDIA (details and
measurements in [video-embedding.md](video-embedding.md), written against GPUI).

Where in-window video is unavailable (X11, or `NOCTURNE_VIDEO=external`), `Player` runs the `mpv` binary as a child
process with its own window, controlled over a private unix socket. A driver crash there cannot take the app down.
Both backends share one API. Buffering watermarks are described in [buffering.md](buffering.md).

The pure player rules (`player_logic.rs`, `video.rs`) and `player_mpris.rs` (media keys over D-Bus with zbus) live in
`crates/core`; the player UI is `player_view.rs` / `player_ui.rs` in the legacy `crates/app` and its Iced replacement in
`crates/ui`. Playback reporting is driven from the root tick (`sync.rs`), not from the player, so one code path
handles all progress, played and stopped reports.

## Watch together (SyncPlay)

`jellyfin-user/src/syncplay/` holds everything that is not UI: `model.rs` (typed messages and their decoding),
`time.rs` (clock offset, lowest-delay sample), `engine.rs` (a pure state machine that takes `now` and returns what to do:
schedule a command, load a queue, report Ready, correct drift), `rest.rs` (calls; writes take `UserConfirmed`) and `ws.rs`
(reqwest does the HTTP upgrade, tungstenite only frames; reconnect with backoff). The app side is `crates/core/src/syncplay.rs` (pure rules: what the screen may offer, ordering the group's items, stall detection) plus the UI glue (legacy GPUI: `crates/app/src/app/ui_syncplay.rs`, the `Sp` state on `Root`, a 50 ms poll loop on the UI thread that drains the socket channel, the screen and the pill; the Iced version is being ported). While in a group the
player's transport methods push `GroupReq` into an outbox instead of acting; `Root` sends them to the group and applies the
group's commands through the `group_*` methods, which never route back. See [syncplay.md](syncplay.md).

## Write gating

All server changes funnel through one master setting, `allow_server_writes` (default off, persisted in
`settings.toml`).

- Progress reporting, played / favourite and password change are skipped (and their controls dimmed) when it is off.
- The admin UI is view only when it is off; when on, every admin action goes through the single confirmation
  choke point (`on_confirm` in the admin screen), which mints the `Confirmed` token the destructive `jellyfin-admin` calls require.
- Developer API-key mode refuses writes regardless of the setting.
- `crates/jellyfin` cannot write at all.

The user-facing matrix is in [WRITES.md](WRITES.md).

## Settings and sessions

`crates/core/src/settings.rs` reads and writes versioned TOML under `$XDG_CONFIG_HOME/nocturne/` atomically on every change.
Secrets never go there: tokens live in the keyring (`NOCTURNE_KEYRING_SERVICE` overrides the service name so tests
never touch real entries).
