# Changelog

All notable changes are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Home "Latest" rows use the server's `Items/Latest`, so new episodes of shows already in the library surface (grouped under
  the show); Play on a show card starts its next unwatched episode (`Shows/NextUp`).
- "Included in" strip on movie and show pages (collections the item belongs to, Jellyfin 12+).
- Optional extra HTTP header per server (Settings > Sources > Header) for servers behind a gateway. The value is kept in the
  system keyring, never in a file, and is sent with every request to that server.

### Fixed
- Playback reports now carry the play method, media source and play session id, and a failed report shows a quiet notice
  (once per playback) instead of being swallowed; the final "stopped" report is retried once.

## [0.3.0] - 2026-11-16

A new foundation: the interface is rebuilt on Iced, and NOCTURNE becomes a media center for local folders and several
Jellyfin servers in one library.

### Added
- **Local library.** Add folders in Settings > Sources (portal chooser or typed path). NOCTURNE scans them (video, audio
  and image files; names such as `Title (Year)` and `Show/Season 01/... S01E02 ...`; `.nfo` files and sidecar artwork;
  stream details from `ffprobe` when installed), with live progress, cancel, incremental rescans and availability for
  unplugged drives. Files that cannot be matched are listed under Unmatched and can be matched by hand. Resume position and
  watched state for local items are kept in NOCTURNE's own data directory. Local files are never modified, moved or deleted.
- **Several servers, one library.** Any number of Jellyfin servers, each with its own account, plus local folders, behind
  one catalog. Home, browse and search merge them; a header filter narrows to one source and small source marks show where an
  item lives. A server that is signed out or offline shows a quiet banner and retries with backoff without affecting the
  others. Settings > Sources shows status and offers sign in again, rescan, enable / disable, priority order and remove.
- **Duplicates as versions and sources.** The same title found in several places appears once (matched on provider ids, never on
  title alone). The detail page has one "Versions and sources" picker listing a server's own versions (for example 4K and
  1080p) and copies on other servers or in local folders, with "Always prefer this source".
- **First-run setup.** A short onboarding flow (five steps for a fresh install, two for an upgrader) offers Local library, Connect to a server, or Both; Settings > Run setup again repeats it.
- **Single instance.** Launching NOCTURNE again raises the running window instead of opening a second one.
- **Player loading overlay.** Opening a stream shows a Stremio-style splash (the item's backdrop, its logo filling with the buffer
  and a status line); a lighter version appears on stalls during playback.
- **Reduce motion** setting (follows the desktop preference by default).
- **Watch together (SyncPlay):** press `g` or use the account menu to list, create, join and leave groups on your server. In a
  group, choosing something to play starts it for everyone, and play, pause, seek, stop and next-item stay in step across
  members' players (clock sync against the server, command scheduling, small drift corrected by nudging mpv's speed and large
  drift by seeking, and the Ready / Buffering handshake so the group waits for the slowest member). A pill in the header and
  on the player shows the group and has Leave. Works with the embedded and the external mpv player. It needs server changes
  switched on (leaving never does) and a user whose policy allows SyncPlay. `NOCTURNE_DEVICE_ID` overrides the device id
  the server keys sessions by.

