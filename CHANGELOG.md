# Changelog

All notable changes are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-11-15

### Fixed
- `install.sh` now writes the absolute binary path into the desktop entry, refreshes the desktop and icon caches, and
  says what to do when the app does not appear in the launcher. In 0.1.0 some launchers did not list NOCTURNE after
  a tarball install.

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

[0.1.1]: https://github.com/ifsvivek/nocturne/releases/tag/v0.1.1
[0.1.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.1.0
