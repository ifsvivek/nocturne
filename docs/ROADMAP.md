# Roadmap

Everything NOCTURNE does not do yet, in the order it is likely to be built (as of 0.5.1), plus what is intentionally out of scope. What already works is in [FEATURES.md](FEATURES.md); verification status is in [QA_REPORT.md](QA_REPORT.md). No dates are promised. Priorities can change with user feedback and with what upstream Jellyfin changes.

## 0.2.x (earlier release; GPUI interface)

Shipped: browsing, search, details, in-window GPU playback with a full player, accounts (password, Quick Connect, switching), playlists and a music queue, an admin dashboard inside the same app, settings, packaging for Arch Linux. See [FEATURES.md](FEATURES.md) for the full list.

Still being verified on a real server (built and tested on a throwaway server, not yet on a real library):
- Newer player features: resume prompt, A-B loop, audio device list, external subtitles, subtitle presets
- Multi-season shows and the photo viewer on real data
- A 401 in the middle of a session and the offline banner
- The image cache size cap on a very large library, and a multi-day soak

## Done: SyncPlay ("watch together")

Shipped after 0.2.x (see [syncplay.md](syncplay.md)): list, create, join and leave groups, synchronised play, pause, seek, stop and next, clock sync, drift correction and the Ready / Buffering handshake, for the embedded and external players. Still to do: show what a group is playing (the API does not say), group repeat and shuffle modes, queue editing from the group, resume positions, and testing with more than two members and with transcoded members.

## Shipped in 0.3: a new foundation

- **Iced UI.** The interface moved from GPUI to Iced 0.14 with the same design and features; GPUI is removed. In-window video keeps working on Wayland ([DECISIONS.md](DECISIONS.md)). A reduce-motion setting follows the desktop.
- **libmpv no longer required to start.** It is loaded at run time; without it playback falls back to the `mpv` binary or explains what to install. The release installer installs runtime packages itself.
- **Local library, several servers, one catalog** ([SOURCES.md](SOURCES.md)): use NOCTURNE with local folders only, with several Jellyfin servers and accounts, or both; duplicates across sources shown once with alternate sources; a first-run choice between the three modes; a Sources section in Settings; local folders with scan progress and manual matching.
- **Player loading overlay** (Stremio-style) when a stream opens.
- **Version picker**: movies and episodes with several versions, and copies of the same title on other servers or in local folders, are offered in one "Versions and sources" choice on the detail page.
- **Footprint baselines** (size, start time, idle memory and CPU) tracked per release ([PLATFORMS.md](PLATFORMS.md)).

- **mpv 0.42 certificate check:** done, no change needed ([QA_REPORT.md](QA_REPORT.md)).
- **First-run setup and single instance:** a short setup flow (Local library, Connect to a server, or Both; rerun from Settings), and a second launch just raises the running window.

Not finished in 0.3 and carried forward: screen-reader support (limited by Iced).

## Shipped in 0.4 (2026-11-18)

Dual (secondary) subtitles; audio and subtitle language remembered per show; truthful playback reports with visible errors; Latest row with new episodes, Play on a show card = next unwatched episode, "Included in" collections; optional extra HTTP header per server; HDR pop-out window ([HDR.md](HDR.md), untested on an HDR display); online metadata and subtitle download for local media ([METADATA.md](METADATA.md)); offline copies from a server; one watched state across sources and Trakt ([TRAKT.md](TRAKT.md)); smart collections; remote control from other Jellyfin apps; couch mode with gamepad support (no gamepad tried); player menus by keyboard; .deb and .rpm packages. Per-feature status: [FEATURES.md](FEATURES.md).

## Shipped in 0.5 and 0.5.1 (2026-11-19)

- **Your look**: import CSS themes and apply them globally or per server; themes restyle cards, Home rows, hero, header, buttons and the player bar on named parts (Marquee and Feed are the examples); no browser engine, nothing executed ([CUSTOMIZATION_PLAN.md](dev/CUSTOMIZATION_PLAN.md)).
- **Server UI** per server: that server's own Home sections, My media row, branding as a theme layer, login disclaimer, and a home-sections editor behind server changes ([SERVER_UI_PLAN.md](dev/SERVER_UI_PLAN.md)).
- **In-app updates**: daily check you can turn off, ed25519-signed downloads for per-user installs, the right command for package installs ([UPDATER_PLAN.md](dev/UPDATER_PLAN.md)).
- **Linux is Wayland-only**; signed pacman, apt and dnf repositories; the tarball self-updates; Windows (x64) and macOS (Apple Silicon) unsigned previews on the GitHub release; a best-effort experimental AppImage; release artifacts built in CI by `release.yml` and signed locally.
- Real text editing in every field, one-click Trakt connect with a built-in client id, headless test-session scripts for contributors.
- 0.5.1: a macOS `.tar.gz` for self-update, a theme import that creates a new theme instead of overwriting the selected one, a steadier input check.

