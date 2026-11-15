# NOCTURNE — Status

Orchestrator's running log. Update at every task boundary.

## Now
- Task 4 (agent C): Jellyfin reads (resume/genres/backdrop tags/image bytes) + read-only boundary docs, then player integration + now-playing controls — about to delegate
- After both: review, integrate Play -> player, then polish. Full autonomy granted by user; no need to ask between tasks. Target: usable app within 1 week.

## Done
- [x] Repo: private GitHub repo, README, .gitignore, .env.example
- [x] Task 11: browse parity (library toolbar: sort/filters/views/A-Z, paged virtualised grid, tabs for genres/studios/years/tags/collections/favourites, global search Ctrl+K, detail pages with logo hero, episode CARD GRID, cast carousel, chapters, similar/extras, media info, person pages, playlists/collections/albums lists, blurhash + disk image cache, history/breadcrumbs). Only Home was viewed live; user must eyeball the rest. Gaps: More menu, Next Up on series, Photo viewer, Book pages, HDR filter, Season/BoxSet as cards.
- [~] Task 10: feature-rich player (skip intro/credits, trickplay, next-episode autoplay, queue, quality/transcode fallback, picture/audio controls, subtitle presets, MPRIS, idle inhibit, mini-player, context menu) on branch `player-stage2`, not yet merged. Live-unverified: resume prompt, favourite heart w/ hooks, A-B loop, audio device list, idle inhibitor registration, direct-play fallback, external subs, subtitle presets.
- [x] Task 9: accounts wired in app (Connect, Login, Quick Connect, keyring restore, account menu, change password, Settings screen + settings.toml, sync via `SyncHooks`, played/favourite chips and keys w/f). API-key dev mode is read-only (writes refused). Reporting is driven from `Root::tick` (player must NOT call started/progress/stopped hooks). Gaps: 401 mid-session -> Login, forget-server action, mouse click on chips untested live. `hooks.rs` placeholder to delete after player lands.
- [x] Task 8: in-window GPU video (EmbeddedPlayer, wl_subsurface + libmpv GL render, external mpv fallback, 20/20 cycles) + stage 2 part 1 (menus, speed, chapters, aspect, delays, stats/help overlays, two-row bar).
- [x] Task 6a: `nocturne-jellyfin-user` (login by name, Quick Connect, logout, playback reporting, played/favorite, playlists, own password w/ UserConfirmed) + `nocturne-session` (keyring Secret Service store, servers.toml URL list, no file fallback) + throwaway script now seeds Quick Connect and known alice/bob passwords. 7/7 live tests on throwaway; reviewed. NOT yet wired into the app.
- [x] Task 5b: full-window layout (full-bleed hero, landscape Continue Watching/Next Up, adaptive grids) + Admin section (sidebar mirroring Jellyfin: Dashboard, General, Branding, Users, Libraries, Metadata, Resume, Streaming, Devices, Activity, Plugins, Networking, API Keys, Logs, Scheduled Tasks), single confirm choke point (`Admin::on_confirm`), writes verified on throwaway only. Gaps: Libraries Display/NFO, Playback Transcoding/Trickplay, Backups, editable Branding/Networking, web/build version, unplayed badge, session cards unseen with a live session.
- [x] Task 4/4b: playback via mpv, now-playing bar (icons, scrub, volume), Continue Watching, jellyfin reads (resume, image_bytes, user data). In-process libmpv gpu output crashed the app on stop (NVIDIA SIGSEGV), so `nocturne-player` now runs the `mpv` binary as a CHILD PROCESS over JSON IPC (gpu-next, hwdec auto-safe, crash-isolated; headers via IPC not argv). libmpv2 dep removed; the embedding prototype will need libmpv again. Reviewed: 4 smoke tests, 10-cycle windowed run OK.
- [x] Task 5a: `crates/jellyfin-admin` (admin reads, writes, `Confirmed` token for destructive ops) + `scripts/test-jellyfin.sh` throwaway instance on :8097 + 4-layer `TestServerGuard`. 8/8 live tests on throwaway only. Reviewed.
- [x] Task 3: `crates/app` GPUI shell, theme (hairline/mono/blue), home hero, library grid, detail, bridge (tokio runtime). No component lib used: gpui-kit/gpui-component need `gpui-pre`, incompatible with gpui 0.2.2. Reviewed: audit clean, 14 tests, visually matches reference. vendor/xattr committed.
- [x] Task 2: `crates/player` (libmpv2, own-window playback, state+event API, headless tests, live test OK) + Jellyfin base_url validation (reviewed)
- [x] Task 1: workspace + read-only `nocturne-jellyfin` client (reviewed, tests green, pushed)

