# Features

What NOCTURNE does today. Items marked (partial) work with the limits stated; items under Not implemented do not exist yet. How each feature was checked is in [QA_REPORT.md](QA_REPORT.md), planned work is in [ROADMAP.md](ROADMAP.md), and which features change data on a server is in [WRITES.md](WRITES.md).

## Sources

- First-run setup: local library, server, or both. It can be repeated from Settings > Run setup again.
- One window: starting NOCTURNE again raises the running one.
- Several Jellyfin servers at once, each with its own account. A source that is signed out or offline never blocks the others; a quiet banner shows it and the app retries with backoff.
- Settings > Sources: status, sign in again, rescan, enable or disable, priority order, remove (it says what is deleted).
- One catalog: Home, browsing and search merge all sources. A header filter narrows to one source, and small marks show where an item lives.
- Duplicates appear once when provider ids (IMDb, TMDB, TVDB) agree, or when series id, season and episode agree. Title alone never merges.
- A versions and sources picker on the detail page lists a server's own versions and the copies in other sources, with "Always prefer this source".
- Server UI per server (Settings > Sources > UI): Home in that server's own section order with its My media row, library order and hidden libraries, and its branding CSS as that server's theme layer. A home sections editor can reorder and switch sections; it writes back only the home section entries and needs server changes on. Live TV and recordings rows, audio and book continue rows and plugin pages of the web client are not shown.
- An optional extra HTTP header per server, stored in the keyring.

## Local library

- Add folders with the portal chooser or a typed path. Scans show progress and can be cancelled; unplugged drives show as unavailable.
- Metadata comes from file names, `.nfo` files, sidecar artwork, and `ffprobe` when it is installed. Unmatched files are listed and can be matched by hand.
- Online details are opt-in per source: TMDB (your own key), TVmaze, and MusicBrainz with Cover Art Archive. Unclear matches are listed, never guessed ([METADATA.md](METADATA.md)).
- Subtitle download from OpenSubtitles (your own key, opt-in), stored in NOCTURNE's folder.
- Offline copies: download a server movie, episode, season or show to this computer. Downloads are resumable and appear as a second local source.
- Smart collections: saved rules over every source (watched state, genres, years, rating, added within N days, resolution, runtime, sources). Resolution rules apply to servers only.
- Resume position and watched state for local files are kept in NOCTURNE's own data. Your files are never modified, moved or deleted.
- Optional shared watched state across sources, matched by provider ids only and write-gated.

## Browsing

- Home with a hero carousel, Continue Watching, Next Up and latest rows per library. Latest rows include new episodes of existing shows, and Play on a show card starts the next unwatched episode.
- Library views with a paged grid, sort, filters, A-Z jump and poster, thumb or list layout.
- Genres, Studios, Years and Tags tabs; Collections, Playlists and Favourites pages.
- Search with Ctrl+K or `/`: grouped results and recent searches.
- Calendar (header link, or `c`): month, week and agenda views of upcoming episodes from every signed-in server and, when Trakt is connected, your Trakt calendar. Each entry has a poster, season and episode, title and the air time in your time zone; it is marked on your server (playable now), airing soon (a server has the show) or not on your servers (Trakt only), and Enter or a click plays it or opens the show. Weeks start on Monday. Servers only know the date for most episodes (shown without a time); a server lists upcoming episodes from yesterday on, so earlier days of the current month are empty. An optional Home row "Coming this week" (Settings, off by default) lists the next seven days from your servers. A server's own layout (Jellyfin layout mode) shows a plain list version of the page. Read only; a request button for shows you do not have comes with Seerr.
- Detail pages for movies, series (logo hero, season tabs, episode grid), episodes and people, with cast, similar items, extras, media info, ratings, chapters and "Included in" collections.
- Images use blurhash placeholders and a size-capped disk cache.
- Breadcrumbs, back and forward, Esc and Backspace.
- Photos (partial): viewer with previous, next and zoom; folders open as grids.
- Music (partial): browsable, with a queue (Play next, Add to queue, reorder, remove, shuffle, repeat). There is no album-art queue view.

## Playback

