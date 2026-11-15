# Features

What NOCTURNE 0.2.0 does today. `[x]` implemented, `[~]` partial, `[ ]` not implemented.
Verification detail per feature is in [QA_REPORT.md](QA_REPORT.md); remaining work is in [ROADMAP.md](ROADMAP.md).
Which features change data on your server is in [WRITES.md](WRITES.md).

## Accounts and connection
- [x] Sign in with username and password, Quick Connect, sign out, change own password
- [x] Session stored in the system keyring (Secret Service); restored on launch
- [x] Server address entry with validation, remembered servers, forget server
- [x] Session-ended handling (401 returns to sign-in), offline banner with retry
- [~] Developer API-key mode via environment variables (read-only; all writes refused)
- [x] Authorize another device with Quick Connect; Your devices (read only list of this account's sessions)
- [x] Switch between remembered accounts of a server without a password (tokens in the keyring, `accounts.toml` holds names and the public profile-image tag, never a token); the profile picture shows in the header avatar, the account menu, the switcher and the Admin Users list; add another account; own profile image set / remove; display language (the `language` entry other Jellyfin apps share). Sign out forgets the account
- [x] Profile picker ("Who's watching?") at launch when the server has two or more remembered accounts: round profile tiles with picture or initial, Add profile tile, mouse / arrows / Enter, local sign-in with the stored token (a rejected token opens the sign-in form with the name filled in); "Use as default profile" skips it at the next launches; account menu "Switch profile" and a Settings "Default profile" drop-down; the default is a name marker in `accounts.toml`, cleared when that account is removed; no server writes, not shown in developer-token mode or with a single account

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
- [~] Music: browsable; a session music queue (Play next, Add to queue from albums, tracks and playlists; reorder, remove, shuffle, repeat in the player queue panel). No album-art queue view
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
- [x] Playlists you own: create (N on the Playlists page), add from any item or album (More), remove and reorder entries (x, [ ], ]), rename, delete (typed name); collections stay read only
- [x] Music queue: queue panel (u) with reorder, remove, shuffle and repeat; kept for the session and started from the account menu

## Settings
- [x] Settings screen: server-changes master switch, progress sync, buffering, grid density, home sections, language preferences
- [ ] Theme and accent choice, subtitle appearance presets

## Admin (administrator accounts only)
- [x] Dashboard, activity, devices, sessions, logs, plugins (view), backups (list)
- [x] Users: create, disable, delete, admin flag, password reset
- [x] Libraries: list, scan; add, remove, add / remove folders; toggle real-time monitor, trickplay and chapter images, NFO saving, embedded titles, internet providers; automatic refresh interval
- [x] Scheduled tasks: run, stop; triggers (daily, weekly, interval, on startup): add, edit, delete
- [x] Trickplay settings: interval, widths, tile size, quality, threads, scan behaviour, priority, hardware flags
- [x] Branding: login message, custom CSS, splash screen
- [x] Networking (safe subset): ports, bind addresses, LAN networks, remote access, UPnP, discovery, IPv4 / IPv6; typed confirmation and a lock-out warning on every change; base URL, HTTPS and certificates, proxies and IP filters stay view only
- [x] Plugins: enable, disable, uninstall (installed from outside), install from the catalog; takes effect after a restart (bundled plugins cannot be changed)
- [x] General, Metadata, Resume, Streaming settings (editable); Transcoding (view, three editable fields)
- [x] API keys: create and delete; server restart
- [x] Every write behind a confirmation dialog; view only while server changes are off
- [x] Plugin repositories: list, add, enable, disable, remove (adding one warns that plugins run with full server access)
- [x] Libraries: rename; Providers page with the metadata and image fetchers the server offers per library and item type (order, on / off), local metadata readers, and what the server lacks (subtitle, lyric and segment providers shown read only); refresh schedule shown (metadata refresh days per library, the server-wide scan task's triggers). A library's type cannot be changed
- [x] Access and limits per user: library access, feature switches (remote access, playback, transcoding, remuxing, downloads, deletion, collections, subtitles, public sharing, hidden), maximum parental rating, blocked and allowed tags, remote bitrate limit, session limit, failed-sign-in lockout, profile image upload / remove, display preferences (read only). Jellyfin has no password length or expiry rules, so none are offered

## Not planned
Live TV / DVR, DLNA, acting as a cast target, gapless music playback, Windows and macOS builds.

## Platform and quality
- [x] Full-window responsive layout, keyboard-first navigation, loading / empty / error states
- [x] Settings in TOML under `$XDG_CONFIG_HOME/nocturne/`, no secrets on disk
- [x] Tarball and Arch PKGBUILD packaging, desktop entry, icon, AppStream metadata
- [x] Keyboard shortcut reference: `?` on any browsing screen (the player has its own `?` with playback keys)
- [~] Accessibility: keyboard navigation and visible focus on controls yes; screen-reader labels are limited by GPUI
