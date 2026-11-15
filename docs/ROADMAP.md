# Roadmap

Everything NOCTURNE does not do yet, in the order it is likely to be built, plus what is intentionally out of scope.
What already works is in [FEATURES.md](FEATURES.md); verification status is in [QA_REPORT.md](QA_REPORT.md).
No dates are promised. Priorities can change with user feedback and with what upstream Jellyfin changes.

## 0.2.x (current release)

Shipped: browsing, search, details, in-window GPU playback with a full player, accounts (password, Quick Connect,
switching), playlists and a music queue, an admin dashboard inside the same app, settings, packaging for Arch Linux.
See [FEATURES.md](FEATURES.md) for the full list.

Still being verified on a real server (built and tested on a throwaway server, not yet on a real library):
- Newer player features: resume prompt, A-B loop, audio device list, external subtitles, subtitle presets
- Multi-season shows and the photo viewer on real data
- A 401 in the middle of a session and the offline banner
- The image cache size cap on a very large library, and a multi-day soak

## Done: SyncPlay ("watch together")

Shipped after 0.2.x (see [syncplay.md](syncplay.md)): list, create, join and leave groups, synchronised play, pause, seek,
stop and next, clock sync, drift correction and the Ready / Buffering handshake, for the embedded and external players.
Still to do: show what a group is playing (the API does not say), group repeat and shuffle modes, queue editing from the
group, resume positions, and testing with more than two members and with transcoded members.

## Then: research-driven improvements

After the release, a review of the Jellyfin ecosystem (server, web and other official clients: open and closed issues,
pull requests, API changes) to decide what to build and what to work around. Findings and priorities will be added here.
Anything that belongs upstream will be prepared as a report for review, not filed automatically.

## Later: missing features

**Media types and playback**
- Books and audiobooks: playback and reading (the detail page exists)
- Music: album-art views, a queue that survives closing the app, queue editing while browsing
- Collections: create, add, remove, reorder
- Sharing playlists with other users
- Flatpak and other distribution packages; a verified AppImage only if it can be honest about system libraries

**Accounts and settings**
- A user picker on the sign-in screen (switching already works from the account menu)
- Theme and accent choice
- Subtitle appearance presets
- Ending your other sessions (the server only allows it for administrators)

**Admin**
- Per-user access schedules, "block unrated" kinds, per-folder deletion rights
- Subtitle, lyric and media-segment provider ordering
- Networking: base URL, HTTPS and certificates, proxies and IP filters are view-only on purpose; editing them
  safely needs a lock-out guard that can be tested

**Platform**
- Packages are tested on Arch Linux only; other distributions are unverified
- Better screen-reader support, limited today by GPUI
- Windows and macOS: possible future work, not planned or supported

## Dropped (will not be built)

- Live TV and DVR
- DLNA setup
- Acting as a cast target
- Gapless music playback
