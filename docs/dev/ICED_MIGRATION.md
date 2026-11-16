# GPUI -> Iced migration plan (working document)

Branch: `iced-migration`. `main` stays at the 0.2.0 baseline until the migration passes the gate below.
Delete this file (fold the lasting parts into ARCHITECTURE.md / DECISIONS.md) when the migration ships.

## Target

- UI: **Iced 0.14** (wgpu renderer, winit). Elm architecture: one `State`, `Message`, `update -> Task`, `view`.
- Video: unchanged strategy if the spike confirms it: `EmbeddedPlayer` (libmpv OpenGL render API over EGL in a
  `wl_subsurface` below the transparent Iced window); external `mpv` child process on X11 / failure.
- libmpv is loaded at runtime (`dlopen`), so a missing library degrades playback instead of killing the process.

## Release scope (user, 2026-11-16, updated)

Arch Linux only (no .deb/.rpm/AppImage/macOS/Windows work). 0.3: push to GitHub and create the GitHub release once the
master's own testing passes and redeploy the website (user approved; after the
screenshot recapture and `VERSION` 0.3.0 in `website/site.config.json`). Then 0.4 on a LOCAL-ONLY branch `v0.4-local`
(never pushed, no release). A docs audit worker runs when a
slot frees, and again after the cut-over (GPUI -> Iced references).

## Scope change 2026-11-16

Multi-source (local library + several Jellyfin servers, unified catalog) is now core: design and milestones in
`docs/SOURCES.md`. Order: 4a (running) -> sources M1 (`crates/local`, parallel, worktree) -> M2 core catalog -> M3 Iced
root on the catalog (first run, Sources settings, startup screen) -> 4b-4e written directly on the catalog model.

## Steps

1. **libmpv runtime loading + installer** (crates/player, packaging). Independent of the UI work.
2. **Embedding spike** (`spikes/iced-embed`, not a workspace member): go/no-go for in-window video under Iced,
   verified Iced 0.14 API notes.
3. **Extract `crates/core` (`nocturne-core`)**: every UI-framework-free module of `crates/app` moves into a library
   crate, unchanged except visibility and the image-format sniff (`source.rs` used `gpui::ImageFormat`; replace with
   a local enum). Modules: settings, nav, libview, search, detail, sync, player_logic, video, source, cache,
   imgcache, blurhash, dropdown, syncplay, player_mpris, bridge, admin/{state,model,act,confirm,fmt},
   account/state. The GPUI app keeps building on top of core; all existing tests move with their modules.
4. **New crate `crates/ui` (`nocturne-ui`, temporary bin `nocturne-iced`)** built on core, screen by screen:
   a. bootstrap, theme (Iced colours from the same tokens), fonts, widget kit (ports of `ui.rs`), header, nav,
      toasts, offline banner, global keys, connect / sign-in / Quick Connect / profile picker, settings page;
   b. home (hero band, rows), library browse (tabs, sort/filter, A-Z, virtualised grid/list), search;
   c. detail pages (movie, series/seasons/episodes, person, collection), playlists, help overlay;
   d. player page (embedded/external backend, controls, popovers, trickplay, skip/next, queue, MPRIS) and the
      `Root::tick` sync reporting;
   e. account page, admin (all sections, confirm dialog, `on_confirm` minting), SyncPlay screen.
5. **Before the cut-over**: measure the GPUI 0.2.0 baseline (release binary size, tarball size, cold start, idle RSS/CPU on
   sign-in and on a loaded home page, CPU during 1080p embedded playback) with `scripts/perf.sh` into `docs/dev/PERF.md`.
   **Cut-over**: delete `crates/app` (GPUI), rename `crates/ui` -> `crates/app` (package `nocturne-app`,
   bin `nocturne`), drop `vendor/xattr` + `[patch]` (only gpui needed it; verify), prune dependencies, regenerate
   THIRD_PARTY_NOTICES, update docs/CI/packaging, grep the repo for GPUI. Re-derive the runtime soname list in
   `packaging/install.sh` / PKGBUILD / PACKAGING.md for winit+wgpu (winit dlopens libX11, libXcursor, libXrandr,
   libXi, libX11-xcb, libwayland-*, libxkbcommon); delete `spikes/`.
6. **Verification + release prep**: fmt/clippy/tests, run on Wayland and X11 (XWayland), clean-container install
   of the tarball and PKGBUILD, playback after install, CHANGELOG + version 0.3.0. No release without the user.

## Rules for the port

- Same product: tokens in `theme.rs` are the only colours; layouts, copy, keys and behaviour match the GPUI app
  (screenshots in `docs/screenshots/`). Use Iced idioms, do not emulate GPUI APIs.
