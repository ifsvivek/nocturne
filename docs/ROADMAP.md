# Roadmap

Everything NOCTURNE does not do yet, in the order it is likely to be built, plus what is intentionally out of scope.
What already works is in [FEATURES.md](FEATURES.md); verification status is in [QA_REPORT.md](QA_REPORT.md).
No dates are promised. Priorities can change with user feedback and with what upstream Jellyfin changes.

## 0.2.x (earlier release; GPUI interface)

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

## Shipped in 0.3: a new foundation

- **Iced UI.** The interface moved from GPUI to Iced 0.14 with the same design and features; GPUI is removed. In-window
  video keeps working on Wayland ([DECISIONS.md](DECISIONS.md)). A reduce-motion setting follows the desktop.
- **libmpv no longer required to start.** It is loaded at run time; without it playback falls back to the `mpv`
  binary or explains what to install. The release installer installs runtime packages itself.
- **Local library, several servers, one catalog** ([SOURCES.md](SOURCES.md)): use NOCTURNE with local folders only,
  with several Jellyfin servers and accounts, or both; duplicates across sources shown once with alternate sources;
  a first-run choice between the three modes; a Sources section in Settings; local folders with scan progress and
  manual matching.
- **Loading splash** (Stremio-style) when a stream opens.
- **Version picker**: movies and episodes with several versions, and copies of the same title on other servers or
  in local folders, are offered in one "Versions and sources" choice on the detail page.
- **Footprint baselines** (size, start time, idle memory and CPU) tracked per release ([PLATFORMS.md](PLATFORMS.md)).

Not finished in 0.3 and carried forward: the mpv 0.42 certificate check (below), screen-reader support (limited by Iced).

## Then: 0.4, right after 0.3

Chosen by the maintainer from the research backlog: dual subtitles; audio and subtitle language remembered per show
across restarts (including original audio); a Latest row with new episodes and Play on a show card starting the next
unwatched episode; truthful playback reports with visible errors and a retried final report; an optional extra HTTP
header per server (reverse proxies); "Included in" collections on detail pages; **HDR playback mode** on Linux: for HDR
videos the viewer chooses between SDR in the window and an HDR pop-out window ([HDR.md](HDR.md)); **more packages and platforms**: .deb, .rpm and an AppImage (only if it can be honest about
system libraries), then macOS (.dmg) and Windows (.msi/.exe) with bundled libmpv ([PLATFORMS.md](PLATFORMS.md));
**online metadata for local media**, opt-in, with the user's own TMDB key, plus MusicBrainz / Cover Art Archive and
TVmaze ([METADATA.md](METADATA.md)). The compatibility items below are folded into the same release where they overlap.

## Then: compatibility and correctness

A review of the Jellyfin ecosystem (server, web and other official clients: issues, pull requests, API changes) found no
critical problems in NOCTURNE. It did find a set of small fixes worth doing before new features.

- Check playback with mpv 0.42, which now verifies HTTPS certificates by default (private or self-signed certificates), and
  decide whether a per-server certificate option is needed
- Playback reports that describe the stream truthfully (direct play or transcode, media source, play session), so the
  server can stop a transcode when you stop watching; visible, non-modal errors when a report fails, and one retry of
  the final "stopped" report
- A Latest row on Home that shows new episodes of shows already in your library, and Play on a show card that starts
  the next unwatched episode
- Trickplay hover previews that stay correct on libraries scanned before Jellyfin 12
- Admin Users: stop showing a password status that Jellyfin 12 no longer reports reliably
- Remove an unused helper that builds URLs with the removed `api_key` parameter
- Tests ready for Jellyfin 13.0 changes once they are released (route casing, user lookup rules, new SyncPlay fields)
- A hardware check of HDR HEVC with text subtitles on NVIDIA, after an open upstream mpv report

## Then: playback depth (0.5)

- Two subtitle tracks at once (mpv supports it; Jellyfin Web already has it)
- Audio and subtitle language choices remembered per show across restarts, including "original audio" where the
  server provides it; stored locally, never on the server
- Intro and credits prompts that stay visible for the whole segment and cope with overlapping segments from two
  providers
- Optional per-server trust for a private or self-signed certificate, if the mpv 0.42 check shows it is needed

## Then: library depth (0.6)

- Library filters for audio and subtitle language
- "Included in" collections on item pages
- An optional combined Continue Watching and Next Up row
- Display language sent to the server so library and genre names follow it
- Lyrics for music, and audiobook playback with chapters, together with the music items under "Later"

## Later: missing features

**Media types and playback**
- Books and audiobooks: reading and playback (chapters now come from the server)
- Music: album-art views, a queue that survives closing the app, queue editing while browsing, multi-disc grouping
- Collections: create, add, remove, reorder
- Sharing playlists with other users
- Flatpak and other distribution packages; a verified AppImage only if it can be honest about system libraries
- Offline downloads: undecided, because it conflicts with the read-only default

**Accounts and settings**
- A user picker on the sign-in screen (switching already works from the account menu)
- A local PIN for profiles (it would protect the profile picker only, not the server)
- Optional extra HTTP header per server for use behind a reverse proxy
- Quick Connect authorization on behalf of another user (administrators)
- Theme and accent choice, subtitle appearance presets
- Ending your other sessions (the server only allows it for administrators)

**Admin**
- Per-user access schedules, "block unrated" kinds, per-folder deletion rights
- Subtitle, lyric and media-segment provider ordering
- Networking: base URL, HTTPS and certificates, proxies and IP filters are view-only on purpose; editing them
  safely needs a lock-out guard that can be tested

**Platform**
- Packages are tested on Arch Linux only; .deb, .rpm and AppImage are scheduled for 0.4 ([PLATFORMS.md](PLATFORMS.md))
- Screen-reader support: re-evaluated on Iced once the migration ships (Iced's accessibility support is still limited)
- HDR in the window depends on libmpv gaining a modern render API; until then it is tone-mapped to SDR, with an HDR
  pop-out window in 0.4 ([HDR.md](HDR.md))
- macOS (.dmg) and Windows (.msi/.exe): scheduled for 0.4, after 0.3 ships for Arch; in-window video there needs its
  own investigation ([PLATFORMS.md](PLATFORMS.md))

## Dropped (will not be built)

- Live TV and DVR
- DLNA setup
- Acting as a cast target
- Gapless music playback

## Upstream reports

A review of the Jellyfin ecosystem (server, web and other official clients: issues, pull requests, API changes) found
no critical problems in NOCTURNE; the items above come from it. Anything that belongs upstream is prepared as a draft
and written up by hand by a person before it is sent; nothing is filed automatically.
