# GPUI -> Iced migration plan (working document)

Branch: `iced-migration`. `main` stays at the 0.2.0 baseline until the migration passes the gate below.
Delete this file (fold the lasting parts into ARCHITECTURE.md / DECISIONS.md) when the migration ships.

## Target

- UI: **Iced 0.14** (wgpu renderer, winit). Elm architecture: one `State`, `Message`, `update -> Task`, `view`.
- Video: unchanged strategy if the spike confirms it: `EmbeddedPlayer` (libmpv OpenGL render API over EGL in a
  `wl_subsurface` below the transparent Iced window); external `mpv` child process on X11 / failure.
- libmpv is loaded at runtime (`dlopen`), so a missing library degrades playback instead of killing the process.

## Release scope (user, 2026-11-16)

Arch Linux only for now (no .deb/.rpm/AppImage/macOS/Windows work). Deliverable: an installable Arch build for the user
to test. No release and no other platforms until the user has used it and approved. A docs audit worker runs when a
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
- Temporary: until the cut-over, pages not yet ported render a plain "not ported yet" panel.

## Running it for checks

Demo library on a throwaway server: `scripts/make-demo-library.sh build` (prints connection info; password in
`$DEMO/jf/demo-password`). Or `scripts/test-jellyfin.sh start` (`/tmp/nocturne-jf-test/test.env`). Run with
`NOCTURNE_JF_URL=http://127.0.0.1:8097` plus throwaway `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`,
`NOCTURNE_KEYRING_SERVICE=nocturne-test`. Never the real server, never the repo `.env`. Screenshot only the app window:
`grim -g "$(hyprctl clients -j | jq -r '.[]|select(.class=="nocturne")|"\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"')" out.png`
(never a full-screen grab: it captures the user's desktop).

## Progress

- [x] 1 libmpv / installer (dlopen FFI verified in-window via the spike; installer container tests pending: docker needs sudo)
- [x] 2 spike: GO (notes in ICED_NOTES.md)
- [x] 3 core extraction (crates/core; 270 tests pass)
- [ ] 4a-4e screens
- [ ] 5 cut-over
- [ ] 6 verification
