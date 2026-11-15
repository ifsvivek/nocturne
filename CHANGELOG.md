# Changelog

All notable changes are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0]

First release (not yet published). Linux only.

### Added
- Native GPUI desktop client for Jellyfin with a near-black, hairline, single-accent design.
- Browsing: home (Continue Watching, Next Up, latest), paged libraries with sort, filters, A-Z and three views,
  genres, studios, years, tags, collections, playlists, favourites, search, people pages, detail pages for movies,
  series, seasons and episodes, photo viewer, basic book pages.
- In-window GPU video through libmpv on Wayland, with an external mpv fallback on other sessions; chapters,
  trickplay, audio and subtitle menus, delays, speed, picture controls, skip intro and credits, next-episode
  autoplay, queue, quality selector with transcode fallback, mini player, MPRIS, idle inhibit, buffering watermarks.
- Accounts: password sign-in, Quick Connect, keyring-backed session restore, session-ended and offline handling.
- Admin section for administrator accounts: dashboard, users, libraries, tasks, devices, activity, logs, API keys,
  several editable server settings, all writes behind confirmation dialogs.
- Read-only by default: one master switch (`allow_server_writes`) gates all server changes; the browsing crate is
  GET-only.
- Packaging: release tarball with installer, Arch PKGBUILD, desktop entry, icons, AppStream metadata.

### Known limitations
See [docs/ROADMAP.md](docs/ROADMAP.md).

[Unreleased]: https://github.com/ifsvivek/nocturne/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.1.0
