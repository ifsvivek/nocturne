# Features

What NOCTURNE 0.1.0 does today. `[x]` implemented, `[~]` partial, `[ ]` not implemented.
Verification detail per feature is in [QA_REPORT.md](QA_REPORT.md); remaining work is in [ROADMAP.md](ROADMAP.md).
Which features change data on your server is in [WRITES.md](WRITES.md).

## Accounts and connection
- [x] Sign in with username and password, Quick Connect, sign out, change own password
- [x] Session stored in the system keyring (Secret Service); restored on launch
- [x] Server address entry with validation, remembered servers, forget server
- [x] Session-ended handling (401 returns to sign-in), offline banner with retry
- [~] Developer API-key mode via environment variables (read-only; all writes refused)
- [ ] User picker / profile switching, profile image, display language

## Browsing
- [x] Home: hero, Continue Watching, Next Up, latest per library
- [x] Library views: paged virtualised grid, sort, filters, A-Z jump, poster / thumb / list views
- [x] Genres, Studios, Years, Tags tabs; Collections, Playlists, Favourites pages (read only)
- [x] Search (Ctrl+K or /), grouped results, recent searches
- [x] Detail pages: movies, series (logo hero, season tabs, episode card grid), episodes, people
- [x] Cast, similar items, extras, media info, ratings, chapters, More menu
- [x] Remote images with blurhash placeholders and a size-capped disk cache
- [x] Breadcrumbs, back / forward, Esc / Backspace navigation
- [~] Photos: viewer with prev / next and zoom; folders open as grids
- [~] Music: browsable, with less depth than movies and shows
- [ ] Books and audiobooks: detail page only, not playable
- [ ] HDR filter (the server exposes none)

## Playback
- [x] In-window GPU video through libmpv (Wayland); external mpv window fallback elsewhere
- [x] Seek bar with chapters and trickplay hover thumbnails, volume, fullscreen, mini player
- [x] Audio and subtitle menus, subtitle styling and delay, audio delay, speed, aspect and picture controls
- [x] Skip intro / credits, next-episode autoplay, queue panel
- [x] Quality selector with transcode fallback, playback info overlay, key help overlay, context menu, screenshot
- [x] MPRIS media controls and idle inhibit
- [x] Resume position; progress, watched and favourite sync (needs server changes on, see WRITES.md)
- [x] Network buffering watermarks ([buffering.md](buffering.md))
- [ ] Playlist / collection editing, shuffle and repeat across a queue
- [ ] Music player features (now playing, album art queue)

## Settings
- [x] Settings screen: server-changes master switch, progress sync, buffering, grid density, home sections, language preferences
- [ ] Theme and accent choice, subtitle appearance presets

## Admin (administrator accounts only)
- [x] Dashboard, activity, devices, sessions, logs, plugins (view), backups (list)
- [x] Users: create, disable, delete, admin flag, password reset
- [x] Libraries: list, scan, display / NFO / trickplay flags (view)
- [x] Scheduled tasks: run and stop
- [x] General, Metadata, Resume, Streaming settings (editable); Transcoding (view, three editable fields)
- [x] API keys: create and delete; server restart
- [x] Every write behind a confirmation dialog; view only while server changes are off
- [~] Branding and Networking: view only
- [ ] Task triggers, plugin install / enable, library add / remove / edit, Trickplay edit
- [ ] User policy details (library access, parental rating, bitrate limits)

## Not planned
Live TV / DVR, SyncPlay, DLNA, acting as a cast target, gapless music playback, Windows and macOS builds.

## Platform and quality
- [x] Full-window responsive layout, keyboard-first navigation, loading / empty / error states
- [x] Settings in TOML under `$XDG_CONFIG_HOME/nocturne/`, no secrets on disk
- [x] Tarball and Arch PKGBUILD packaging, desktop entry, icon, AppStream metadata
- [~] Accessibility: keyboard navigation yes; screen-reader labels are limited by GPUI
