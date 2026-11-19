# NOCTURNE: development status

Maintainer working notes for the public tree. User-facing status is in
[FEATURES.md](../FEATURES.md) and [QA_REPORT.md](../QA_REPORT.md); plans are in [ROADMAP.md](../ROADMAP.md).

## Release state

**0.5.1 is the latest release (2026-11-19)**, after 0.5.0 the same day; 0.4.0 (2026-11-18) and 0.3.0 before it. Published: GitHub release (tarball, .deb, .rpm, Windows setup.exe and zip, macOS .dmg and, from 0.5.1, the macOS `.tar.gz`, best-effort AppImage), signed pacman, apt and dnf repositories at `https://nocturne.ifsvivek.in/repo`, the website, and the signed update manifest. Contents: [CHANGELOG.md](../../CHANGELOG.md) `[0.5.0]` and `[0.5.1]`; per-feature status in [FEATURES.md](../FEATURES.md); the 0.5 QA pass in [QA_REPORT.md](../QA_REPORT.md). Release flow: [PACKAGING.md](../PACKAGING.md#releasing) (CI builds and drafts, the maintainer signs repositories and update artifacts locally). Gate: `scripts/ci-checks.sh` (fmt, clippy `-D warnings`, tests, `cargo deny`, secret scan of the tree and history).

What 0.5 added: native CSS themes with layout on named parts (`crates/core/src/customize/`, Themes page in `crates/app/src/app/themes.rs`, Marquee and Feed examples, a theme per server); Server UI per server (`core/server_ui.rs`, `app/server_ui.rs`; home-sections editor behind `allow_server_writes`); in-app updates (`core/update.rs`, `core/update_install.rs`, signed with a separate ed25519 key); Linux Wayland-only; apt and dnf repositories; Windows and macOS previews; real text editing in every field; built-in Trakt client id; headless test-session scripts. 0.5.1: the macOS update tarball, theme import creating a new theme (`customize::import_name`), a steadier `scripts/headless-input-check.sh`.

## Still open after 0.5.1

- The updater end to end from a published release (tested with unit and integration tests and a local server); the macOS update path has never run on a Mac.
- Windows and macOS previews on real hardware (build, keyring, mpv IPC, paths, SmartScreen / Gatekeeper behaviour); signing certificates.
- Pointer-only theme actions (Keep, Reset, assigning a per-server theme by click), hover zoom with a real pointer; Server UI against a server with real branding CSS and with several servers; a theme from the Jellyfin web.
- Carried from 0.4: HDR on an HDR display, couch mode with a real gamepad and on a TV, remote control from a phone app, the .deb and .rpm on full desktop installs; Flatpak is undecided.
- Live Trakt scrobble and import were verified in 0.4 against a real account; one-click connect with the built-in id is verified.

## Next: 0.6

Seerr requests, play and save links with yt-dlp, and the native flex/grid theme engine (stage 2, researched on the branch `theme-engine-spike`, not released). Brief: [V06_BRIEF.md](V06_BRIEF.md); order and scope are the maintainer's call, nothing starts without a go-ahead. Small compatibility fixes listed in [ROADMAP.md](../ROADMAP.md) can go first.

## Debug hooks for QA

Themes: `NOCTURNE_KEYS=open-themes@ms,theme-safe@ms,layout-all@ms,layout-2@ms (cycle whose UI),theme-path@ms,theme-import@ms,theme-preview@ms,theme-apply@ms` with `NOCTURNE_THEME_FILE=/path/x.css`. Server UI: `NOCTURNE_KEYS=cycle-filter@ms,layout-1@ms,sections-1@ms,sec-down-0@ms,sec-save@ms`. Real keyboard input: `scripts/headless-session.sh` and `scripts/headless-input-check.sh`.

## Decisions

- Linux first, Wayland only (Arch is the primary tested platform; .deb and .rpm are install-tested in containers). macOS and Windows are unsigned previews built and smoke-tested in CI but never run on real hardware, so neither is claimed as supported ([PLATFORMS.md](../PLATFORMS.md)). Platform-specific code stays inside the seams listed there.
- Iced replaced GPUI; the reasons are in [DECISIONS.md](../DECISIONS.md). `vendor/xattr` and the `[patch]` that only GPUI needed are gone.
- Sources are a two-variant enum (Jellyfin, Local) behind one catalog ([SOURCES.md](../SOURCES.md)); Jellyfin is the first media source, not the last; UI copy avoids hard-coding the product name where practical.
- Out of scope: Live TV/DVR, DLNA, acting as a cast target, gapless music.
- Read-only by default: every server change sits behind one master setting (`allow_server_writes`, default off), routed to the item's origin server. Local files are never modified. See [WRITES.md](../WRITES.md).
- `crates/jellyfin` is GET-only (enforced by `crates/jellyfin/tests/readonly.rs`); admin writes live in `crates/jellyfin-admin` behind a `Confirmed` token; user-scope writes in `crates/jellyfin-user`.
- Tests that write run only against a throwaway Jellyfin (`scripts/test-jellyfin.sh`) and must use isolated `XDG_CONFIG_HOME`/`XDG_DATA_HOME` (`scripts/run-isolated.sh` does this) and an explicit `NOCTURNE_JF_URL`. Never type real credentials into a test window.
- Package and install tests run locally in containers; release artifacts are built by `release.yml` on GitHub runners and the repositories and update artifacts are signed locally ([PACKAGING.md](../PACKAGING.md#releasing)). No history rewrite or force-push.

## Known follow-ups

- Not verified live: several newer player features (resume prompt, A-B loop, audio device list, external subtitles, subtitle presets), 401 mid-session and offline banner on a real server, multi-season data.
- Linux packages are built from one binary compiled in a `debian:stable` container (glibc 2.41); older distributions may refuse to start it.
- In-window video is SDR; the HDR pop-out window is in 0.4 but unverified on an HDR display ([HDR.md](../HDR.md)).
- Screen-reader support is limited by Iced; re-evaluate when it improves.
- 0.4 online details / subtitles: built and unit-tested against a loopback mock; needs a real TMDB key and OpenSubtitles key to verify live; the player loads `LocalBackend::subtitles_of` and offers the finder in its Subtitles menu (checked live with a mock-downloaded file).
- 0.4 offline copies (`core/download`, `app/downloads.rs`): verified live on a throwaway server (movie, episode batch, interrupted download resumed with Range, copy merges as "2 sources", local copy plays). Not checked: a server that really denies downloads (mapped from 403, unit-tested), exFAT/FAT targets (no hard links: falls back to a checked rename), very large files.
- Local scans hold the local library's write lock, so reads of that source wait during a scan (note in `core/folders.rs`).
- `scripts/test-jellyfin.sh`: reject any `..` in `NOCTURNE_JF_TEST_DIR` before `reset` removes the directory.
- Player: mpv's own window key bindings stay active in the external-mpv fallback.
- `crates/jellyfin-user` live test `playback_reporting_sets_resume_then_played` assumes the first movie is a 2 s clip.
- Gaps in the admin section and other media types: see [ROADMAP.md](../ROADMAP.md).
