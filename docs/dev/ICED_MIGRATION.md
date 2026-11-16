# GPUI -> Iced migration plan (working document)

Branch: `iced-migration`. `main` stays at the 0.2.0 baseline until the migration passes the gate below.
Delete this file (fold the lasting parts into ARCHITECTURE.md / DECISIONS.md) when the migration ships.

## Target

- UI: **Iced 0.14** (wgpu renderer, winit). Elm architecture: one `State`, `Message`, `update -> Task`, `view`.
- Video: unchanged strategy if the spike confirms it: `EmbeddedPlayer` (libmpv OpenGL render API over EGL in a
  `wl_subsurface` below the transparent Iced window); external `mpv` child process on X11 / failure.
- libmpv is loaded at runtime (`dlopen`), so a missing library degrades playback instead of killing the process.

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
5. **Cut-over**: delete `crates/app` (GPUI), rename `crates/ui` -> `crates/app` (package `nocturne-app`,
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

## Progress

- [x] 1 libmpv / installer (dlopen FFI verified in-window via the spike; installer container tests pending: docker needs sudo)
- [x] 2 spike: GO (notes in ICED_NOTES.md)
- [x] 3 core extraction (crates/core; 270 tests pass)
- [ ] 4a-4e screens
- [ ] 5 cut-over
- [ ] 6 verification
