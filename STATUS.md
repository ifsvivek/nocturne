# NOCTURNE — Status

Orchestrator's running log. Update at every task boundary.

## Now
- Task 3 (agent B): `crates/app` GPUI shell, design system, library/detail screens, gpui component lib evaluation — running (parallel, disjoint files)
- After both: review, integrate Play -> player, then polish. Full autonomy granted by user; no need to ask between tasks. Target: usable app within 1 week.

## Done
- [x] Repo: private GitHub repo, README, .gitignore, .env.example
- [x] Task 2: `crates/player` (libmpv2, own-window playback, state+event API, headless tests, live test OK) + Jellyfin base_url validation (reviewed)
- [x] Task 1: workspace + read-only `nocturne-jellyfin` client (reviewed, tests green, pushed)

## Next
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

## Rules
- Jellyfin: GET only; tests print counts/booleans, never titles/paths/ids/tokens.
- Commits: focused, no AI trailers. Max 2 subagents, one at a time by default.