- Video plays inside the app window through libmpv with hardware decoding on Wayland. An external mpv window is the fallback. Settings > Video player > Player can send video to VLC or to mpv's own window instead (each only when installed); VLC is controlled over a loopback interface with a random password, keeps resume, progress and Trakt, and has no subtitle or audio menus of its own in the app.
- libmpv is loaded at run time. Without it the app still starts, falls back to the `mpv` program, or says what to install.
- A loading overlay fills as the stream buffers.
- HDR video is tone-mapped to SDR in the window. An HDR pop-out window (mpv `gpu-next`, fullscreen) has an Ask, SDR or Pop-out setting. It needs HDR hardware and has not been tested on an HDR display ([HDR.md](HDR.md)).
- Seek bar with chapters and trickplay thumbnails, volume, fullscreen and a mini player (a 640x360 window with only the picture; a tiling compositor keeps its own window size, so there only the controls hide).
- Audio and subtitle menus with delay and styling, a secondary subtitle, speed, aspect and picture controls. Audio and subtitle language are remembered per show. Menus open from the keyboard (T, V, O, Q).
- Skip intro and credits, next-episode autoplay, a queue panel, a quality selector with transcode fallback, a playback info overlay, a key help overlay, a context menu and screenshots.
- MPRIS media controls and idle inhibit on Linux.
- Resume position, and progress, watched and favourite sync (needs server changes on).
- Playback reports state the real play method and media source, show a notice on failure, and retry the final "stopped" once.
- Network buffering watermarks ([buffering.md](buffering.md)).
- Seerr requests (opt-in): sign in to your Seerr server, request movies and shows that are not in your library from Search (seasons for shows), follow and cancel them on the Requests page; see [WRITES.md](WRITES.md).
- Trakt (opt-in): one-click connect, scrobbling and watched-history import ([TRAKT.md](TRAKT.md)).
- Remote control from other Jellyfin apps (partial): opt-in, needs server changes on. Checked against a throwaway server, not from a phone app.
- Couch mode (partial): a 10-foot layout with keyboard, remote-key and gamepad navigation. It has not been tried with a real gamepad or TV.

## Accounts

- Sign in with username and password or Quick Connect; sign out; change your own password.
- Authorize another device with Quick Connect; a read-only list of this account's devices.
- Tokens are stored in the system keyring, one per server account, and restored on launch.
- Switch between remembered accounts of a server without a password. A "Who's watching?" picker appears at launch when a server has two or more, and a default profile can skip it.
- Own profile image, display language, add account, forget account.
- Watch together (SyncPlay): create, join and leave groups; play, pause, seek, stop and next stay in step across members. Press `g`. Needs server changes on ([syncplay.md](syncplay.md)).
- Playlists you own: create, add items or albums, remove, reorder, rename and delete. Collections are read only.
- A developer API-key mode through environment variables adds a transient source that is never saved and refuses all writes.

## Themes and settings

- Themes are CSS, imported by pasting, from a file or from an address. They set colours, radii, window background and fonts, and style layout on named parts: cards, rows, Home sections and hero, header, buttons and the player bar. No browser engine is used and nothing from a theme is executed.
- Built-in NOCTURNE and Glass themes, and Marquee, Feed and Classic examples. Themes apply globally or per server, with a 15 second preview.
- Themes gallery (Settings > Themes: Official, Community, My themes): previews and one-click install of NOCTURNE's own themes and, later, community themes, from an index on the website; each download is checked against its sha256, obeys the remote CSS switch and is saved as a normal theme (Update replaces it). Themes added by hand are shown as not from the gallery. Community themes come in by pull request ([themes/README.md](../themes/README.md)).
- Theme Studio (Themes > Open Studio): pick any element of the live screens, edit it with grouped controls ("this element only" or "all like this"), undo and redo, edit raw CSS in sync, start from a gallery, save as a named theme, and export a `.css` file. It covers Home, the header, Library, Search, the detail page and the player's controls.
- Themes follow `@import` and load web fonts (https only, with size, time and count limits). A Settings switch turns all downloading off.
- Jellyfin layout mode draws Home, the header, the library and the item page with the server's own stylesheets (the web client's CSS, the stylesheets its plugins add to the start page, and its branding CSS), so most server web themes look as they do in the browser. Scripts never run. A server with custom branding uses its own look by default; Settings > Sources can choose NOCTURNE's look or another server's look for any source and for All sources.
- Settings > Themes reports, per theme and per server, what the engine read and skipped, with a Copy button for bug reports.
- Safe look: Ctrl+Shift+0, or starting with `NOCTURNE_SAFE_THEME=1`, restores the built-in look. `NOCTURNE_ENGINE_UI=0` shows the hand-written screens instead of the theme engine.
- Settings cover server changes, progress sync, buffering, grid density, home sections, home banner, language preferences, and reduce motion (follows the desktop).
- Real text editing in every field: selection with mouse and keyboard, copy, cut and paste.