## PUBLIC RELEASE PROGRAM (user directive, 2026-11-15)
Goal: complete, polished, publicly releasable Linux Jellyfin client; public GitHub repo; tested artifacts; GitHub Release. Do not publish until the security audit passes. No history rewrite/force-push without explicit approval; old Co-Authored-By trailers stay.
Decisions: License = GPL-3.0-or-later (user). WRITE POLICY RECONCILED: all server writes (progress sync, played, favourite, playlists, admin actions) are OFF by default behind ONE master setting "Allow NOCTURNE to change data on the server" (default OFF; user opts in explicitly = authorization; dev API-key mode always refuses writes). Sync and admin writes additionally keep their own toggle/confirmations. Docs must state which features need writes. Real-server tests stay read-only; writes only on throwaway.
History audit 1 (2026-11-15): 62 commits, 4 branches, 0 tags: no .env/keys/images/media; real token string occurs 0 times. REPEAT before publishing.
RELEASE TIMING (user, 2026-11-15): NO tag, GitHub Release, visibility change or artifact upload until EVERYTHING is implemented and the final quality gate passes. Repo stays private until then. Packaging/docs/screenshot work may be prepared locally meanwhile.
Checklist (check only with evidence): [ ] clean-checkout build [ ] launch [ ] login/Quick Connect [ ] browse real library read-only [ ] details/artwork [ ] search [ ] playback in-window [ ] X11 fallback check [ ] visible controls all work/no placeholders [ ] error/loading/empty states [ ] write master switch implemented [ ] repo cleanup [ ] history audit repeated [ ] LICENSE + THIRD_PARTY_NOTICES [ ] README/docs/CONTRIBUTING/SECURITY/CHANGELOG/CoC [ ] demo-library screenshots [ ] repo metadata/topics [ ] packages (tarball, PKGBUILD, AppImage if feasible) built+install-tested [ ] checksums+release notes [ ] public + tag + Release.
## Next
- AUTHORIZATION (user, 2026-11-15): items A-G of the remaining-work list are APPROVED, INCLUDING publishing: repo public, repo metadata/topics, tag, GitHub Release, website release state. Gates still apply: everything implemented, final quality gate, security/history audit PASS, licensing OK; on a real secret or licensing blocker STOP and report. Item H (phase 2 research) stays LOCAL ONLY: not pushed, nothing filed upstream. User returns ~9-10pm IST to give next instructions.
- SCOPE (user, 2026-11-15): Linux only. No Windows or macOS builds for now; mention as possible future work only, never as supported. Site/README/tagline stay Linux-focused.
- WEBSITE (user, 2026-11-15): host a project site as a Cloudflare Worker on subdomain `nocturne.ifsvivek.in` (zone ifsvivek.in is on the owner's Cloudflare; DNS name verified free; do NOT touch other records, esp. `jelly.`). Plan: new folder `website/` (static Worker, brand-consistent: near-black/indigo/blue, hairline, mono labels), built into a single `dist/worker.js` by a small script; orchestrator deploys through the Cloudflare API (custom domain on the Worker, same pattern as the owner's other Workers). No secrets/account IDs in the repo (use env). Content uses only brand visuals until sanitized demo-library screenshots exist; no personal media. Download/GitHub links stay hidden behind a `RELEASED=false` flag until the release step; flip at release. Delegate the build to a subagent when a slot frees (two agents are running).
- Running: Task 12 (agent ae13fa..., crates/player only): buffer watermarks high 300s/low 120s (demuxer-readahead-secs + demuxer-hysteresis-secs, max_bytes 1.5GiB) + `State.buffered`; app must plumb Settings buffer_high/low later and fix State initialisers.
- (done, see git log) MERGE PENDING: branch `player-stage2` (worktree /home/ifsvivek/Projects/NOCTURNE-player, 3 commits, reviewed: 92 tests, fmt/clippy clean). Player UI was extracted from `Root`/app.rs into `PlayerView` (player_view.rs, player_ui.rs, player_logic.rs, player_mpris.rs); source.rs gets `Source::client()`; playback.rs deleted. Must be merged AFTER the browse agent (running, edits app.rs/source.rs) finishes. Merge agent must: resolve app.rs conflicts, re-point `Root::tick` reporting (SyncHooks) at PlayerView, pass real `PlayerHooks` + `Settings::prefs()` (PlayerPrefs from video.rs) into `PlayerCtx`, delete `hooks.rs`, remove the worktree, regenerate Cargo.lock (zbus).
- Running now: agent H (id abb600...) in-window video integration + Stage 2 feature-rich player; (agent I done). Orchestrator commits Cargo.lock after H finishes. NEXT after H: app wiring (login/Quick Connect screens, session restore from keyring, server picker, playback reporting + played/favorite actions with sync toggle, settings persistence), then browse parity (sort/filter/search/people/collections/genres/playlists), detail depth, user settings, admin gaps.
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

## Release program progress (engineer: app/player merge)
- Merged player-stage2; buffering settings; master write switch (docs/WRITES.md); 401 -> sign-in, offline banner + backoff; next up, More menu, forget server, collection cards, welcome steps.
- NOT yet verified live: 401 mid-session, server stop/start banner, admin writes ON, real-server read-only browse/playback. Photo viewer / Book pages stay basic detail pages. Admin page does not itself trigger the 401 path (it polls its own client).
