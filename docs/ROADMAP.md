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
- **Player loading overlay** (Stremio-style) when a stream opens.
- **Version picker**: movies and episodes with several versions, and copies of the same title on other servers or
  in local folders, are offered in one "Versions and sources" choice on the detail page.
- **Footprint baselines** (size, start time, idle memory and CPU) tracked per release ([PLATFORMS.md](PLATFORMS.md)).

- **mpv 0.42 certificate check:** done, no change needed ([QA_REPORT.md](QA_REPORT.md)).
- **First-run setup and single instance:** a short setup flow (Local library, Connect to a server, or Both; rerun from Settings), and a second launch just raises the running window.

Not finished in 0.3 and carried forward: screen-reader support (limited by Iced).

## 0.4 (in development)

Local branch, not released. Contents: dual (secondary) subtitles; audio and subtitle language remembered per show; truthful
playback reports with visible errors; Latest row with new episodes, Play on a show card = next unwatched episode, "Included in"
collections; optional extra HTTP header per server; **HDR pop-out window** ([HDR.md](HDR.md)); online metadata and subtitle
download for local media ([METADATA.md](METADATA.md)); offline copies from a server; one watched state across sources and Trakt
([TRAKT.md](TRAKT.md)); smart collections; remote control from other Jellyfin apps; couch mode with gamepad support; player
menus by keyboard; .deb and .rpm packages; and macOS / Windows code behind the platform seams ([PLATFORMS.md](PLATFORMS.md)).
Not yet verified: live Trakt sign-in, HDR on an HDR display, a real gamepad and TV, anything on real Windows or macOS (the
macOS code has never even been built). Status and the release checklist: [dev/STATUS.md](dev/STATUS.md).

## Next: 0.5

Headline: a per-server choice of interface. For each server you pick NOCTURNE's own interface or a native rendering of the
server's own UI (its home sections, display settings and custom CSS interpreted by NOCTURNE; no web view), so people who have
tuned their Jellyfin look keep it. The exact meaning is agreed with the maintainer when 0.5 starts. Around it: interface
refresh and customization comparable to Jellyfin's display settings (themes and accent, home sections and their order, density
and scale, backdrops, per-library views). Also: evaluate Flatpak (sandbox permissions for GPU, Wayland, keyring and local
folders), and decide on a macOS / Windows installer path once real-machine testing exists.

## Then: compatibility and correctness

A review of the Jellyfin ecosystem (server, web and other official clients: issues, pull requests, API changes) found no
critical problems in NOCTURNE. It did find a set of small fixes worth doing before new features.

- Trickplay hover previews that stay correct on libraries scanned before Jellyfin 12
- Admin Users: stop showing a password status that Jellyfin 12 no longer reports reliably
- Remove an unused helper that builds URLs with the removed `api_key` parameter
- Tests ready for Jellyfin 13.0 changes once they are released (route casing, user lookup rules, new SyncPlay fields)
- A hardware check of HDR HEVC with text subtitles on NVIDIA, after an open upstream mpv report

## Then: playback depth (0.6)

- Intro and credits prompts that stay visible for the whole segment and cope with overlapping segments from two
  providers
- Optional per-server trust for a private or self-signed certificate, if a real need appears (the mpv 0.42 check found none)

## Then: library depth (0.7)

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
- Flatpak (to be evaluated in 0.5) and other distribution packages; no AppImage (see [PACKAGING.md](PACKAGING.md))

**Accounts and settings**
- A user picker on the sign-in screen (switching already works from the account menu)
- A local PIN for profiles (it would protect the profile picker only, not the server)
- Quick Connect authorization on behalf of another user (administrators)
- Theme and accent choice, subtitle appearance presets
- Ending your other sessions (the server only allows it for administrators)

**Admin**
- Per-user access schedules, "block unrated" kinds, per-folder deletion rights
- Subtitle, lyric and media-segment provider ordering
- Networking: base URL, HTTPS and certificates, proxies and IP filters are view-only on purpose; editing them
  safely needs a lock-out guard that can be tested

**Platform**
- Packages: Arch is the only distribution tested on a real install; .deb and .rpm (0.4) are install-tested in containers only ([PLATFORMS.md](PLATFORMS.md))
- Screen-reader support: re-evaluated on Iced once the migration ships (Iced's accessibility support is still limited)
- HDR in the window depends on libmpv gaining a modern render API; until then it is tone-mapped to SDR, with an HDR
  pop-out window in 0.4, untested on an HDR display ([HDR.md](HDR.md))
- macOS (.dmg) and Windows (.msi/.exe) installers, bundled libmpv, native in-window video and real-machine testing: code exists since 0.4, nothing has been run ([PLATFORMS.md](PLATFORMS.md))

## Dropped (will not be built)

- Live TV and DVR
- DLNA setup
- Acting as a cast target
- Gapless music playback

## Upstream reports

A review of the Jellyfin ecosystem (server, web and other official clients: issues, pull requests, API changes) found
no critical problems in NOCTURNE; the items above come from it. Anything that belongs upstream is prepared as a draft
and written up by hand by a person before it is sent; nothing is filed automatically.
