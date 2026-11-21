# 0.6.0 plan

Maintainer decisions (2026-11-21): one release, 0.6.0, with every feature below; at most three workers at a time; docs, README and website updated and deployed with the release. Background: [V06_BRIEF.md](V06_BRIEF.md) and the 0.6 to 0.8 entries in [ROADMAP.md](../ROADMAP.md).

## Features

1. **Player choice.** Settings > Video player > Player: In-window (default) / VLC / mpv window, offered only when installed. VLC runs with a loopback control interface protected by a random password, so NOCTURNE keeps resume position, progress sync, Trakt scrobbling and Stop. A new backend beside the mpv ones in `crates/player`, behind the same interface the app already uses; the choice logic lives in `crates/core/src/video.rs`. MPC-HC (Windows) and IINA (macOS) come later, once someone can try them.
2. **Calendar.** A Calendar page (header link) with month, week and agenda views of upcoming episodes: each server's upcoming list (a new GET in `crates/jellyfin`) merged across servers, plus the Trakt calendar when Trakt is connected. Each entry: poster, season and episode, title, air time in the local timezone and 12/24 h format (`video::clock_text`), marked on your server / airing soon / not on your servers; a click opens the show or plays the episode. An optional "Coming this week" Home row (setting). Read only.
3. **Seerr requests.** Request movies and shows from search, detail pages and the Calendar, and a Requests page (requested, pending, available). A request is a write to the Seerr server: its own setting (off by default) and a confirmation; nothing is sent to a Jellyfin server. Research first (`docs/dev/SEERR_PLAN.md`: API, sign-in, permissions, 4K, seasons), reviewed by the master before any code. Tested only against a throwaway Seerr in Docker on loopback.
4. ~~**Play links with yt-dlp.**~~ Dropped (maintainer, 2026-11-21): yt-dlp now needs a JavaScript runtime for YouTube. Paste a video address in Search and play it in NOCTURNE's player through mpv's yt-dlp hook (`ytdl=yes`; check the embedded libmpv loads it). On by default. **Save to library** (off by default): yt-dlp saves into a chosen local folder through the downloads queue, neutral wording, a note to save only what you have the right to keep. yt-dlp is optional and never bundled; a missing yt-dlp shows the install command for the system.
5. **Themes gallery.** `themes/<name>/` in this repository (`theme.css`, `preview.webp` from demo data, `theme.toml`: name, author, licence, description, kind official/community). The website build writes `update/themes.json` (name, description, author, licence, preview URL, CSS URL, sha256). Settings > Themes gains tabs Official / Community / My themes with previews and one-click install (respects the remote CSS switch; downloads checked against the sha256). Themes added by hand keep working, marked "not from the gallery".
6. **Flatpak evaluation.** A report (`docs/dev/FLATPAK.md`): manifest sketch, portals and permissions (Wayland, GPU, keyring through the Secret portal, local folders, mpv/libmpv inside the sandbox, external players), size, update path; no shipping without the maintainer.

## Work order

| Wave | Workers | Files they own |
|---|---|---|
| 1 | Player choice | `crates/player` (new backend), `crates/core/src/video.rs`, the playback part of settings, player wiring in `crates/app/src/app/play*.rs` |
| 1 | Calendar | new `crates/core/src/calendar.rs`, the upcoming GET in `crates/jellyfin`, Trakt calendar in `crates/core/src/trakt.rs`, new `crates/app/src/app/calendar*.rs`, the header link |
| 1 | Seerr research | `docs/dev/SEERR_PLAN.md` only, then stop for review |
| 2 | Seerr build | new `crates/seerr`, the Requests page, request buttons |
| 2 | Themes gallery + Flatpak report | `themes/`, website index script, the Themes page tabs |
| 3 | Capture, independent visual review, fixes, docs, README, website, release | |

Shared files (`crates/core/src/settings.rs` rows and `cycle()` arms, `crates/app/src/app/message.rs`, `header_tree.rs`, `CHANGELOG.md`): add new entries at the end of the relevant list; the master resolves conflicts when merging.

## Rules for every task

Read CLAUDE.md first. `crates/jellyfin` stays GET only. Writes (Seerr requests) behind their own setting and a confirmation. Demo data and throwaway servers only; never the maintainer's servers. GUI only in named headless sessions; real input through `scripts/headless-session.sh pointer` and a virtual keyboard. Never kill processes by name. Stop the sessions, servers and containers you started. Each worker proves its feature with unit tests and headless screenshots, runs fmt, clippy `-D warnings` (default and `--no-default-features`), `cargo test --workspace --locked` (both), `crates/jellyfin/tests/readonly.rs`, `scripts/headless-input-check.sh` and `scripts/headless-parity.sh`, writes CHANGELOG lines under [Unreleased], commits focused commits without AI trailers, and never pushes.
