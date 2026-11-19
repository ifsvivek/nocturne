# Features

What NOCTURNE does: 0.3.0 is released; entries marked **(0.4)** are on the unreleased local 0.4 branch. `[x]` implemented, `[~]` partial, `[ ]` not implemented. Verification tags on 0.4 items: *verified* (run against a real or throwaway server / real data), *mock-tested* (unit tests and loopback mocks only), *needs hardware / account* (cannot be checked here). Verification detail per feature is in [QA_REPORT.md](QA_REPORT.md); remaining work is in [ROADMAP.md](ROADMAP.md). Which features change data on your server is in [WRITES.md](WRITES.md).

## Sources: local folders and several servers
- [x] First-run setup flow (5 steps fresh, 2 for upgraders): Local library, Connect to a server, or Both; Settings > Run setup again
- [x] Single instance: a second launch raises the running window
- [x] Several Jellyfin servers at once, each with its own account; a source that is signed out or offline never blocks the others (quiet banner, retry with backoff)
- [x] Settings > Sources: status, sign in again, rescan, enable / disable, priority order, remove (says what is deleted)
- [x] One unified library: Home, browse and search merge sources; a header filter narrows to one source; small source marks on cells and detail pages
- [x] Duplicates shown once when provider ids (IMDb / TMDB / TVDB) or series id + season + episode agree; title alone never merges
- [x] Versions and sources picker on the detail page: a server's own versions plus copies in other sources, with "Always prefer this source"
- [x] Local folders: add by portal chooser or typed path, scan with live progress and cancel, availability of unplugged drives, counts
- [x] Local metadata from names, `.nfo`, sidecar art and `ffprobe` (if installed); Unmatched list with manual match; your files are never modified
- [x] Local playback with resume position and watched state kept in NOCTURNE's own data
- [x] (0.4) Online details for local media, opt-in per source: TMDB (your own key), TVmaze, MusicBrainz + Cover Art Archive; unclear matches listed, never guessed; provider credits shown ([METADATA.md](METADATA.md)). Live-tested with TMDB; MusicBrainz and TVmaze against mocks
- [x] (0.4) Subtitle download for local files (OpenSubtitles, your own key, opt-in), stored in NOCTURNE's folder. Search live-tested; actual download needs an OpenSubtitles login and is mock-tested
- [x] (0.4) Offline copies: "Download to this computer" for a server movie, episode, season or show; GET only, resumable, shows up as a second local source. Verified on a throwaway server; exFAT/FAT targets and servers that deny downloads untested live
- [x] (0.4) Smart collections: saved rules (watched state, genres, years, rating, added within N days, resolution, runtime, sources) over every source; local only. Unit-tested; resolution rules apply to servers only
- [x] (0.4) One watched state across sources (opt-in, write-gated), matched by provider ids only. Verified on a throwaway server with a local copy of the same title (setting off: nothing follows; on with server changes: the server copy follows; on without: only local copies; unrelated titles never touched). Not yet tried with two real servers
- [x] (0.4) Optional extra HTTP header per server (keyring). Verified on a throwaway server
- [x] (0.5) Server UI per server (Settings > Sources > Layout): Home in the server's own section order with a My media row and the server's library order and hidden libraries, and the server's branding CSS as that server's theme layer; a home-sections editor (reorder, on / off) that writes back only the `homesection` entries, behind server changes. Verified on a throwaway server (layout, editor with writes off and on, reload); not tried against a server with custom branding CSS or Live TV. Not shown natively: Live TV and recordings rows, audio and book continue rows, web-client plugins

