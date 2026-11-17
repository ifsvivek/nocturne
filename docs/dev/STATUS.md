# NOCTURNE: development status

Maintainer working notes for the public tree. User-facing status is in
[FEATURES.md](../FEATURES.md) and [QA_REPORT.md](../QA_REPORT.md); plans are in [ROADMAP.md](../ROADMAP.md).

## Release state

0.2.0 (GPUI interface, Jellyfin only) is published (tag v0.2.0 and a GitHub Release with the tarball, checksum and
PKGBUILD). **0.3.0 is the current development state**: Iced interface (GPUI removed), local library, several servers
and one unified library, libmpv loaded at run time. Release notes draft: [RELEASE_NOTES_0.3.0.md](RELEASE_NOTES_0.3.0.md).
Release gate: `scripts/ci-checks.sh` (fmt, clippy `-D warnings`, tests, `cargo deny`, secret scan of the tree and history).

0.3.0 release checklist (the maintainer tests an Arch build first; nothing is published before that):
- [ ] `scripts/ci-checks.sh` green on the final tree.
- [ ] Run on Wayland and on X11 (Xwayland): sign in, local folder, play from each, versions and sources picker, quit while playing.
- [ ] Clean-container install of the tarball and the PKGBUILD (`scripts/test-install.sh`); play a video after install.
- [ ] Re-derive the runtime library list for the final binary (winit and wgpu dlopen libX11, libXcursor, libXrandr, libXi,
      libX11-xcb, libwayland-*, libxkbcommon) and keep `packaging/install.sh`, the PKGBUILD and
      [PACKAGING.md](../PACKAGING.md) in step.
- [ ] Put the real sha256 in `packaging/PKGBUILD`; regenerate `THIRD_PARTY_NOTICES.md` (`scripts/gen-notices.py`).
- [ ] Footprint numbers for the release in [PERF.md](PERF.md) (`scripts/perf.sh`) against the 0.2.0 baseline.
- [ ] Repeat the history secret and personal-data audit.
- [ ] Recapture the screenshots from a generated demo library (`docs/screenshots/`, README, website) and set `VERSION` to
      0.3.0 and flip `RELEASED` in `website/site.config.json`; redeploy the site.
- [ ] Set the date in `CHANGELOG.md` (`[0.3.0] - UNRELEASED`), tag, and publish the GitHub release from
      [RELEASE_NOTES_0.3.0.md](RELEASE_NOTES_0.3.0.md) with the checksum.

## 0.4 (local branch `v0.4-local`, never pushed)

0.3.0 is released (tag, pacman repository live). 0.4 is feature-complete on the local branch and not published; no release
until the maintainer has tested it. Contents: [CHANGELOG.md](../../CHANGELOG.md) `[0.4.0]`, honest per-feature status in
[FEATURES.md](../FEATURES.md).

Verified: offline copies (throwaway server), .deb / .rpm install in clean containers, online metadata (TMDB) and OpenSubtitles
search live, Windows cross-compile of the whole app, unit tests for everything pure.
Mock-tested only: Trakt (sign-in, scrobble, import), remote control, watched cascade on two real servers, MusicBrainz / TVmaze.
Cannot be checked here: HDR output (no HDR display, mpv 0.41), gamepad / TV couch mode, anything on real Windows or macOS
(macOS code was never built).

