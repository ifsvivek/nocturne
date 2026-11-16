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
- [~] 4d player page (worktree, own module; root hook wired by master after M3)
- [ ] 4b-4e screens
- [ ] 5 cut-over
- [ ] 6 verification
