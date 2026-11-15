# NOCTURNE — Status

Orchestrator's running log. Update at every task boundary.

## Now
- Task 4 (agent C): Jellyfin reads (resume/genres/backdrop tags/image bytes) + read-only boundary docs, then player integration + now-playing controls — about to delegate
- After both: review, integrate Play -> player, then polish. Full autonomy granted by user; no need to ask between tasks. Target: usable app within 1 week.

## Done
- [x] Repo: private GitHub repo, README, .gitignore, .env.example
- [x] Task 5b: full-window layout (full-bleed hero, landscape Continue Watching/Next Up, adaptive grids) + Admin section (sidebar mirroring Jellyfin: Dashboard, General, Branding, Users, Libraries, Metadata, Resume, Streaming, Devices, Activity, Plugins, Networking, API Keys, Logs, Scheduled Tasks), single confirm choke point (`Admin::on_confirm`), writes verified on throwaway only. Gaps: Libraries Display/NFO, Playback Transcoding/Trickplay, Backups, editable Branding/Networking, web/build version, unplayed badge, session cards unseen with a live session.
- [x] Task 4/4b: playback via mpv, now-playing bar (icons, scrub, volume), Continue Watching, jellyfin reads (resume, image_bytes, user data). In-process libmpv gpu output crashed the app on stop (NVIDIA SIGSEGV), so `nocturne-player` now runs the `mpv` binary as a CHILD PROCESS over JSON IPC (gpu-next, hwdec auto-safe, crash-isolated; headers via IPC not argv). libmpv2 dep removed; the embedding prototype will need libmpv again. Reviewed: 4 smoke tests, 10-cycle windowed run OK.
- [x] Task 5a: `crates/jellyfin-admin` (admin reads, writes, `Confirmed` token for destructive ops) + `scripts/test-jellyfin.sh` throwaway instance on :8097 + 4-layer `TestServerGuard`. 8/8 live tests on throwaway only. Reviewed.
- [x] Task 3: `crates/app` GPUI shell, theme (hairline/mono/blue), home hero, library grid, detail, bridge (tokio runtime). No component lib used: gpui-kit/gpui-component need `gpui-pre`, incompatible with gpui 0.2.2. Reviewed: audit clean, 14 tests, visually matches reference. vendor/xattr committed.
- [x] Task 2: `crates/player` (libmpv2, own-window playback, state+event API, headless tests, live test OK) + Jellyfin base_url validation (reviewed)
- [x] Task 1: workspace + read-only `nocturne-jellyfin` client (reviewed, tests green, pushed)

## Next
- Running now: agent H (id abb600...) in-window video integration + Stage 2 feature-rich player; agent I (id a18c1a...) `jellyfin-user` + `session` crates (login, quick connect, reporting, keyring). Orchestrator commits Cargo.lock.
- [ ] PARITY PROGRAM (user, 2026-11-15): feature-complete vs Jellyfin web, see FEATURES.md (source of truth). Order after current tasks: (1) login/quick connect/keyring + settings persistence, (2) `nocturne-jellyfin-user` (progress reporting, played/favorites, playlists) + player integration, (3) shared components + browse (sort/filter/search/people/collections/genres), (4) detail depth (cast, similar, chapters, tracks/subtitles, skip intro), (5) user settings, (6) remaining admin depth (libraries editor, playback/network settings, plugins, task triggers), (7) music + other media types, (8) in-window GPU video integration. Live TV/SyncPlay/DLNA/cast target/gapless music DROPPED by user. In-window GPU video is the user's TOP priority: prototype (agent G, crate `crates/video-proto`) is running in parallel from now.
- [ ] Task 5: Admin dashboard, READ side: new nav section; server info, activity log, live sessions, devices, scheduled tasks, library status, users list, plugins, logs. Reads go through `nocturne-jellyfin` (or a read module in the admin crate).
- [ ] Task 6: Admin WRITE side in new crate `crates/jellyfin-admin` (library scan, run/stop tasks, user create/edit/disable, session actions, selected settings, restart). Every write behind a confirmation dialog. Tested ONLY against a throwaway Jellyfin instance (`jellyfin --datadir/--configdir` under /tmp, port 8097), never the user's real server.
- [ ] Task 2: libmpv playback crate (`libmpv2` 7.x) — headless smoke test, then app-controlled playback
- [ ] Task 3: GPUI app shell (`gpui` 0.2.x from crates.io) + design system (indigo/navy, grain, see README tagline)
- [ ] Task 4: library grid, detail view, player UI with motion
- [ ] Task 5: keyboard nav, accessibility, image cache

