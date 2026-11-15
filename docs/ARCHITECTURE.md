# Architecture

NOCTURNE is a Cargo workspace of six crates. The app is the only binary (`nocturne`).

| Crate | Role | Writes to server? |
|---|---|---|
| `crates/jellyfin` | Async read-only Jellyfin client: libraries, items, images, stream URLs. GET only, enforced by `tests/readonly.rs` | never |
| `crates/jellyfin-user` | User-scope calls: login, Quick Connect, logout, playback reporting, played / favourite, own password, own profile image, display language, own playlists, SyncPlay (REST, WebSocket, clock sync, state machine) | yes, user scope |
| `crates/jellyfin-admin` | Admin reads and writes. Destructive calls need a `Confirmed` token; live tests guarded by `TestServerGuard` | yes, admin |
| `crates/session` | Tokens in the system keyring (Secret Service): the active login per server and one per remembered account; server URLs in `servers.toml`, account names in `accounts.toml`; atomic writes | n/a |
| `crates/player` | mpv playback: `EmbeddedPlayer` (libmpv in a Wayland subsurface) and `Player` (mpv child process over JSON IPC) | n/a |
| `crates/app` | GPUI application: views, navigation, settings, caches, admin UI | via the crates above |

`player` has no GPUI or Jellyfin dependencies; the Jellyfin crates have no UI dependencies.

## Data flow

1. The UI (a GPUI view on the main thread) asks for data and does not block.
2. The request runs on the tokio bridge (below), which calls `nocturne-jellyfin` over HTTPS (reqwest, rustls).
3. The result is applied back to the view on the UI thread and the view re-renders.
4. Images go through `imgcache.rs`: blurhash placeholder first, then bytes from a size-capped disk LRU cache,
   fetched with retry on a miss. Item data is kept in `cache.rs`.
5. Navigation state (`nav.rs`) is a history stack of pages, which gives back / forward and breadcrumbs.

## Tokio bridge

`nocturne-jellyfin` is async; GPUI runs its own executor on the main thread. `crates/app/src/bridge.rs` keeps one
process-wide multi-thread tokio runtime (two worker threads) and hands futures to it with `bridge::spawn`. The
returned handle is a plain future, so a GPUI `cx.spawn` task awaits it and updates the entity on the UI thread
(`bridge::run`). No channels, no `block_on`, no nested runtimes.

## Playback and in-window video

GPUI has no native video surface. `EmbeddedPlayer` creates a `wl_subsurface` placed below the (transparent) GPUI
window, renders with libmpv's OpenGL render API through EGL, and GPUI draws the controls on top. Hardware decoding
is on by default. This uses raw Wayland handles and about 300 lines of `unsafe` glue that depends on GPUI
internals, which is why GPUI is version-pinned. libmpv's Vulkan context is deliberately not used: a second Vulkan
instance next to GPUI's crashed on NVIDIA (details and measurements in [video-embedding.md](video-embedding.md)).

Where in-window video is unavailable (X11, or `NOCTURNE_VIDEO=external`), `Player` runs the `mpv` binary as a child
process with its own window, controlled over a private unix socket. A driver crash there cannot take the app down.
Both backends share one API. Buffering watermarks are described in [buffering.md](buffering.md).

The player UI (`player_view.rs`, `player_ui.rs`, `player_logic.rs`) sits in the app; `player_mpris.rs` exposes media
keys over D-Bus (zbus). Playback reporting is driven from `Root::tick`, not from the player, so one code path
handles all progress, played and stopped reports.

## Watch together (SyncPlay)

`jellyfin-user/src/syncplay/` holds everything that is not UI: `model.rs` (typed messages and their decoding),
`time.rs` (clock offset, lowest-delay sample), `engine.rs` (a pure state machine that takes `now` and returns what to do:
schedule a command, load a queue, report Ready, correct drift), `rest.rs` (calls; writes take `UserConfirmed`) and `ws.rs`
(reqwest does the HTTP upgrade, tungstenite only frames; reconnect with backoff). The app side is `app/ui_syncplay.rs`
(the `Sp` state on `Root`, a 50 ms poll loop on the UI thread that drains the socket channel, the screen and the pill) and
`syncplay.rs` (pure rules: what the screen may offer, ordering the group's items, stall detection). While in a group the
player's transport methods push `GroupReq` into an outbox instead of acting; `Root` sends them to the group and applies the
group's commands through the `group_*` methods, which never route back. See [syncplay.md](syncplay.md).

## Write gating

All server changes funnel through one master setting, `allow_server_writes` (default off, persisted in
`settings.toml`).

- Progress reporting, played / favourite and password change are skipped (and their controls dimmed) when it is off.
- The admin UI is view only when it is off; when on, every admin action goes through the single confirmation
  choke point `Admin::on_confirm`, which mints the `Confirmed` token the destructive `jellyfin-admin` calls require.
- Developer API-key mode refuses writes regardless of the setting.
- `crates/jellyfin` cannot write at all.

The user-facing matrix is in [WRITES.md](WRITES.md).

## Settings and sessions

`settings.rs` reads and writes versioned TOML under `$XDG_CONFIG_HOME/nocturne/` atomically on every change.
Secrets never go there: tokens live in the keyring (`NOCTURNE_KEYRING_SERVICE` overrides the service name so tests
never touch real entries).