- No Jellyfin types in view code beyond what core already exposes (`Media`, `Source`, ...); no raw libmpv handles in
  the UI; playback goes through core's `video.rs` seam.
- Safety rules from CLAUDE.md are unchanged: writes gated by `allow_server_writes`; `Confirmed` / `UserConfirmed`
  minted only after their dialogs.
- Async: responses carry page/epoch and are dropped when stale, as today.

## UI crate architecture (`crates/ui`, decided; every worker follows it)

- `main.rs`: `iced::application(App::boot, App::update, App::view)` + `.subscription`, `.title`, `.theme`, `.style`
  (transparent app background), `.window(Settings { transparent: true, min_size 800x560, size 1280x800,
  platform_specific.application_id = "nocturne" })`, `.default_font(Noto Sans)`, `.exit_on_close_request(false)`
  (close drops the player first, then `window::close`). SIGTERM/SIGINT keep the `TERM` atomic, polled by the tick.
- One root `App` (port of `Root`) in `app/mod.rs` with `Message` in `app/message.rs`. Screens that were their own
  GPUI entities (Admin, Accounts, Picker, PlayerView) become plain structs with their own `Msg` enum,
  `update(&mut self, msg, ..) -> Task<Msg>` (or an `Outcome` the root applies) and `view(&self) -> Element<Msg>`;
  the root wraps them (`Message::Admin(admin::Msg)`) and maps with `.map(...)`. No global mutable state.
- Files mirror the GPUI app so reviewers can diff behaviour: `app/{mod,message,header,home,browse,detail,search,
  playlists,help,syncplay,settings}.rs`, `account/{mod,view,picker}.rs`, `admin/{mod,view}.rs`,
  `player/{mod,view}.rs`, `widget/*.rs` (port of `ui.rs`: chips, buttons, tiles, poster cells, avatar, switch,
  value chip, section band, tabs, progress, wordmark, video hole), `theme.rs`, `style.rs` (container/button style fns).
- Theme: same tokens and hex as `crates/app/src/theme.rs` as `iced::Color` consts + the contrast test. Only
  `theme.rs` holds raw colours. Type scale, spacing, radii and motion durations unchanged.
- Async: all Jellyfin/network futures go through `nocturne_core::bridge::spawn` (one tokio runtime owns the reqwest
  clients) wrapped in `Task::perform`. Iced is built with its `tokio` feature only for `time::every`. Responses carry
  the page id / epoch and are dropped when stale, exactly as today.
- Images: `iced::widget::image::Handle` (cheap clone) cached in the root by the same keys `Root` uses today;
  blurhash placeholder first (`Handle::from_rgba`), then the disk/network bytes (`Handle::from_bytes`).
- Input: `keyboard::listen()` subscription routed through one `on_key` that keeps today's precedence (overlays >
  menus > search > page); text inputs use `widget::Id` + `focus` tasks. Mouse via `button` / `mouse_area`.
- Motion: `iced::animation::Animation` for hover/fade/slide (durations from theme: HOVER 140 ms, BASE 220, SLOW
  300, ease-out); subscribe to `window::frames()` only while something animates.
- Video: player page leaves a transparent hole (custom `Hole` widget publishes its bounds -> `set_bounds`); Wayland
  handles via `window::run`; X11 / failure / `NOCTURNE_VIDEO=external` -> external `Player`. libmpv missing ->
  external mpv; mpv binary missing too -> actionable message with `nocturne_player::install_hint()`.
- Hard requirements: launch is fast (first frame and interactive shell measured with NOCTURNE_TIMING=1; nothing
  blocking before the first frame); library grids/lists are virtualised (build only the rows in the scroll viewport, from
  `on_scroll` viewport math; no thousands of widgets); the player step verifies hardware decoding by reading mpv's
  `hwdec-current` during embedded playback and records it in QA notes.
- Temporary: until the cut-over, pages not yet ported render a plain "not ported yet" panel.

## Running it for checks

