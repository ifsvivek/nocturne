# Architecture

NOCTURNE is a Cargo workspace. The only shipped binary is `nocturne` (package `nocturne-app`, `crates/app`), an
[Iced](https://iced.rs/) 0.14 application. Everything that does not need a window lives in framework-free crates so it
can be tested without one.

| Crate | Role | Writes to a server? |
|---|---|---|
| `crates/jellyfin` | Async read-only Jellyfin client: libraries, items, images, stream URLs, `Config`. GET only, enforced by `tests/readonly.rs` | never |
| `crates/jellyfin-user` | User-scope calls: login, Quick Connect, logout, playback reporting, played / favourite, own password, own profile image, display language, own playlists, SyncPlay (REST, WebSocket, clock sync, state machine) | yes, user scope |
| `crates/seerr` | Seerr client: sign-in (Quick Connect, password), search, details, quota, own requests; create and cancel a request only with a `RequestConfirmed` (all in `write.rs`); `guard::SeerrTestGuard` for live tests. Never talks to a media server | no (Seerr only) |
| `crates/jellyfin-admin` | Admin reads and writes. Destructive calls need a `Confirmed` token; live tests guarded by `TestServerGuard` | yes, admin |
| `crates/session` | Tokens in the system keyring (Secret Service), one per server account; server and account names on disk; atomic writes | n/a |
| `crates/player` | playback: `EmbeddedPlayer` (libmpv in a Wayland subsurface), `Player` (mpv child process over JSON IPC) and `VlcPlayer` (VLC child process over its loopback HTTP interface, `vlc.rs`). libmpv is `dlopen`ed (`mpv_ffi.rs`); `deps.rs` probes libmpv and the mpv binary and builds the install hint. No UI or Jellyfin dependencies | n/a |
| `crates/core` | Everything UI-framework-free: settings, navigation, list / detail / admin / account state, the source layer (`source`, `backend`, `catalog`, `sources`, `merge`, `versions`), playback rules (`player_logic`, `video`, `sync`, `syncplay`, `player_mpris`), caches, image cache, blurhash, the tokio `bridge` | via the crates above |
| `crates/local` | The local library: scan, naming rules, `.nfo` and sidecar metadata, optional `ffprobe` details, index, overrides, local resume state. Never writes to media folders. No UI dependencies | never |
| `crates/gateway` | The optional companion binary `nocturne-gateway`: one Jellyfin-compatible address over every source (axum router with an allow-list, gateway sign-in, id map, merged sessions and remote control, `/socket`). The app starts it as a child (Settings > Gateway) and talks to it over a unix control socket; nothing links it. Off by default: with the switch off the process does not exist | yes, only the gated forwards of [WRITES.md](WRITES.md) |
| `crates/server` | The headless server binary `nocturne-server` (also the CLI): modules for the library manager, downloads, requests, gateway, sources and storage behind one REST + WebSocket API; state in redb, config in `server.toml` ([dev/SERVER_PLAN.md](dev/SERVER_PLAN.md)) | no; imports into library folders the user chose |
| `crates/arr-types`, `crates/torrent`, `crates/indexer` | The library manager's contract (`Downloader`, `Indexer`, release model), the librqbit torrent engine, the Torznab / Newznab client and release parser | no |
| `crates/app` | The Iced application. The only crate with UI code | via the crates above |

`player`, `core`, `local` and the Jellyfin crates have no UI-framework dependency; `player` also has no Jellyfin dependency.

## Data flow (Elm architecture)

`crates/app/src/main.rs` starts `iced::application(App::boot, App::update, App::view)`. There is one `App` state (`app/mod.rs`) and one `Message` enum (`app/message.rs`).

1. `view(&self)` builds the widget tree from state. It does no I/O.
2. A user action or subscription event becomes a `Message`.
3. `update(&mut self, Message) -> Task<Message>` changes the state on the UI thread and returns a `Task` for any work.
4. Network and disk work runs through `bridge::spawn` (below), wrapped in `Task::perform`; its answer arrives as a new `Message`.

Screens that are larger than a handful of fields (`detail/`, `account/`, `player/`, admin) are plain structs with their own `Msg`, `update` and `view`; the root wraps them with `.map(...)`. A screen reports side effects to the root as an `Outcome` (play this, write that, prefer this source) instead of reaching into root state.

Answers are stamped with what they were asked for: the page id or an **epoch** counter, plus the source. When the user has moved on (another page, another filter, a source removed) the answer is dropped on arrival, so a slow server can never paint over a newer screen. Subscriptions are limited to keyboard input, a 200 ms tick (connection health, playback reporting, shutdown on SIGTERM) and `window::frames()`, which is active only while something animates.

### Tokio bridge

`nocturne-jellyfin` is async; Iced runs its own executor on the main thread. `crates/core/src/bridge.rs` keeps one process-wide multi-thread tokio runtime (two worker threads, so every reqwest client lives on it) and hands futures to it with `bridge::spawn`. The returned handle is a plain future that `Task::perform` awaits. No `block_on`, no nested runtimes.

## Catalog and sources

A **source** is a local library or one Jellyfin server with one account. `core/catalog.rs` owns them as `Slot`s: `{ SourceId, Profile, Backend, Status }`.

- `Backend` is an enum, `Jellyfin(Source)` or `Local(LocalBackend)`, with the methods the UI uses (libraries, browse, latest, resume, detail, play target). A third kind would be one more variant; there is no plugin system. What a local source cannot do (studios, similar items, extras, playlists) answers empty; admin and SyncPlay are Jellyfin-only and reached through `Backend::jellyfin`.
- Every `Media` carries `origin: SourceId`; `(origin, id)` is an item's identity in navigation, caches, image keys, playback and writes.
- `Profile`s persist in `sources.toml` (versioned, atomic, no secrets). A 0.2 install migrates its server login into a source on first start; the old files stay in place. Tokens stay in the keyring.
- `Status` per slot: `Connecting`, `Online`, `Offline { retry_at }` (backoff), `SignedOut` (401: sign in again for that server only), `Indexing { done, total }` (local), `Disabled`. A health check on the tick updates them; one failing source never blocks the others. Sources load in a task after the first frame (keyring reads, opening the index), so nothing blocks startup.
- A **fan-out** (`fan()` in the app, state in `core/sources.rs`) sends one request per online source; answers merge as they arrive and stale epochs are dropped.
- **Duplicates** (`core/merge.rs`) merge only on strong identity: movies and series on a shared provider id (IMDb, TMDB, TVDB), episodes on the series' provider id plus season and episode number. Title and year alone never merge. A merged `Entry` shows the preferred copy (by source priority, or the user's per-title choice kept locally) and lists the others as alternates.
- **Versions and sources** (`core/versions.rs`, picker in `detail/overlays.rs`): a server's own `MediaSources` and the copies in other sources form one list. The choice goes to the root as `Outcome::Play { source_id }`; "Always prefer" as `Outcome::Prefer`.
- A developer token (`NOCTURNE_JF_TOKEN`, debug builds) is a transient read-only source with id 0.

