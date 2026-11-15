# NOCTURNE: development status

Maintainer working notes for the public tree. User-facing status is in
[FEATURES.md](../FEATURES.md) and [QA_REPORT.md](../QA_REPORT.md); plans are in [ROADMAP.md](../ROADMAP.md).

## Release state

Version 0.1.0 is prepared but not yet published (no tag, no GitHub Release). Release gate:
`scripts/ci-checks.sh` (fmt, clippy `-D warnings`, tests, `cargo deny`, secret scan of the tree and history).

Before tagging:
- [ ] Re-run `makepkg` `check()` on the final tree; put the real sha256 in `packaging/PKGBUILD`.
- [ ] Regenerate `THIRD_PARTY_NOTICES.md` (`scripts/gen-notices.py`) after any dependency change.
- [ ] Repeat the history secret and personal-data audit.
- [ ] Add sanitized demo-library screenshots (`docs/screenshots/`, website screens).
- [ ] Flip `RELEASED` in `website/site.config.json` and redeploy the site.

## Decisions

- Linux only. Windows and macOS are possible future work, never claimed as supported.
- Jellyfin is the first media source; UI copy avoids hard-coding the product name where practical.
- Out of scope: Live TV/DVR, SyncPlay, DLNA, acting as a cast target, gapless music.
- Read-only by default: every server change sits behind one master setting (`allow_server_writes`,
  default off). See [WRITES.md](../WRITES.md).
- `crates/jellyfin` is GET-only (enforced by `crates/jellyfin/tests/readonly.rs`); admin writes live in
  `crates/jellyfin-admin` behind a `Confirmed` token; user-scope writes in `crates/jellyfin-user`.
- Tests that write run only against a throwaway Jellyfin (`scripts/test-jellyfin.sh`) and must use isolated
  `XDG_CONFIG_HOME`/`XDG_DATA_HOME` and an explicit `NOCTURNE_JF_URL`. Never type real credentials into a
  test window.
- `vendor/xattr` is `xattr` 0.2.3 with a one-line fix (`ENOATTR` to `ENODATA`); remove when upstream builds
  against current `libc`.
- No history rewrite or force-push.

## Known follow-ups

- Not verified live: several newer player features (resume prompt, A-B loop, audio device list, external
  subtitles, subtitle presets), 401 mid-session and offline banner on a real server, multi-season data.
- `scripts/test-jellyfin.sh`: reject any `..` in `NOCTURNE_JF_TEST_DIR` before `reset` removes the directory.
- Player: mpv's own window key bindings stay active in the external-mpv fallback.
- `crates/jellyfin-user` live test `playback_reporting_sets_resume_then_played` assumes the first movie is a
  2 s clip.
- Gaps in the admin section and other media types: see [ROADMAP.md](../ROADMAP.md).