Demo library on a throwaway server: `scripts/make-demo-library.sh build` (prints connection info; password in
`$DEMO/jf/demo-password`). Or `scripts/test-jellyfin.sh start` (`/tmp/nocturne-jf-test/test.env`). Run with
`NOCTURNE_JF_URL=http://127.0.0.1:8097` plus throwaway `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`,
`NOCTURNE_KEYRING_SERVICE=nocturne-test`. Never the real server, never the repo `.env`: `Config::load` takes the nearest
`.env` of the current directory **or any ancestor**, so run the binary from a directory outside the repository (a worktree
under the repo finds the real `.env`). Screenshots: debug builds only, `NOCTURNE_SNAP=/tmp/x.png`
(+ `NOCTURNE_SNAP_AFTER_MS=1500,4000` for several) writes the app window's own pixels through `window::screenshot`; never grab
the screen (`grim`) because it captures the desktop. Driving the window on Hyprland: `wtype` for keys, clicks with
`ydotool mousemove --absolute` (arguments are half the pixel coordinates on a 2560x1440 output) plus `ydotool click 0xC0`,
window origin from `hyprctl clients -j`. Keyring entries are shared by every throwaway profile that uses the same
`NOCTURNE_KEYRING_SERVICE`, so removing a server in one profile signs it out in the others.

## Progress

- [x] 1 libmpv / installer (dlopen FFI verified in-window via the spike; installer container tests pending: docker needs sudo)
- [x] 2 spike: GO (notes in ICED_NOTES.md)
- [x] 3 core extraction (crates/core; 270 tests pass)
- [x] sources M1 crates/local merged (docs/METADATA.md too)
- [x] docs audit pass 1 merged (pass 2 after cut-over: PKGBUILD -p nocturne-app, gen-notices GPUI text, video-embedding.md trim, ci.yml mpv)
- [x] 4a foundation (`crates/ui`, bin `nocturne-iced`): bootstrap, theme, widget kit (eased `Press` button, `Popover`
  overlay menu), connect / sign-in / Quick Connect / profile picker, header, history, breadcrumbs, toasts, offline
  banner, account menu, Settings, help overlay, own-profile dialogs (password, authorize, profile image, language).
  Verified against a demo server with wtype/ydotool. Deferred: Home/browse (4b), detail/playlists/search overlay (4b/4c),
  player (4d), devices overlay, Watch together, admin, music queue (4e). Platform code is isolated in `ui/src/platform.rs`.
  While GPUI shares the workspace, `crates/ui` carries a `naga` feature pin (see its Cargo.toml); delete at cut-over.
- [x] sources M2 core catalog merged (348 tests): SourceId/origin, provider ids, sources.toml profiles + migration,
  Backend enum (Jellyfin/Local), Catalog + statuses, merge/duplicates, preferred sources, write routing.
  Carry into M3: migrate every server with a usable login (not just the last), dev-token mode as a transient read-only
  source, availability flag on Media so unplugged local copies are demoted in merges.
- [x] first-run welcome, tracked-text glyph fix, normal-size buttons, reduce-motion setting, Stremio-style player
  buffering overlay widget (`widget/buffering.rs`, wired in 4d). Launch: ~9 ms app boot, ~430 ms to first frame
  (wgpu/Vulkan init dominates). Screenshots from now on via the debug `NOCTURNE_SNAP` in-app hook, never grim.
- [x] GPUI baseline in docs/dev/PERF.md (25.7 MiB, 606 ms to window, 343/414 MiB idle RSS, ~2% idle CPU);
  clean-container installs pass (Arch tarball + PKGBUILD, Debian/Ubuntu/Fedora dep names). Binaries built here need
  glibc 2.44: fine for Arch; 0.4 .deb/.rpm must be built inside their distro containers; docs pass 2 must say the
  tarball targets current Arch.
- [x] M3 Iced root on the catalog (worktree, `crates/ui/src/app/{catalog,sources,folders}.rs` + `crates/core/src/{sources,folders}.rs`):
  `App` holds `Sources` + `Catalog` + `Preferred` + `SeriesIds`; sources load in a task after the first frame (keyring
  reads, index opening); per-server health on the 200 ms tick (401 -> that slot SignedOut with a quiet banner, no
  answer -> Offline with backoff, other sources unaffected); developer token = transient read-only slot id 0;
  `Settings > Sources` page (own page, reached from Settings and the account menu) with status, sign in again,
  rescan, enable/disable, priority up/down, remove (confirmation, says what is deleted); local folders screen (portal
  chooser via `rfd` or typed path, availability, live scan progress on a thread, cancel, counts, Unmatched with manual
  match -> overrides); first-run routing (`FirstRun`/`Step` in core); `source_mark` widget, `SourceFilter` (header
  chip), `fan()` helper (one request per online slot, answers merge as they arrive, stale epochs dropped).
  Debug-only `NOCTURNE_SNAP=/tmp/x.png` (+ `NOCTURNE_SNAP_AFTER_MS=1500[,3000,...]`) captures the app window through
  `window::screenshot`; use it instead of any screen grab. Carry into 4b: header nav needs a "More" menu once there
  are many libraries (now scrolls sideways), `Message::Fan` is only used for libraries, a scan holds the local
  library's write lock (reads of that source wait; see the ponytail note in `core/folders.rs`).