The first-run routing (`FirstRun` / `Step` in `core/sources.rs`) shows the welcome screen when there are no sources; Settings > Sources (`app/sources.rs`) and the local folders screen (`app/folders.rs`, scan on a thread with progress and cancel, unmatched items with manual match) manage them afterwards.

### Local library

`crates/local` scans each root (symlink loops end, unreadable entries are reported, hidden and `.nomedia` directories are skipped, re-scans are incremental), parses names (movies, `Show/Season 01/... S01E02 ...`, provider tags), and reads `.nfo` and sidecar art; `ffprobe` adds runtime and streams when installed. Metadata precedence: user overrides, `.nfo`, sidecar art, embedded tags, file name. Files keep a stable id across moves and renames (inode or size plus partial hash). Persistence under `$XDG_DATA_HOME/nocturne/local/`: `index.json` (rebuilt by scans), `overrides.json` (user edits, never touched by scans) and `state.json` (resume and played state). The index is held in memory, which is comfortable to tens of thousands of files; SQLite is the step beyond that if libraries outgrow it ([SOURCES.md](SOURCES.md)).

## Images

`core/imgcache.rs`: a blurhash placeholder first, then bytes from a size-capped disk LRU cache, fetched with retry on a miss; cache keys include the origin. The UI keeps decoded `iced::widget::image::Handle`s (cheap to clone) keyed the same way. Item data is cached in `core/cache.rs`. Library grids and lists are virtualised: only the rows in the scroll viewport are built.

