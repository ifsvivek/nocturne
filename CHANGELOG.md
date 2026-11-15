# Changelog

All notable changes are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Admin: editable Branding (login message, custom CSS, splash screen), Networking (ports, bind addresses, LAN
  networks, remote access, UPnP, discovery, IPv4/IPv6; each change needs the typed word NETWORK and warns that
  wrong values can lock you out), scheduled-task triggers (add, edit, delete), a Trickplay settings page, library
  add / remove / folders / common options, and plugin enable, disable, uninstall and install from the catalog.
- Account menu: Authorize a device (approve another device's Quick Connect code), Your devices (read only list
  of this account's sessions), and a global `?` keyboard shortcut reference.
- Dialog inputs are validated per field (time of day, ports, networks, paths) and marked when invalid.
- `scripts/test-jellyfin.sh` and the test guard accept `NOCTURNE_JF_TEST_PORT` to run a second throwaway server.

- Admin: plugin repositories (add, enable, disable, remove), library rename, a Providers page (metadata and image
  fetcher order and on / off per library and item type, local readers, what the server offers), the library scan and
  metadata refresh schedule, and an Access and limits page per user (library access, feature switches, parental
  rating, tags, bitrate and session limits, lockout, profile image, display preferences read).

- Accounts: switch between remembered accounts of a server without a password, add another account, own profile
  image and display language.
- Playlists you own: create, add from any item or album, remove, reorder, rename, delete. Music queue with play
  next, add to queue, reorder, remove, shuffle and repeat, kept for the session.

### Fixed
- Top nav no longer lists Collections / Playlists twice; images first seen without tags (search hits) are fetched
  again once the full item arrives and failed images retry after 30 s; the series hero logo fits its box; the
  breadcrumb bar covers content scrolled under it.
- Admin sidebar: the "Playback" sub-heading is gone; the group lists consistently.
- Typing `/` or `?` inside an admin dialog opened search or help instead of entering the character.

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
