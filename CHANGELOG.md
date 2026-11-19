# Changelog

All notable changes are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Update now for pacman, apt and dnf installs: when PackageKit is available the update banner can refresh and update only the `nocturne` package through the desktop's normal authentication prompt, then offer Restart; the command stays visible if it is refused or fails.
- Themes can rebuild the familiar server web home layout: new parts for the unplayed-count badge, progress bar, episode line, caption alignment, row arrows, header server name, link icons and Watch together button, and library tiles, plus a built-in "Classic" example theme.
- Themes follow `@import` and load web fonts: stylesheets that import other stylesheets (Jellyfin web themes, font sheets) now apply, and `@font-face` fonts in TTF, OTF, WOFF and WOFF2 are downloaded and used. https only, no cookies or sign-in, size, time and count limits, kept in the cache folder and refreshed only by hand or when the text changes. Settings > Themes has a switch that turns all downloading off.
- One server's UI over another: each source, "All" and local folders can use NOCTURNE's layout, their own server's UI or another server's (its home sections and look, filled with what you are browsing). Local only; nothing is written to either server. A signed-out or unreachable UI server falls back to the NOCTURNE layout with a note.

- Settings > Sources has a "UI:" button on every source and on a new "All sources" row to choose whose UI is used (NOCTURNE, this server, another server); a fallback is explained on the row. Settings > Themes lists the built-in themes first, has "Refresh imports", shows what was fetched and which fonts loaded, and previews a pasted or imported theme on the whole app until Keep or Revert (a floating bar on every screen).
- Safe look: Ctrl+Shift+0 or starting with `NOCTURNE_SAFE_THEME=1` restores the built-in look at once, whatever theme is saved.
- Data for the Glass look (UI follows): title logo art for server items (own or the show's) and local `logo.png` / `clearlogo.png` sidecars (next to the movie or in the show folder, read only); Settings > Home banner (rotate by itself, interval 5 to 30 s); the banner's slide selection and timer (several sources in order, no duplicates, pauses while hovered, focused, expanded, hidden or playing, none with reduced motion); expandable descriptions; loading states for skeleton placeholders.

### Changed
- Loaded theme fonts apply at once to headings, labels and buttons (text without an explicit font keeps the launch font).
- `sources.toml` is version 2: the per-server Server UI switch is now `ui_from = "self"` (migrated automatically).

## [0.5.1] - 2026-11-19

### Added
- macOS packaging also produces `nocturne-<version>-aarch64-macos.tar.gz` (the `NOCTURNE.app` bundle) with its sha256, the artifact the in-app updater installs; until now macOS only had the `.dmg` and showed Download.

### Fixed
- Themes: importing a CSS file or address while a saved theme is selected no longer overwrites that theme; it fills the editor as a new theme named after the file, and Save adds it.
- `scripts/headless-input-check.sh` is steadier: the paste check reads the field through the clipboard instead of a fixed screen crop, and chords that go missing are retried.

## [0.5.0] - 2026-11-19

### Added
- Updates: NOCTURNE checks once a day (Settings > Updates turns it off). Per-user installs offer Download and install, then Restart to update; every download is checked against the announced size, sha256 and an ed25519 signature before anything is replaced, and a failed update leaves the current version untouched. Package-managed installs show the pacman, apt or dnf command instead.
- Themes can restyle layout with CSS on named parts, and the app follows it: cards (aspect, width, caption position, radius, shadow, typography, hover zoom), Home rows (carousel or grid, gap, padding, section order and hiding), hero (billboard, banner or none, height, overlay), header (height, hidden items, static or overlaid, link typography), button hover scale and the player bar background. Two built-in examples, Marquee and Feed, can be copied from Settings > Themes. `--nocturne-density`, `font-size` (whole-interface zoom) and `backdrop-blur` (a soft placeholder on the detail page) are applied approximately; `letter-spacing` is read but not drawn.
- Themes: Settings > Themes opens a page to import a CSS theme (paste, a `.css` file, or one https address you type, fetched once and refreshed only when you ask) and apply it to the whole app, optionally with a different theme per server. NOCTURNE reads only what it can map to its own look (colours, corner radii, window background, a font family, backdrop opacity) and lists everything else with its line and column; nothing is executed or downloaded from inside the stylesheet. A preview lasts 15 seconds unless you keep it ([CUSTOMIZATION_PLAN.md](docs/dev/CUSTOMIZATION_PLAN.md)).
- Trakt connects with one click: NOCTURNE ships a public client id (PKCE, no secret), so Settings > Trakt shows just "Connect Trakt". "Advanced: use your own Client ID" (or `NOCTURNE_TRAKT_CLIENT_ID`) still overrides it, and a build without a built-in id asks for one as before ([TRAKT.md](docs/TRAKT.md)).
- Pushing a `vX.Y.Z` tag builds every format on GitHub-hosted runners (tarball, .deb, .rpm, Windows installer and zip, macOS .dmg, best-effort AppImage) and creates a draft release with `SHA256SUMS` and the CHANGELOG section as notes; the repositories are still signed and published locally (`.github/workflows/release.yml`, [PACKAGING.md](docs/PACKAGING.md)).
- apt and dnf repositories next to the pacman one (`https://nocturne.ifsvivek.in/repo/deb` and `/repo/rpm`, signed with the same key): Debian/Ubuntu and Fedora users get NOCTURNE updates through `apt upgrade` / `dnf upgrade` and their software centres. Built by `scripts/publish-repo-deb-rpm.sh` (called by `scripts/publish-repo.sh`); the key is also published as `repo/NOCTURNE-GPG-KEY` (armored) and `repo/NOCTURNE-GPG-KEY.gpg` (binary).
- Server UI (per server, Settings > Sources > Layout, off for new servers): with one server selected in the source filter, Home follows that server's own layout: its home sections in its order, a My media row of its libraries in your order without the ones you hid on the server, and Latest rows without the excluded libraries. The server's branding CSS becomes that server's theme (below a theme you pick for it, above the global one; listed in Settings > Themes with what could not be mapped). The sign-in page shows the server's login disclaimer. Live TV rows and the audio and book continue rows are skipped. The unified Home (filter All) is unchanged ([SERVER_UI_PLAN.md](docs/dev/SERVER_UI_PLAN.md)).
- Settings > Sources > Sections edits the server's home sections (reorder, on / off) for your account. It is a server change: only with "allow server changes" on, never with a developer token, and it replaces only the home section entries of your display preferences on that server, leaving every other setting as it was ([WRITES.md](docs/WRITES.md)).

### Fixed
- Local library: after a scan was interrupted before saving its index, a newly found file could take the id of an existing item; ids are now recovered from the indexed items too.
- The connect and sign-in forms put the cursor in the first field when they open, so you can type straight away and Tab cycles the fields and buttons.
- Every text field can now be edited like a normal one: select with the mouse or Shift+arrows, double-click a word, Ctrl+A, Ctrl+C / Ctrl+X / Ctrl+V. The sign-in, add-server, account dialog, admin dialog and SyncPlay name fields used a hand-rolled input that could only append and delete characters. Password fields cannot be copied or cut.

### Changed
- Packaging: the deb, rpm, Arch package and `install.sh` no longer depend on X11 libraries (`libX11`, `libxcb`, `libXcursor`, `libXi`, `libxkbcommon-x11`) now that Linux is Wayland-only.
- **Linux is Wayland-only.** X11 support is removed: Iced is built without its X11 backend (no X11 libraries are loaded) and on a session with only `DISPLAY` set NOCTURNE prints that X11 is not supported and exits. The external mpv player remains the fallback when in-window video cannot start and for the HDR pop-out.

## [0.4.0] - 2026-11-18

What is verified and what is not is listed per feature in [FEATURES.md](docs/FEATURES.md).

### Added
**Playback**
- Secondary subtitle (Subtitles menu, "Secondary" section; Shift+C cycles it; top by default).
- Audio and subtitle language remembered per show (and per movie) across restarts, in a local file under `$XDG_DATA_HOME/nocturne/`, never on a server; "Original audio" is a choice in the Audio menu.
- HDR video: for an HDR stream the player asks whether to play in the window in SDR or open an HDR pop-out window (mpv's own window, driven from the app). New setting "HDR video" (Ask / Play in the window in SDR / Open in HDR pop-out window) ([HDR.md](docs/HDR.md)). Not yet seen on an HDR display.
- Player menus by keyboard: T Subtitles, V Audio, O (or Menu) Settings, Q Quality; Shift+Left / Right seeks 30 s.
- Local files play the subtitles you downloaded; the Subtitles menu offers "Find subtitles online" (when the source's subtitle download is on) and a download is added to the playing video at once.

**Library and sources**
- Home "Latest" rows use the server's `Items/Latest`, so new episodes of shows already in the library surface (grouped under the show); Play on a show card starts its next unwatched episode (`Shows/NextUp`).
- "Included in" strip on movie and show pages (collections the item belongs to, Jellyfin 12+).
- Optional extra HTTP header per server (Settings > Sources > Header) for servers behind a gateway. The value is kept in the system keyring, never in a file.
- Smart collections (header: Smart): saved rules over every source, e.g. unwatched 4K movies, added this week, rated 8+ dramas from the 90s. Five starters; create, edit, delete and reorder your own with a live match count. Stored only in `smart.json` on this computer. Resolution rules need a server (local folders sit them out).
- Optional online details for local media (Local folders > Online details, off by default, per source): TMDB (your own free key, kept in the keyring), TVmaze and MusicBrainz with the Cover Art Archive. Only title, year, ids and season / episode numbers (album and artist for music) are sent, never file names or paths. Unclear matches are listed under "Choose the right match". Results sit below your own edits, `.nfo` files and sidecar art, and TMDB data is refreshed or dropped within six months. Provider credits and the TMDB notice are shown ([METADATA.md](docs/METADATA.md)).
- Optional subtitle download for local files: "Find subtitles online" searches OpenSubtitles with your own API key (and, for downloads, your OpenSubtitles login; both in the keyring) and keeps the file in NOCTURNE's own folder, never next to media.
- Offline copies: "Download to this computer" in the ... menu of a server movie, episode, season or show saves the original file(s) into a local library folder (or NOCTURNE's downloads folder), named the Jellyfin way with a small `.nfo` so the copy shows as a second source of the same title. GET only, resumable, never overwrites, one transfer at a time, queue survives restarts (account menu > Downloads), external subtitles come along.

**Across services**
- One watched state across sources (Settings > Watched state, opt-in): marking a title watched also marks its copies on your other sources, matched by IMDb / TMDB / TVDB id (episodes: series ids plus season and episode), never by title alone. Server copies change only when server changes are on, each on its own server; local copies always follow. Changes that arrive from elsewhere (Trakt import) never spread further ([WRITES.md](docs/WRITES.md)).
- Trakt (Settings > Trakt, opt-in): connect your own Trakt API app with a browser sign-in (PKCE, loopback redirect; no credentials shipped, no client secret; client id and sign-in live in the keyring), scrobble movies and episodes that have provider ids, and optionally import watched history into local items (server items only with server changes on) ([TRAKT.md](docs/TRAKT.md)).
- Remote control from other Jellyfin apps (opt-in, off by default, needs server changes on): this computer appears as a controllable device; play, pause, seek, next / previous, stop, volume, mute, audio / subtitle track and on-screen messages sent from another app are carried out ([WRITES.md](docs/WRITES.md)).

**Couch mode and platforms**
- Couch mode (Settings > Living room: Off / On / Automatic): a 10-foot layout with a larger UI, thicker focus ring, a hint bar, and gamepad support (`gilrs`, a background thread only while the mode is not Off): D-pad / left stick move, A select, B back, Y search, X favourite, Start account menu, LB / RB page, LT / RT seek 30 s. Automatic turns on while a gamepad is connected or on a 1900 px+ display. Remote / media keys share the keyboard path.
- macOS and Windows code behind the platform seams (preview: built by a one-shot CI workflow, never run on a real PC or Mac): per-OS paths, keyring backends, named-pipe mpv IPC, per-OS libmpv loading, single-instance guard, playback through the external mpv window. `scripts/check-cross.sh` runs the compile checks in Docker ([PLATFORMS.md](docs/PLATFORMS.md)).
- `.deb` and `.rpm` packages, built inside Debian / Ubuntu and Fedora containers and install-tested in clean containers (the Fedora one needs RPM Fusion for `mpv-libs`). No AppImage: it would be 107 MB and clash over libmpv / glibc ([PACKAGING.md](docs/PACKAGING.md)).
- Header links that scroll sideways show a fade and chevron at the hidden end and follow the focused link.

### Changed
- Playback reports carry the play method, media source and play session id, so the server can tell direct play from a transcode.
- Pasted API keys (TMDB, OpenSubtitles, Trakt client id) and the extra-header value lose surrounding whitespace and one pair of quotes.
- libmpv older than 0.40 is tolerated (background options are skipped).
- With no graphical session the app prints a clear message instead of panicking.

### Fixed
- Online details can no longer attach to the wrong local file after an interrupted scan (item ids are never reused, each online entry is tied to the file it was fetched for, and purged items lose their entries and artwork).
- The online subtitle finder is usable with arrows, Enter and a gamepad, and a download without an OpenSubtitles login explains that a login is needed.
- A failed playback report shows a quiet notice (once per playback) instead of being swallowed; the final "stopped" report is retried once.
- 0.3.1 fix batch: the divider rule no longer runs through the genre chips on detail pages, and the chip band grows when chips wrap.
- Repository setup instructions work in fish (a one-line `printf` instead of a heredoc), with troubleshooting notes.

## [0.3.0] - 2026-11-16

A new foundation: the interface is rebuilt on Iced, and NOCTURNE becomes a media center for local folders and several Jellyfin servers in one library.

### Added
- **Local library.** Add folders in Settings > Sources (portal chooser or typed path). NOCTURNE scans them (video, audio and image files; names such as `Title (Year)` and `Show/Season 01/... S01E02 ...`; `.nfo` files and sidecar artwork; stream details from `ffprobe` when installed), with live progress, cancel, incremental rescans and availability for unplugged drives. Files that cannot be matched are listed under Unmatched and can be matched by hand. Resume position and watched state for local items are kept in NOCTURNE's own data directory. Local files are never modified, moved or deleted.
- **Several servers, one library.** Any number of Jellyfin servers, each with its own account, plus local folders, behind one catalog. Home, browse and search merge them; a header filter narrows to one source and small source marks show where an item lives. A server that is signed out or offline shows a quiet banner and retries with backoff without affecting the others. Settings > Sources shows status and offers sign in again, rescan, enable / disable, priority order and remove.
- **Duplicates as versions and sources.** The same title found in several places appears once (matched on provider ids, never on title alone). The detail page has one "Versions and sources" picker listing a server's own versions (for example 4K and 1080p) and copies on other servers or in local folders, with "Always prefer this source".
- **First-run setup.** A short onboarding flow (five steps for a fresh install, two for an upgrader) offers Local library, Connect to a server, or Both; Settings > Run setup again repeats it.
- **Single instance.** Launching NOCTURNE again raises the running window instead of opening a second one.
- **Player loading overlay.** Opening a stream shows a Stremio-style splash (the item's backdrop, its logo filling with the buffer and a status line); a lighter version appears on stalls during playback.
- **Reduce motion** setting (follows the desktop preference by default).
- **Watch together (SyncPlay):** press `g` or use the account menu to list, create, join and leave groups on your server. In a group, choosing something to play starts it for everyone, and play, pause, seek, stop and next-item stay in step across members' players (clock sync against the server, command scheduling, small drift corrected by nudging mpv's speed and large drift by seeking, and the Ready / Buffering handshake so the group waits for the slowest member). A pill in the header and on the player shows the group and has Leave. Works with the embedded and the external mpv player. It needs server changes switched on (leaving never does) and a user whose policy allows SyncPlay. `NOCTURNE_DEVICE_ID` overrides the device id the server keys sessions by.

### Changed
- **The interface is built on Iced 0.14; GPUI is removed.** Same design, layouts and keys. Video still plays inside the window on Wayland (libmpv's OpenGL render API in a subsurface below the transparent window) and in an external mpv window on X11. The pinned `gpui` and the patched `vendor/xattr` crate are gone.
- **libmpv is loaded at run time** instead of linked, so a missing `libmpv.so.2` is an error the app can handle: browsing keeps working, playback falls back to the `mpv` program, and otherwise the app shows a per-distribution install hint. The tarball `install.sh` now checks the runtime libraries and installs missing ones through the distribution's package manager (`--no-deps` skips it). The Arch package declares `mpv` explicitly.
- **Release builds no longer read `.env`.** Only debug builds look for one, so a stray file in a parent directory cannot point an installed app at a server. `NOCTURNE_JF_URL` / `NOCTURNE_JF_TOKEN` from the environment still add a transient read-only source.
- Server changes go only to the server an item came from; the single `allow_server_writes` switch still gates all of them.
- Upgrading from 0.2: your server login becomes a source automatically (`sources.toml`); settings are kept; the old `servers.toml` and `accounts.toml` are left in place. Nothing is changed on any server.
- Documentation rewritten for the new architecture (`docs/ARCHITECTURE.md`, `docs/SOURCES.md`, `docs/DECISIONS.md`, `docs/PLATFORMS.md`, `docs/HDR.md`, `docs/METADATA.md`). The "Linux only" statements became "Linux first; macOS and Windows planned, not available".

### Fixed
- Clicking Home with the mouse could leave the header highlighting a library (for example Movies) instead of Home, because the Home page kept the keyboard focus on a header entry.
- Wide-tracked uppercase labels dropped or clipped glyphs; letter-spaced text now renders every character.
- Buttons in dialogs and the Sources screens are normal-sized again after the port.

## [0.2.0] - 2026-11-15

### Added
- Profile picker ("Who's watching?"): when the last used server has two or more remembered accounts and no default, the app opens a full-screen picker of large round profile tiles (profile picture, or the initial, with the name below) and an "Add profile" tile that opens the usual sign-in form. Pick with the mouse, Left / Right (Up / Down in wrapped rows) and Enter; Esc does nothing. Choosing a profile signs in with its stored token, no password, and shows Home; if the server rejects the token the sign-in form opens with the name already filled in. A "Use as default profile" box (Space or click; off by default) makes the chosen profile open at launch without the picker. The account menu has "Switch profile" (always opens the picker, never clears the default by itself), Settings has a "Default profile" drop-down ("Ask every time" or a profile), and removing or signing out the default account clears it. One remembered account behaves as before, and developer-token mode never shows the picker. Switching profile is a local sign-in with an existing token: nothing is written to the server and the server-changes switch is not involved. `accounts.toml` gains an optional `default = true` marker on one entry per server (no token; older files load unchanged).

### Changed
- Contrast and Settings controls: tertiary text rose from 3.5:1 to at least 4.6:1 and secondary text is lighter, so every text token now reaches WCAG AA 4.5:1 on every surface (a unit test checks all token and surface pairs). Interactive controls (buttons, inputs, chips, avatar buttons, profile tiles) got a new `edge` outline of at least 3:1. Settings value controls are now 32 px filled drop-downs with 13 px text, a caret and clear hover, focus, pressed and open states; On / Off settings are switches that keep the ON / OFF word; the selected row has an accent bar; section captions, descriptions and row spacing are larger and easier to read.
- Settings: options with several choices (audio and subtitle language, subtitles, buffer ahead, refill below, max RAM buffer, grid density, image cache) are now drop-downs. Click the value (or press Enter / Space) to open a menu under it with the current choice marked; pick one to apply, Esc or a click elsewhere closes it unchanged, and Up / Down / Home / End / Enter work from the keyboard. On / Off settings still flip with one click. The language lists now offer Automatic plus 28 common languages instead of nine; saved values keep their format and an unknown saved value is shown as it is.

### Fixed
- The round avatar button in the header showed only the letter initial even when the account had a profile image. It now shows the picture (clipped to a circle), also larger in the account menu next to the name, on the "Switch to ..." rows and in the Admin Users list. The initial stays while the picture loads, if it fails, and when none is set. The picture is read with GET `/Users/{id}/Images/Primary`, cached on disk by server, user and image tag, and follows a change at once: setting or removing it from the app, or switching account, needs no restart. `accounts.toml` now also keeps the server's public image tag per account (no token, no image data).
- The tarball installer (`install.sh`) now writes the absolute binary path into the desktop entry, refreshes the desktop and icon caches, and says what to do when the app does not appear in the launcher. In 0.1.0 some launchers did not list NOCTURNE after a tarball install.
- Settings: the page could not be scrolled, so the settings below the fold were out of reach. It now scrolls with the wheel, trackpad, PgUp / PgDn, Home / End and the arrow keys, which keep the selected row in view.

## [0.1.0] - 2026-11-15

First release. Linux only.

### Added
- Native GPUI desktop client for Jellyfin with a near-black, hairline, single-accent design.
- Browsing: home (Continue Watching, Next Up, latest), paged libraries with sort, filters, A-Z and three views, genres, studios, years, tags, collections, playlists, favourites, search, people pages, detail pages for movies, series, seasons (season cards) and episodes, a photo viewer and basic book pages.
- In-window GPU video through libmpv on Wayland, with an external mpv fallback on other sessions; chapters, trickplay, audio and subtitle menus, delays, speed, picture controls, skip intro and credits, next-episode autoplay, queue, quality selector with transcode fallback, mini player, MPRIS, idle inhibit, buffering watermarks.
- Accounts: password sign-in, Quick Connect (including authorizing another device), keyring-backed session restore, switching between remembered accounts without a password, own profile image and display language, own devices, session-ended and offline handling, and a global `?` shortcut reference.
- Playlists you own (create, add, remove, reorder, rename, delete) and a music queue (play next, add, reorder, remove, shuffle, repeat).
- Admin section for administrator accounts: dashboard, users (access and limits, profile image), libraries (add, remove, rename, folders, options, providers, refresh schedule), scheduled tasks and triggers, devices, activity, logs, API keys, branding, networking (safe subset), transcoding, trickplay, backups, plugins and plugin repositories. Every change is behind a confirmation dialog.
- Read-only by default: one master switch (`allow_server_writes`) gates every server change; the browsing crate is GET-only. See [docs/WRITES.md](docs/WRITES.md).
- Packaging: release tarball with installer, Arch PKGBUILD, desktop entry, icons, AppStream metadata.

### Known limitations
See [docs/ROADMAP.md](docs/ROADMAP.md).

[0.3.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.3.0
[0.2.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.2.0
[0.1.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.1.0