## Playback and in-window video

Iced has no video surface. `EmbeddedPlayer` creates a `wl_subsurface` placed below the (transparent) Iced window and renders with libmpv's **OpenGL render API through EGL**. The player page draws a `Hole` widget (`widget/hole.rs`): it paints nothing, so the compositor shows the video through it, and it reports its layout bounds, which become `set_bounds`. The controls are ordinary Iced widgets drawn over it. Raw Wayland handles come from `window::run` (`platform.rs`). Hardware decoding is on by default; `hwdec-current` is read during embedded playback to verify it.

**Never create an mpv Vulkan / `gpu-next` context in the app process.** A second Vulkan instance next to the renderer's crashed on NVIDIA; history and measurements in [video-embedding.md](video-embedding.md). The Iced renderer should also stay on its Vulkan backend: with the GL backend the video was darkened.

Linux is Wayland-only: the UI toolkit is built without its X11 backend and the app exits with a message on an X11-only session. Where in-window video is unavailable (`NOCTURNE_VIDEO=external`, a failed Wayland setup, the HDR pop-out, or macOS/Windows for now), `Player` runs the `mpv` binary as a child process with its own window, controlled over a private unix socket, so a driver crash cannot take the app down. Both backends are used through `core/video.rs` (`Playback`), which picks the backend at the first play. libmpv is `dlopen`ed at that point: if it is missing the external mpv is used, and if that is missing too the player page shows an install hint for the distribution. Opening a stream and stalls show the buffering overlay (`widget/buffering.rs`): backdrop, logo revealed by the buffer fill, status line. Buffering watermarks: [buffering.md](buffering.md).

Settings > Video player > Player (`video::choose_player`; `NOCTURNE_VIDEO` wins) can pick VLC instead: `VlcPlayer` starts `vlc` with `--extraintf=http` on 127.0.0.1, a random port and a random password, and one worker thread polls `status.json` and sends commands with short timeouts, so the UI thread never waits on it. VLC cannot send request headers, so the server token travels as `api_key` in the URL handed over that authenticated loopback interface (never on the command line); a source that needs any other header plays in-window. Tracks, filters and mpv properties do not exist there. Resume comes from the same seek-on-load as the other backends.

Pure player rules (`player_logic.rs`, `video.rs`) and MPRIS (`player_mpris.rs`, zbus) are in `crates/core`. Playback reporting (start, progress, stop) is driven from the root tick (`sync_tick`, pure state machine in `core/sync.rs`), not from the player, so one code path handles every report. The player page (`player/`) is its own screen with controls, popovers, trickplay and key handling; it closes by dropping the player before the window closes (`exit_on_close_request(false)`). Playback targets are a stream URL plus header lines (credentials never go in the URL or argv) or a local file path.

## 0.4 modules

All pure logic is in `crates/core`, with the UI glue in `crates/app/src/app/`.
- `remote.rs` + `app/remote.rs`: session capabilities and the WebSocket command stream from other Jellyfin apps; commands only drive the player.
- `watched.rs` + `app/cascade.rs`: the watched cascade (strong-id matching of copies, one hop only so changes never echo).
- `trakt.rs` + `app/trakt.rs`: PKCE sign-in, scrobbling, history import. Tokens in the keyring.
- `smart.rs` + `app/smart.rs`: smart-collection rules evaluated over the merged catalog; store in `smart.json`.
- `online/` (TMDB, TVmaze, MusicBrainz, matching, subtitles) + `app/online.rs`, with the stored layer in `crates/local/src/online.rs`.
- `download/` (queue, resumable fetch, Jellyfin-style naming) + `app/downloads.rs`: offline copies; GET only.
- `hdr.rs`: HDR detection and the SDR / pop-out decision; the pop-out is the external `Player` (`vo=gpu-next`).
- `lang_memory.rs`: per-show language memory in `languages.json`.
- `app/src/couch.rs`, `pad.rs`, `app/couch.rs`: couch-mode scale and focus, the gamepad thread (`gilrs`), remote keys.
- `session::paths` (re-exported as `core::paths`): per-OS config / data / cache directories, one pure `resolve()`.