- [x] 4d player page merged (`crates/ui/src/player/`): embedded + external + libmpv/mpv-missing paths, full controls,
  Stremio-style splash wired, MPRIS, SyncPlay hooks kept. hwdec-current = nvdec for H.264 1080p and HEVC 10-bit (0
  dropped). 25 open/close cycles: no leak (debug RSS ~555 MiB; re-measure in release). Root wiring: 4b worker.
  Follow-ups: music-queue overlay (4e), check splash logo clipping with a real logo, one snapshot hook.
- [x] 4b home, browse, search, header for many sources, Sources polish (`app/{home,browse,search,images,actions,play,cells}.rs`,
  `widget/cell.rs`, `virt.rs`, core `unified.rs`). Home: hero + Continue watching / Next up (merged across sources,
  interleaved by recency) + Latest per library, rows arrive per source. Browse: one source = lazily paged grid, several
  = every source loads fully in the background and is merged and sorted client-side (`unified::Merged`, cap 20,000
  items per source); both build only the rows in the scroll viewport (`virt.rs`) between two spacers. Unified Movies /
  Shows / Music / Photos + Collections / Playlists / Favourites span all sources; the header shows unified kinds and a
  Libraries menu when several sources contributed, the GPUI link row with one. Search merges all sources. Images: blurhash
  stand-in, then bytes decoded off the UI thread to RGBA at the drawn size (bounded `Cache`, separate 6-entry hero cache).
  Played / favourite route to the item's own source (`write_slot` / local state) with optimistic patch + revert.
  `Message::Play(Media, Vec<Media>)` opens the player page (`app/play.rs`, 4d is wired). Strips scroll sideways with
  horizontal wheel / Shift+wheel / keyboard (a plain wheel scrolls the page: Iced hands a vertical wheel to the page unless the
  strip is the only scroller). Deferred: detail pages and the photo viewer (4c), shimmer / fade-in on images.
- [x] 4c detail pages merged ( own module `crates/ui/src/detail/`, root hook documented at the top of `detail/mod.rs`):
  movie, series (season tabs, episode cards), season, episode, person, collection, playlist (own-playlist edit keys and
  rename / delete / add dialogs), music album / artist, photo viewer; hero, facts, chips, about, cast, chapters, extras,
  similar, media info, More menu, optimistic watched / favourite as `Outcome::Write` (the root performs and answers with
  `Msg::Wrote`), writes-off dimming + tooltip, calm `PlayError` text for a missing local file. **Versions and sources
  picker** (`core/versions.rs` + `detail/overlays.rs`): server `MediaSources` and other sources' copies in one list,
  choice goes to `Outcome::Play { source_id }` (`Backend::play_target_for` adds `MediaSourceId`), "Always prefer this source"
  goes to `Outcome::Prefer` (`versions::prefer`). Pages are laid out to fixed band heights (`detail/geom.rs`) so
  keyboard focus scrolls into view without measuring. Debug preview: `NOCTURNE_PREVIEW=detail` (see `detail/preview.rs`).
  Deferred: hero fade-in, source marks on strip cells, "New playlist" entry point (the Add dialog creates one by name),
  a shared root image cache (the page keeps its own).
- [~] 4e account page + devices, admin per server (confirm choke point), SyncPlay, music queue (worktree, modules + hooks)
- polish backlog: disk-cached shrunken local posters (first scroll of a 5k local library decodes full-size art at ~92% CPU); several-source browse eagerly loads every page (cap 20k/source; revisit for huge servers); version sizes < 1 MB show "0 MB"; splash logo clipping check; one snapshot hook
- [x] docs pass 2 + CHANGELOG [0.3.0] + docs/dev/RELEASE_NOTES_0.3.0.md drafted on local branch `docs-0.3-pending`
  (written for the post-cut-over state; merge WITH the cut-over). Before release verify its assumptions: 4b/4e merged
  and working, mpv 0.42 TLS check DONE (it moved it to 'carried forward'; move back once done), startup does not wait
  for the network (measure), reduce motion honoured everywhere.
- [ ] integration: mpv 0.42 TLS check (test playback against an HTTPS throwaway server with a self-signed cert using
  mpv 0.42 if available, else document; add per-server trust only if needed); wire the detail hook (and later 4e hooks) into app/mod.rs after 4b merges
- [ ] 5 cut-over
- [ ] 6 verification