## Decisions
- Media source sits behind a boundary; UI copy must not hardcode "Jellyfin" (product is not Jellyfin-locked).
- Jellyfin client is async (reqwest/rustls); runtime bridging with GPUI is decided in Task 3.
- mpv output: start with libmpv driving its own Wayland window (`vo=gpu-next`); evaluate in-window embedding only after playback works. GPUI has no native GL/video surface.
- Auth header for mpv passed via `http-header-fields`, not `api_key` in URLs.

## Follow-ups
- LAYOUT (user, 2026-11-15): app must use full window width/height (no 1200px column); full-bleed hero, landscape Continue Watching/Next Up rows, adaptive grids. Assigned to the Admin-UI engineer as its first step. Add `/Shows/NextUp` read.
- Player: mpv's own window keybindings stay active (q quits the mpv window); `--no-config` ignores the user's mpv.conf. Decide later if the embedding prototype changes this.
- `scripts/test-jellyfin.sh`: `/tmp/?*` case pattern also matches `/tmp/..`; reject any `..` in NOCTURNE_JF_TEST_DIR before `reset` does rm -rf.
- Git: origin has pre-amend commit 44c389d; merge origin/main (no force-push) after Task 4 finishes, audit full range before pushing.
- `jellyfin-admin` depends on `nocturne_jellyfin::Config`; keep its fields stable.
- Windowed (non-headless) mpv with gpu-next not yet exercised; verify during integration.
- `Client::auth_header()` is the value only; callers prefix `Authorization: ` for `Player::load`.
- Cargo.lock to be committed after Task 3 lands.
- `first_user_id()` needs a user picker if several users exist.

## Decisions from user (2026-11-15)
- In-window video is the intended final UX, but NOT before browse/detail/playback controls/polish are done. Then: GPUI/libmpv embedding feasibility prototype; report trade-offs before committing. External mpv window = temporary fallback only.
- `vendor/xattr` = xattr 0.2.3 with a one-line patch (ENOATTR -> ENODATA) because libc dropped ENOATTR on Linux; gpui -> zed-async-tar needs it. Wired via `[patch.crates-io]`; must be committed for reproducible builds. Remove when upstream fixes it. Linux-only.
- No history rewrite / force-push; old trailer commits stay. Clean commits from now on.
- Before every push: inspect staged changes AND the resulting diff for credentials, tokens, server addresses, personal filenames, artwork, identifying data. Counts-only logs.
- Read-only boundary must be documented in the Jellyfin service layer (crate docs + guard test message) so write calls aren't added by accident. (To do in next task.)
- Scope: reliable polished core first. Tracked follow-ups: subtitles, audio-track selection, watched-state sync (needs write calls; separate explicit decision), hardware-accelerated embedded video.

- Admin (user, 2026-11-15): wants a full admin dashboard, reads AND writes with confirmations. `nocturne-jellyfin` stays strictly read-only; writes live only in `crates/jellyfin-admin`. Real-server writes are not tested; use a throwaway instance.

- Single app only (user): admin is an Admin section inside the NOCTURNE app (shown only to Jellyfin administrators), not a separate binary. `jellyfin-admin` is a library crate.

## Rules
- Jellyfin: GET only; tests print counts/booleans, never titles/paths/ids/tokens.
- Commits: focused, no AI trailers. Max 2 subagents, one at a time by default.