## Admin

Administrator accounts only. The admin screens are view-only while server changes are off, and every write goes through a confirmation dialog.

- Dashboard, activity, devices, sessions, logs, backups (list) and plugins (view).
- Users: create, disable, delete, admin flag, password reset. Access and limits per user: library access, feature switches, parental rating, tags, bitrate and session limits, sign-in lockout, profile image.
- Libraries: add, rename, remove, scan, folders, real-time monitor, trickplay and chapter images, NFO saving, embedded titles, providers (order and on or off per library and item type), refresh interval.
- Scheduled tasks: run, stop, and edit triggers.
- Trickplay, General, Metadata, Resume and Streaming settings. Transcoding shows its settings and edits three fields.
- Branding: login message, custom CSS, splash screen.
- Networking (safe subset): ports, bind addresses, LAN networks, remote access, UPnP, discovery, IPv4 and IPv6, with typed confirmation and a lock-out warning. Base URL, HTTPS and certificates, proxies and IP filters are view only.
- Plugins: enable, disable, uninstall, install from the catalog; takes effect after a restart. Plugin repositories: add, enable, disable, remove.
- API keys: create and delete. Server restart.

## Updates

- At most one check a day (setting `check_updates`, on by default).
- Package-managed installs (pacman, apt, dnf) show the update command. Where PackageKit is running, the banner also offers Update now, which updates only the `nocturne` package through the desktop's own password prompt, then offers Restart. NOCTURNE never runs sudo or sees a password.
- Per-user installs (the tarball in a directory you can write, the Windows installer, the macOS app in a writable folder) download, verify and replace themselves, then ask for a restart. The download is checked against its announced size, sha256 and an ed25519 signature before anything is replaced; if any step fails, the current version stays.

## Platform and accessibility

- Responsive full-window layout, keyboard-first navigation, and loading, empty and error states. `?` shows the shortcuts.
- Settings are TOML under `$XDG_CONFIG_HOME/nocturne/` with no secrets on disk.
- Linux is Wayland-only. On an X11-only session NOCTURNE says so and exits.
- Packages: pacman, apt and dnf repositories, a tarball and an Arch PKGBUILD. Windows and macOS are unsigned previews. See [PLATFORMS.md](PLATFORMS.md).
- Accessibility (partial): keyboard navigation and visible focus work; screen-reader support is limited by what Iced exposes.

## Gateway (0.7, in progress)

- Settings > Gateway starts and stops the optional `nocturne-gateway` helper, shows its status and address (Copy, QR code), the sign-in (a generated password shown once, Quick Connect approval) and the signed-in devices with Revoke. Off by default; with it off nothing runs. Modes: this computer only, local network (plain HTTP, private addresses only) or remote over HTTPS with your own certificate. "Keep running when NOCTURNE is closed" installs a systemd user service (Linux).
- Remote control through the gateway (off by default): apps list the players of every signed-in server and of other gateway clients and send play, pause, seek, next, volume, track and message commands. Commands to a device on a real server also need server changes on. Checked in process and against a throwaway server, not yet from a phone app.

## Not implemented

- Live TV and DVR, DLNA, being a cast target, gapless music: planned for 0.9 ([ROADMAP.md](ROADMAP.md)).
- Playable books and audiobooks (detail page only).
- An HDR filter (servers expose none) and subtitle appearance presets.
