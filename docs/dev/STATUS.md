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

## Decisions

- Linux first (Arch is the only tested platform). macOS and Windows, and .deb/.rpm/AppImage, are planned for 0.4 but not
  started and never claimed as supported until they exist ([PLATFORMS.md](../PLATFORMS.md)). Platform-specific code stays
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
- In-window video is SDR; the HDR pop-out window is 0.4 ([HDR.md](../HDR.md)).
- Screen-reader support is limited by Iced; re-evaluate when it improves.
- Local scans hold the local library's write lock, so reads of that source wait during a scan (note in `core/folders.rs`).
- `scripts/test-jellyfin.sh`: reject any `..` in `NOCTURNE_JF_TEST_DIR` before `reset` removes the directory.
- Player: mpv's own window key bindings stay active in the external-mpv fallback.
- `crates/jellyfin-user` live test `playback_reporting_sets_resume_then_played` assumes the first movie is a
  2 s clip.
- Gaps in the admin section and other media types: see [ROADMAP.md](../ROADMAP.md).