## 0.5 modules

- `crates/core/src/customize/`: native theming from CSS, with no browser engine, no network and no code execution. `css.rs` is a small offline reader, `value.rs` parses colours, lengths and fonts, `mod.rs` translates the supported subset into `Theme` tokens (only the fields a stylesheet sets, plus a `Warning` with line and column for everything it did not understand), `layout.rs` is the vocabulary of named app parts (`.card`, `.row`, `.hero`, `.header`, ...) and the typed `Layout` they become, and `store.rs` keeps saved themes in `themes.toml` (versioned, atomic; only the CSS text is stored and tokens are re-read on load). `examples/` holds the Marquee and Feed themes. Reference: [dev/CUSTOMIZATION_PLAN.md](dev/CUSTOMIZATION_PLAN.md).
- `crates/app/src/app/themes.rs`: the Settings > Themes page (editor, import from paste, file or one https address, the 15 second preview) and the glue that applies the active theme. `crates/app/src/theme.rs` holds the live colour and size tokens and the layout the widgets read.
- **Theme resolution layers**, lowest to highest: built-in defaults, the global theme, the server's branding CSS (Server UI only), the theme the user assigned to that server. `Themes::resolve_all(source)` merges them; `sync_theme` runs after every message and re-resolves only when the selection key changed, never while drawing. The source is the detail page's own server, else the single server picked in the source filter, else none (global). Server branding is never saved into `themes.toml`.
- `crates/core/src/server_ui.rs` + `crates/app/src/app/server_ui.rs`: Server UI per server (Settings > Sources > Layout). The pure part reads the user's `homesection0..9` display preferences, `OrderedViews` and the exclusions into the section list, the My media row and the Latest rows; the reads are in `nocturne-jellyfin`, the one write (the Sections editor) in `nocturne-jellyfin-user`, behind `allow_server_writes`. With the filter on All the unified Home is used.
- `crates/core/src/update.rs`: the daily check (`check_updates`), install-kind detection (`Dev`, `SystemPackage(Pacman | Apt | Dnf)`, `PerUser`) and the manifest rules; one small JSON file, at most 64 KiB, nothing sent but the version in the User-Agent. `update_install.rs`: size-capped streaming download into a staging file, sha256, ed25519 signature, and only then the install directory is touched (Linux tarball, Windows directory swap, macOS `.app` from a `.tar.gz`); any failure leaves the running install as it was. The public update keys are compiled in; the private one never leaves the maintainer's machine ([DECISIONS.md](DECISIONS.md), [SECURITY.md](../SECURITY.md)). Plan: [dev/UPDATER_PLAN.md](dev/UPDATER_PLAN.md).
- Text editing: every text field is an Iced `text_input` with real selection and clipboard handling; the hand-rolled appending input of 0.4 is gone.
- Trakt ships a built-in public client id (PKCE, no secret); a user-supplied id still overrides it ([TRAKT.md](TRAKT.md)).
- Release flow: `release.yml` builds every format on GitHub runners and drafts a release; repositories and update artifacts are signed locally ([PACKAGING.md](PACKAGING.md)). Contributor scripts: `scripts/headless-session.sh` and `scripts/headless-input-check.sh` ([CONTRIBUTING.md](../CONTRIBUTING.md)).

## 0.6 modules

- `calendar.rs` + `app/calendar.rs`, `app/calendar_tree.rs`: the Calendar. Pure part (`core/calendar.rs`): civil-day arithmetic, ISO air-time parsing with the zone as a parameter, merging of each server's upcoming episodes (`Backend::upcoming`, a GET of `/Shows/Upcoming`, servers only) and the Trakt calendar (`Trakt::calendar`, read only) into dated entries marked on your server / airing soon / not on your servers, month, week and agenda ranges. The page is `View::Calendar` with state in `App::cal`, drawn as an element tree through `Which::Page` (classes `.calendar`, `.calendar-day`, `.calendar-entry`, ...); a server's own layout and `NOCTURNE_ENGINE_UI=0` use a plain list page. Home's optional "Coming this week" row is `HRow::Coming`.