### Changed
- **The interface is built on Iced 0.14; GPUI is removed.** Same design, layouts and keys. Video still plays inside the
  window on Wayland (libmpv's OpenGL render API in a subsurface below the transparent window) and in an external mpv window on
  X11. The pinned `gpui` and the patched `vendor/xattr` crate are gone.
- **libmpv is loaded at run time** instead of linked, so a missing `libmpv.so.2` is an error the app can handle: browsing keeps
  working, playback falls back to the `mpv` program, and otherwise the app shows a per-distribution install hint. The tarball
  `install.sh` now checks the runtime libraries and installs missing ones through the distribution's package manager
  (`--no-deps` skips it). The Arch package declares `mpv` explicitly.
- **Release builds no longer read `.env`.** Only debug builds look for one, so a stray file in a parent directory cannot point an
  installed app at a server. `NOCTURNE_JF_URL` / `NOCTURNE_JF_TOKEN` from the environment still add a transient read-only source.
- Server changes go only to the server an item came from; the single `allow_server_writes` switch still gates all of them.
- Upgrading from 0.2: your server login becomes a source automatically (`sources.toml`); settings are kept; the old
  `servers.toml` and `accounts.toml` are left in place. Nothing is changed on any server.
- Documentation rewritten for the new architecture (`docs/ARCHITECTURE.md`, `docs/SOURCES.md`, `docs/DECISIONS.md`,
  `docs/PLATFORMS.md`, `docs/HDR.md`, `docs/METADATA.md`). The "Linux only" statements became "Linux first; macOS and Windows
  planned, not available".

### Fixed
- Clicking Home with the mouse could leave the header highlighting a library (for example Movies) instead of Home, because
  the Home page kept the keyboard focus on a header entry.
- Wide-tracked uppercase labels dropped or clipped glyphs; letter-spaced text now renders every character.
- Buttons in dialogs and the Sources screens are normal-sized again after the port.

## [0.2.0] - 2026-11-15

### Added
- Profile picker ("Who's watching?"): when the last used server has two or more remembered accounts and no default, the
  app opens a full-screen picker of large round profile tiles (profile picture, or the initial, with the name below) and
  an "Add profile" tile that opens the usual sign-in form. Pick with the mouse, Left / Right (Up / Down in wrapped rows)
  and Enter; Esc does nothing. Choosing a profile signs in with its stored token, no password, and shows Home; if the
  server rejects the token the sign-in form opens with the name already filled in. A "Use as default profile" box (Space
  or click; off by default) makes the chosen profile open at launch without the picker. The account menu has
  "Switch profile" (always opens the picker, never clears the default by itself), Settings has a "Default profile"
  drop-down ("Ask every time" or a profile), and removing or signing out the default account clears it. One remembered
  account behaves as before, and developer-token mode never shows the picker. Switching profile is a local sign-in with
  an existing token: nothing is written to the server and the server-changes switch is not involved. `accounts.toml`
  gains an optional `default = true` marker on one entry per server (no token; older files load unchanged).

### Changed
- Contrast and Settings controls: tertiary text rose from 3.5:1 to at least 4.6:1 and secondary text is lighter, so every
  text token now reaches WCAG AA 4.5:1 on every surface (a unit test checks all token and surface pairs). Interactive
  controls (buttons, inputs, chips, avatar buttons, profile tiles) got a new `edge` outline of at least 3:1. Settings
  value controls are now 32 px filled drop-downs with 13 px text, a caret and clear hover, focus, pressed and open states;
  On / Off settings are switches that keep the ON / OFF word; the selected row has an accent bar; section captions,
  descriptions and row spacing are larger and easier to read.
- Settings: options with several choices (audio and subtitle language, subtitles, buffer ahead, refill below, max RAM
  buffer, grid density, image cache) are now drop-downs. Click the value (or press Enter / Space) to open a menu under it
  with the current choice marked; pick one to apply, Esc or a click elsewhere closes it unchanged, and Up / Down /
  Home / End / Enter work from the keyboard. On / Off settings still flip with one click. The language lists now offer
  Automatic plus 28 common languages instead of nine; saved values keep their format and an unknown saved value is
  shown as it is.

### Fixed
- The round avatar button in the header showed only the letter initial even when the account had a profile image. It now
  shows the picture (clipped to a circle), also larger in the account menu next to the name, on the "Switch to ..." rows
  and in the Admin Users list. The initial stays while the picture loads, if it fails, and when none is set. The picture
  is read with GET `/Users/{id}/Images/Primary`, cached on disk by server, user and image tag, and follows a change at
  once: setting or removing it from the app, or switching account, needs no restart. `accounts.toml` now also keeps the
  server's public image tag per account (no token, no image data).
- The tarball installer (`install.sh`) now writes the absolute binary path into the desktop entry, refreshes the desktop
  and icon caches, and says what to do when the app does not appear in the launcher. In 0.1.0 some launchers did not
  list NOCTURNE after a tarball install.
- Settings: the page could not be scrolled, so the settings below the fold were out of reach. It now scrolls with the
  wheel, trackpad, PgUp / PgDn, Home / End and the arrow keys, which keep the selected row in view.

## [0.1.0] - 2026-11-15

First release. Linux only.

### Added
- Native GPUI desktop client for Jellyfin with a near-black, hairline, single-accent design.
- Browsing: home (Continue Watching, Next Up, latest), paged libraries with sort, filters, A-Z and three views,
  genres, studios, years, tags, collections, playlists, favourites, search, people pages, detail pages for movies,
  series, seasons (season cards) and episodes, a photo viewer and basic book pages.
- In-window GPU video through libmpv on Wayland, with an external mpv fallback on other sessions; chapters,
  trickplay, audio and subtitle menus, delays, speed, picture controls, skip intro and credits, next-episode
  autoplay, queue, quality selector with transcode fallback, mini player, MPRIS, idle inhibit, buffering watermarks.
- Accounts: password sign-in, Quick Connect (including authorizing another device), keyring-backed session restore,
  switching between remembered accounts without a password, own profile image and display language, own devices,
  session-ended and offline handling, and a global `?` shortcut reference.
- Playlists you own (create, add, remove, reorder, rename, delete) and a music queue (play next, add, reorder,
  remove, shuffle, repeat).
- Admin section for administrator accounts: dashboard, users (access and limits, profile image), libraries (add,
  remove, rename, folders, options, providers, refresh schedule), scheduled tasks and triggers, devices, activity,
  logs, API keys, branding, networking (safe subset), transcoding, trickplay, backups, plugins and plugin
  repositories. Every change is behind a confirmation dialog.
- Read-only by default: one master switch (`allow_server_writes`) gates every server change; the browsing crate is
  GET-only. See [docs/WRITES.md](docs/WRITES.md).
- Packaging: release tarball with installer, Arch PKGBUILD, desktop entry, icons, AppStream metadata.

### Known limitations
See [docs/ROADMAP.md](docs/ROADMAP.md).

[0.3.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.3.0
[0.2.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.2.0
[0.1.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.1.0
