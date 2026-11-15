# NOCTURNE — Status

Orchestrator's running log. Update at every task boundary.

## Now
- Task 4 (agent C): Jellyfin reads (resume/genres/backdrop tags/image bytes) + read-only boundary docs, then player integration + now-playing controls — about to delegate
- After both: review, integrate Play -> player, then polish. Full autonomy granted by user; no need to ask between tasks. Target: usable app within 1 week.

## Done
- [x] Repo: private GitHub repo, README, .gitignore, .env.example
- [x] Task 3: `crates/app` GPUI shell, theme (hairline/mono/blue), home hero, library grid, detail, bridge (tokio runtime). No component lib used: gpui-kit/gpui-component need `gpui-pre`, incompatible with gpui 0.2.2. Reviewed: audit clean, 14 tests, visually matches reference. vendor/xattr committed.
- [x] Task 2: `crates/player` (libmpv2, own-window playback, state+event API, headless tests, live test OK) + Jellyfin base_url validation (reviewed)
- [x] Task 1: workspace + read-only `nocturne-jellyfin` client (reviewed, tests green, pushed)

## Next
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

## Rules
- Jellyfin: GET only; tests print counts/booleans, never titles/paths/ids/tokens.
- Commits: focused, no AI trailers. Max 2 subagents, one at a time by default.