0.4 release checklist:
- [ ] QA walkthrough of every 0.4 feature through `scripts/run-isolated.sh` (throwaway server + demo library); update [QA_REPORT.md](../QA_REPORT.md).
- [ ] Live Trakt test with the maintainer's own Trakt app: sign-in, scrobble, history import, disconnect.
- [ ] HDR hardware test on an HDR display (pop-out colours, fullscreen, IPC control, return to the window).
- [ ] Couch mode with a real gamepad and on a TV.
- [ ] Real-machine tests on Windows and macOS (build, keyring, mpv IPC, paths); installers only after that.
- [ ] Remote control checked from a real Jellyfin Web / mobile client; watched state checked across two real servers.
- [ ] `scripts/ci-checks.sh` green; footprint numbers in [PERF.md](PERF.md) against 0.3.0; history secret audit; `THIRD_PARTY_NOTICES.md` regenerated (new deps: gilrs and others).
- [ ] .deb / .rpm run on real Debian / Fedora machines; decide Flatpak (0.5).
- [ ] Update the website (version, features, download) and the screenshots from a generated demo library; set `VERSION` and `RELEASED`.
- [ ] Date the changelog section, tag, publish only when the maintainer asks.
- Left from the polish list: admin dashboard session label.

## Decisions

- Linux first (Arch is the only tested platform). macOS and Windows code is written behind the seams and compile-checked (`scripts/check-cross.sh`) but never run, so
  neither is claimed as supported; .deb/.rpm/AppImage are separate work ([PLATFORMS.md](../PLATFORMS.md)). Platform-specific code stays
  inside the seams listed there.
- Iced replaced GPUI; the reasons are in [DECISIONS.md](../DECISIONS.md). `vendor/xattr` and the `[patch]` that only GPUI
  needed are gone.
- Sources are a two-variant enum (Jellyfin, Local) behind one catalog ([SOURCES.md](../SOURCES.md)); Jellyfin is the first
  media source, not the last; UI copy avoids hard-coding the product name where practical.
- Out of scope: Live TV/DVR, DLNA, acting as a cast target, gapless music.
- Read-only by default: every server change sits behind one master setting (`allow_server_writes`, default off), routed to
  the item's origin server. Local files are never modified. See [WRITES.md](../WRITES.md).
- `crates/jellyfin` is GET-only (enforced by `crates/jellyfin/tests/readonly.rs`); admin writes live in
  `crates/jellyfin-admin` behind a `Confirmed` token; user-scope writes in `crates/jellyfin-user`.
- Tests that write run only against a throwaway Jellyfin (`scripts/test-jellyfin.sh`) and must use isolated
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME` (`scripts/run-isolated.sh` does this) and an explicit `NOCTURNE_JF_URL`. Never type real
  credentials into a test window.
- Builds and package tests run locally, not on hosted CI. No history rewrite or force-push.

## Known follow-ups

- Not verified live: several newer player features (resume prompt, A-B loop, audio device list, external
  subtitles, subtitle presets), 401 mid-session and offline banner on a real server, multi-season data.
- Binaries built on Arch need a current glibc; the 0.4 .deb / .rpm must be built inside their distribution containers.
- In-window video is SDR; the HDR pop-out window is in 0.4 but unverified on an HDR display ([HDR.md](../HDR.md)).
- Screen-reader support is limited by Iced; re-evaluate when it improves.
- 0.4 online details / subtitles: built and unit-tested against a loopback mock; needs a real TMDB key and OpenSubtitles key to verify live; the player loads `LocalBackend::subtitles_of` and offers the finder in its Subtitles menu (checked live with a mock-downloaded file).
- 0.4 offline copies (`core/download`, `app/downloads.rs`): verified live on a throwaway server (movie, episode batch, interrupted download resumed with Range, copy merges as "2 sources", local copy plays). Not checked: a server that really denies downloads (mapped from 403, unit-tested), exFAT/FAT targets (no hard links: falls back to a checked rename), very large files.
- Local scans hold the local library's write lock, so reads of that source wait during a scan (note in `core/folders.rs`).
- `scripts/test-jellyfin.sh`: reject any `..` in `NOCTURNE_JF_TEST_DIR` before `reset` removes the directory.
- Player: mpv's own window key bindings stay active in the external-mpv fallback.
- `crates/jellyfin-user` live test `playback_reporting_sets_resume_then_played` assumes the first movie is a
  2 s clip.
- Gaps in the admin section and other media types: see [ROADMAP.md](../ROADMAP.md).
