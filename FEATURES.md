# NOCTURNE — Jellyfin feature parity checklist

Goal (user): everything the Jellyfin web client does should exist in NOCTURNE, as one app.
Legend: [x] done, [~] partial, [ ] todo. Tiers: P0 core, P1 expected, P2 completeness, P3 stretch.
Update this file at every task boundary. Scope risks are listed at the bottom.

## Accounts and connection (P0)
- [~] Login with username/password; session in keyring (crates done: `nocturne-jellyfin-user`, `nocturne-session`; app UI not wired yet)
- [~] Quick Connect (crate done; UI not wired)
- [ ] Server address entry, validation, recent servers, multiple servers
- [ ] User picker / profile switch / sign out
- [~] API-key mode (dev/admin only, via env)
- [ ] Password change, own profile image, display language

## Browsing (P0)
- [x] Home: hero, Continue Watching, libraries, latest per library
- [~] Home: Next Up, My Media, Favorites row, configurable home sections
- [~] Library views: grid, adaptive columns (full width)
- [ ] Sort (name, date added, premiere, rating, runtime, random), filters (played, unplayed, favorite, genre, year, rating, resolution, HDR, subtitles, trailers), A-Z jump, view type (poster/thumb/list)
- [ ] Collections, Playlists, Genres, Studios, People, Years, Tags browse
- [ ] Movies, Shows, Seasons, Episodes, Music (artists/albums/tracks), Music videos, Home videos, Photos, Books, Audiobooks (type coverage)
- [ ] Item detail: overview, cast and crew, similar, extras/trailers, media info (streams), external ratings, chapters, next/previous episode
- [ ] Person page (filmography)
- [ ] Search (global, as-you-type, grouped by type), recently searched
- [ ] Favorites, watched/unwatched toggles (needs user-state writes, see below)
- [ ] Collections/playlist create, add, remove, reorder (user writes)
- [ ] Remote images: posters, backdrops, logos, thumbs everywhere, blurhash placeholders

## Playback (P0/P1)
- [ ] **Feature-rich video player (user, 2026-11-15; not barebone)**: audio/subtitle menus with styling and delays, speed, chapters on scrub bar, trickplay hover thumbnails, skip intro/credits (media segments), next-episode autoplay, aspect/zoom/picture controls, quality/transcode selector with direct-play fallback, stats overlay, shortcut help, OSD, resume prompt, context menu, screenshot, queue panel, MPRIS and idle inhibit
- [x] Direct play via mpv (child process), now-playing bar, seek, volume
- [ ] **PRIORITY (user): in-window GPU video** (mpv video inside the NOCTURNE window with overlay controls, GPU-accelerated, crash-safe). Prototype first, then integrate
- [~] Playback progress reporting (client done in `nocturne-jellyfin-user`; player/app wiring pending), resume position
- [ ] Mark played at end, next-episode autoplay with countdown, skip intro/credits (media segments)
- [ ] Audio track and subtitle track selection, external/embedded/text and image subs, subtitle size/position/delay, audio delay
- [ ] Quality/transcoding selection (bitrate cap, HLS fallback when direct play fails), play method info overlay
- [ ] Chapters and thumbnails on the seek bar (trickplay)
- [ ] Playlists/queue, shuffle, repeat, play all
- [ ] Music player: queue, now playing, album art (gapless dropped)
- [ ] Fullscreen, picture-in-picture-ish mini player, keyboard map, MPRIS (media keys, desktop integration)

## Dropped by user (2026-11-15): Live TV/DVR, SyncPlay, DLNA setup, being a cast target, gapless music. Do not build.

## User settings (P1)
- [ ] Display: language, theme/accent options, home layout, subtitle appearance
- [ ] Playback: preferred audio/subtitle languages, default quality, next-episode and skip behaviour
- [ ] Controls: keyboard shortcut reference
- [ ] Quick Connect authorize, devices/sessions of own account

## Admin dashboard (P1, in progress)
- [x] Admin shell + sidebar; Dashboard, Users (create/disable/admin/password/delete), Libraries list/scan, Scheduled Tasks start/stop, Devices, Activity, API Keys, Logs, Plugins (read), General/Metadata/Resume/Streaming (editable), Branding/Networking (read-only)
- [ ] Admin gaps: Libraries Display/NFO, Transcoding, Trickplay, Backups, editable Branding/Networking, task triggers, plugin install/enable, library add/remove/edit
- [ ] Users: policy details (library access, parental rating, features, remote access, bitrate limits, sync play, live TV), profile image, password policy
- [ ] Libraries: add/remove library, paths, type, metadata providers, image fetchers, real-time monitor, schedule, chapter images, trickplay settings, NFO, subtitles downloaders, languages
- [ ] Playback settings: transcoding (hardware acceleration, ffmpeg path, thread count, tone mapping, throttling), resume, streaming bitrate, trickplay
- [ ] Networking: ports, HTTPS, bind, remote access, proxies, LAN networks
- [ ] Metadata/NFO, subtitles, display settings, branding (login message, CSS read-only/edit)
- [ ] Devices, Activity (filters, paging), Alerts, Logs (all files, download), Scheduled task triggers (add/edit/delete)
- [ ] Plugins: installed, catalog, install/uninstall/enable/disable, repositories
- [ ] API keys, backup/restore (Jellyfin backup API if present), restart/shutdown, server update info

## Platform and quality (P1)
- [ ] Full-window responsive layout, keyboard-first navigation everywhere, focus management and screen-reader labels where GPUI allows
- [ ] Image cache on disk with eviction, offline-tolerant UI, retry/backoff, loading skeletons
- [ ] Error states, empty states, connection-lost handling, update/version compatibility check (10.9 to 12.x)
- [ ] Settings persistence (TOML in XDG config, no secrets), logging with redaction, crash-safe
- [ ] Packaging (AppImage/Flatpak/PKGBUILD), desktop entry, icon, app id
- [ ] Docs and onboarding

## Decisions needed for writes (recorded 2026-11-15)
- Crate split: `nocturne-jellyfin` stays read-only; `nocturne-jellyfin-admin` = admin writes; NEW `nocturne-jellyfin-user` = user-scope writes only (playback progress reporting, played/favorite flags, playlists/collections the user owns, own profile). All write paths tested only on the throwaway server.
- Watch-state sync to the real server is a visible setting ("Sync watch progress and watched state") and defaults ON, because a Jellyfin client that doesn't sync is not feature-complete. It can be switched off.

## Scope risks (be honest)
- In-window GPU video is the hard/uncertain one and is the user's top technical priority: prototype now, in parallel with UI work.
- Everything else is implementable with the Jellyfin REST API; the main cost is UI volume, so shared components (list, filter bar, settings form, dialog, tabs) come first.