Not yet verified (see [QA_REPORT.md](QA_REPORT.md) and [FEATURES.md](FEATURES.md)): the updater end to end from a published release, Windows and macOS on real machines, pointer-only theme actions, Server UI against a server with real branding CSS or several servers at once.

## Next: 0.6

From the maintainer's brief ([dev/V06_BRIEF.md](dev/V06_BRIEF.md)); not started, no dates.

- **Seerr requests** inside the app (request movies and shows, see Requested / Pending / Available; the request is a write to the Seerr server, so it will sit behind its own explicit setting and the user's Seerr permissions). Downloading stays in the user's own stack.
- **Play and save links with yt-dlp**: play a pasted video link in the player (mpv's yt-dlp hook), and optionally save it into a local folder through the downloads queue (off by default). `yt-dlp` stays an optional, never-bundled dependency.
- **Native flex/grid layout engine for themes** (stage 2): each screen becomes a tree of NOCTURNE components laid out natively so a stylesheet can rearrange whole screens. Researched on a branch, not released; still no web view, HTML or arbitrary elements. May move to 0.7 if it is too large.
- Evaluate Flatpak (sandbox permissions for GPU, Wayland, keyring and local folders), and the path from the Windows and macOS previews to signed installers once real-machine testing exists.

## Then: compatibility and correctness

A review of the Jellyfin ecosystem (server, web and other official clients: issues, pull requests, API changes) found no critical problems in NOCTURNE. It did find a set of small fixes worth doing before new features.

- Trickplay hover previews that stay correct on libraries scanned before Jellyfin 12
- Admin Users: stop showing a password status that Jellyfin 12 no longer reports reliably
- Remove an unused helper that builds URLs with the removed `api_key` parameter
- Tests ready for Jellyfin 13.0 changes once they are released (route casing, user lookup rules, new SyncPlay fields)
- A hardware check of HDR HEVC with text subtitles on NVIDIA, after an open upstream mpv report

## Then: playback depth

- Intro and credits prompts that stay visible for the whole segment and cope with overlapping segments from two providers
- Optional per-server trust for a private or self-signed certificate, if a real need appears (the mpv 0.42 check found none)

## Then: library depth

- Library filters for audio and subtitle language
- An optional combined Continue Watching and Next Up row
- Display language sent to the server so library and genre names follow it
- Lyrics for music, and audiobook playback with chapters, together with the music items under "Later"

## Later: missing features

**Media types and playback**
- Books and audiobooks: reading and playback (chapters now come from the server)
- Music: album-art views, a queue that survives closing the app, queue editing while browsing, multi-disc grouping
- Collections: create, add, remove, reorder
- Sharing playlists with other users
- Flatpak (to be evaluated) and other distribution packages; the AppImage stays experimental (see [PACKAGING.md](PACKAGING.md))

**Accounts and settings**
- A user picker on the sign-in screen (switching already works from the account menu)
- A local PIN for profiles (it would protect the profile picker only, not the server)
- Quick Connect authorization on behalf of another user (administrators)
- Subtitle appearance presets
- Ending your other sessions (the server only allows it for administrators)

**Admin**
- Per-user access schedules, "block unrated" kinds, per-folder deletion rights
- Subtitle, lyric and media-segment provider ordering
- Networking: base URL, HTTPS and certificates, proxies and IP filters are view-only on purpose; editing them safely needs a lock-out guard that can be tested

**Platform**
- Packages: Arch is the only distribution tested on a real install; the .deb and .rpm (apt and dnf repositories) are install-tested in containers only ([PLATFORMS.md](PLATFORMS.md))
- Screen-reader support: re-evaluated on Iced once the migration ships (Iced's accessibility support is still limited)
- HDR in the window depends on libmpv gaining a modern render API; until then it is tone-mapped to SDR, with an HDR pop-out window in 0.4, untested on an HDR display ([HDR.md](HDR.md))
- Windows and macOS: signed installers, native in-window video, media keys and real-machine testing; unsigned previews are on the GitHub release since 0.5, never run on real hardware ([PLATFORMS.md](PLATFORMS.md))

## Dropped (will not be built)

- Live TV and DVR
- DLNA setup
- Acting as a cast target
- Gapless music playback

## Upstream reports

A review of the Jellyfin ecosystem (server, web and other official clients: issues, pull requests, API changes) found no critical problems in NOCTURNE; the items above come from it. Anything that belongs upstream is prepared as a draft and written up by hand by a person before it is sent; nothing is filed automatically.