## Accounts and connection
- [x] Sign in with username and password, Quick Connect, sign out, change own password
- [x] Sessions stored in the system keyring (Secret Service), one per server account; restored on launch
- [x] Server address entry with validation, remembered servers, forget server
- [x] Session-ended handling (401 returns to sign-in), offline banner with retry
- [~] Developer API-key mode via environment variables (a transient read-only source, never saved; all writes refused; `.env` is read by debug builds only)
- [x] Watch together (SyncPlay): list, create, join and leave groups on the server; play, pause, seek, stop and next stay in step across members (clock sync, drift correction, Ready / Buffering handshake). `g` or the account menu; works with the embedded and the external mpv player. Needs server changes on. See [syncplay.md](syncplay.md)
- [x] Authorize another device with Quick Connect; Your devices (read only list of this account's sessions)
- [x] Switch between remembered accounts of a server without a password (tokens in the keyring, `accounts.toml` holds names and the public profile-image tag, never a token); the profile picture shows in the header avatar, the account menu, the switcher and the Admin Users list; add another account; own profile image set / remove; display language (the `language` entry other Jellyfin apps share). Sign out forgets the account
- [x] Profile picker ("Who's watching?") at launch when the server has two or more remembered accounts: round profile tiles with picture or initial, Add profile tile, mouse / arrows / Enter, local sign-in with the stored token (a rejected token opens the sign-in form with the name filled in); "Use as default profile" skips it at the next launches; account menu "Switch profile" and a Settings "Default profile" drop-down; the default is a name marker in `accounts.toml`, cleared when that account is removed; no server writes, not shown in developer-token mode or with a single account

## Browsing
- [x] Home: hero, Continue Watching, Next Up, latest per library
- [x] (0.4) Latest rows include new episodes of existing shows; Play on a show card starts the next unwatched episode; "Included in" collections on movie and show pages (Jellyfin 12+)
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
- [x] In-window GPU video through libmpv with hardware decoding (Wayland); external mpv window as a fallback. Linux is Wayland-only (X11 sessions are refused with a message)
- [x] libmpv is loaded at run time: without it the app still starts and falls back to the `mpv` program or shows what to install
- [x] Player loading overlay (Stremio-style: backdrop, logo filling with the buffer) when a stream opens, a lighter version on stalls
- [~] HDR video is tone-mapped to SDR in the window. (0.4) HDR pop-out window (mpv `gpu-next`, fullscreen, driven from the app) with an Ask / SDR / Pop-out setting: needs HDR hardware, **no HDR display has been tested** ([HDR.md](HDR.md))
- [x] (0.4) Secondary subtitle; audio and subtitle language remembered per show across restarts (local file, "Original audio" option); player menus by keyboard (T, V, O, Q). Built and unit-tested; not checked against a long real series
- [x] (0.4) Truthful playback reports (play method, media source, session id), visible notice on failure, final "stopped" retried once
- [x] (0.4) Trakt (opt-in, one-click connect with the built-in client id; your own Trakt app stays an advanced option): scrobble and watched-history import. Verified live against a real Trakt account (2026-11-17): browser sign-in with PKCE through the local redirect `http://127.0.0.1:38917/callback`, username, history read, scrobble start/stop ([TRAKT.md](TRAKT.md))
- [~] (0.4) Remote control from other Jellyfin apps (opt-in, needs server changes on). Verified on a throwaway 12.x server: the session shows as controllable, and Play, Pause, Unpause, Seek, Next and Stop sent through the server's API took effect; with the setting off the session is not controllable. Not yet tried from a phone app ([WRITES.md](WRITES.md))
- [~] (0.4) Couch mode: 10-foot layout, keyboard / remote-key navigation, gamepad (`gilrs`). Layout and key paths checked; **no real gamepad or TV tested**
- [x] Seek bar with chapters and trickplay hover thumbnails, volume, fullscreen, mini player
- [x] Audio and subtitle menus, subtitle styling and delay, audio delay, speed, aspect and picture controls
- [x] Skip intro / credits, next-episode autoplay, queue panel
- [x] Quality selector with transcode fallback, playback info overlay, key help overlay, context menu, screenshot
- [x] MPRIS media controls and idle inhibit
- [x] Resume position; progress, watched and favourite sync (needs server changes on, see WRITES.md)
- [x] Network buffering watermarks ([buffering.md](buffering.md))
- [x] Playlists you own: create (N on the Playlists page), add from any item or album (More), remove and reorder entries (x, [ ], ]), rename, delete (typed name); collections stay read only
- [x] Music queue: queue panel (u) with reorder, remove, shuffle and repeat; kept for the session and started from the account menu

## Updates

- At most one check a day (setting `check_updates`, on by default) tells you when a newer version exists. Package-managed installs show the command (`sudo pacman -Syu`, `sudo apt update && sudo apt upgrade`, `sudo dnf upgrade`) and never touch themselves.
- Per-user installs (tarball under `~/.local`, Windows installer, macOS app in a folder you can write) update in the app: Download and install, then Restart to update. The download is checked against the announced size, its sha256 and an ed25519 signature made with the NOCTURNE update key before anything is replaced; if any step fails the current version stays as it was.

## Settings
- [x] Settings screen: server-changes master switch, progress sync, buffering, grid density, home sections, language preferences, reduce motion (follows the desktop by default)
- [x] Themes (Settings > Themes): colours, corner radii, window background and font family from imported CSS, global with an optional per-server override, 15 second preview. Layout CSS on named parts (cards, rows, Home sections and hero, header, buttons, player bar) with built-in Marquee and Feed examples; density, type size (whole-UI zoom) and backdrop blur (blurhash stand-in) are applied approximately; the font applies from the next launch. Unit-tested and checked in the app on a local library; not tried against a real Jellyfin theme from the web or two servers at once ([CUSTOMIZATION_PLAN.md](dev/CUSTOMIZATION_PLAN.md))
- [ ] Subtitle appearance presets

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
Live TV / DVR, DLNA, acting as a cast target, gapless music playback.

## Platform and quality
- [x] Full-window responsive layout, keyboard-first navigation, loading / empty / error states
- [x] Settings in TOML under `$XDG_CONFIG_HOME/nocturne/`, no secrets on disk
- [x] Tarball and Arch PKGBUILD packaging, desktop entry, icon, AppStream metadata; the tarball installer installs missing runtime libraries
- [x] Keyboard shortcut reference: `?` on any browsing screen (the player has its own `?` with playback keys)
- [~] (0.4) .deb and .rpm packages: built and install-tested in clean distro containers; not run on a bare-metal Debian / Fedora install. AppImage is not shipped (107 MB, libmpv / glibc conflicts); Flatpak to be evaluated ([PLATFORMS.md](PLATFORMS.md))
- [~] (0.4) Windows and macOS: code behind the platform seams. Windows compiles for `x86_64-pc-windows-gnu`; macOS is written but not built (needs a real Mac). Never run; no installer or bundled libmpv; playback uses the external mpv window ([PLATFORMS.md](PLATFORMS.md))
- [~] Accessibility: keyboard navigation and visible focus on controls yes; screen-reader support is limited by what Iced exposes today