## Gateway (0.7)

`crates/gateway` is a separate process so a network-facing listener cannot freeze the window and nothing runs when it is off ([dev/GATEWAY_PLAN.md](dev/GATEWAY_PLAN.md)). The protocol between the app and the process (status, reload, stop, devices, Quick Connect approval, password) and the gateway settings are pure types in `core/gateway.rs`; `app/gateway.rs` owns the child process, polls the control socket only while Settings is open or the gateway is on, and draws Settings > Gateway (QR code of the address through the `qrcode` crate, in the app only). "Keep running when NOCTURNE is closed" writes a systemd user unit (`core/user_unit.rs`, Linux only). `sessions.rs` and `ws.rs` in the gateway crate hold the merged session table and the `/socket` WebSocket: native sessions are driven over their socket, devices on real servers by a forwarded call gated as in [WRITES.md](WRITES.md). The self-updater installs `nocturne-gateway` next to `nocturne` when the archive has it. The gateway never creates a `Confirmed` or `UserConfirmed` token.

## Watch together (SyncPlay)

`jellyfin-user/src/syncplay/` holds everything that is not UI: `model.rs` (typed messages), `time.rs` (clock offset), `engine.rs` (a pure state machine that takes `now` and returns what to do), `rest.rs` (calls; writes take `UserConfirmed`) and `ws.rs` (reqwest upgrade, tungstenite framing, reconnect with backoff). `core/syncplay.rs` has the pure app rules. The Iced side drains the socket channel on a short poll, shows the screen and the header pill, and applies the group's commands through methods that never route back into the group. While in a group the player pushes transport requests into an outbox instead of acting. SyncPlay acts on one chosen server. See [syncplay.md](syncplay.md).

## Write gating

All server changes funnel through one master setting, `allow_server_writes` (default off, persisted in `settings.toml`).

- A write goes to the item's **origin**: the root resolves the slot from `media.origin` (`Catalog::write_slot`) and uses that server's user client, never another one. Local items have no server: their watched and resume state is saved to the local `state.json`, which needs neither setting.
- Progress reporting, played / favourite and password change are skipped (controls dimmed) when the setting is off.
- The admin UI is view only when it is off; when on, every admin action goes through the single confirmation choke point (`Admin::on_confirm`), which mints the `Confirmed` token that destructive `jellyfin-admin` calls require. The user's own password change mints `UserConfirmed` only in `account/mod.rs`.
- The developer API-key source refuses writes whatever the setting says.
- `crates/jellyfin` cannot write at all.

User-facing matrix: [WRITES.md](WRITES.md).

## Settings and sessions

`core/settings.rs` reads and writes versioned TOML under `$XDG_CONFIG_HOME/nocturne/` atomically on every change. Secrets never go there: tokens live in the keyring (`NOCTURNE_KEYRING_SERVICE` overrides the service name so tests never touch real entries). Release builds never read `.env`.

## Platform seams

OS-specific code stays behind a few seams ([PLATFORMS.md](PLATFORMS.md)): `app/src/platform.rs` (window handles for the video surface, signals, now-playing art file), `crates/player` (embedded and external backends, IPC, libmpv loading), `crates/session` (keyring), MPRIS in core, and XDG paths. Everything else in the UI is OS-neutral.

## Debug hooks

Debug builds only: `NOCTURNE_SNAP=/tmp/x.png` (with `NOCTURNE_SNAP_AFTER_MS=1500,4000` for several) writes the app window's own pixels through `window::screenshot` (`snap.rs`); `NOCTURNE_PREVIEW=<screen>` opens a screen with canned or throwaway-server data for layout work (`detail/preview.rs`, `player/preview.rs`, `app/preview.rs`); `NOCTURNE_TIMING=1` prints boot and first-frame timings. Use `scripts/run-isolated.sh` so a test run never touches the real config, data or keyring.
