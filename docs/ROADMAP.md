# Roadmap

Everything NOCTURNE does not do yet, in the order it is likely to be built (as of 0.6.0), plus what is intentionally out of scope. What already works is in [FEATURES.md](FEATURES.md); verification status is in [QA_REPORT.md](QA_REPORT.md). No dates are promised. Priorities can change with user feedback and with what upstream Jellyfin changes.

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

## Shipped in 0.6.0

- **Player choice**: Settings offers the in-window player (the default), VLC and an mpv window, each only when installed. VLC is controlled over a loopback interface with a random password, so resume, progress sync, Trakt and Stop keep working; its own menus handle subtitles and audio. MPC-HC on Windows and IINA on macOS are not supported yet.
- **Calendar**: month, week and agenda views of upcoming episodes from each server (merged) and, with Trakt connected, your Trakt calendar; read only, with an optional "Coming this week" row on Home.
- **Seerr requests**: request movies and shows that no source has, from Search and the Calendar, and follow them on a Requests page. A request goes to the Seerr server only, behind its own setting (off by default). 4K requests and Seerr's server, profile and folder choices are not offered ([SOURCES.md](SOURCES.md), [WRITES.md](WRITES.md)).
- **Themes gallery**: Settings > Themes opens on a Gallery with Official, Community and My themes, previews and one-click install checked against a sha256, from folders in this repository (`themes/`); the Editor keeps the lists and the CSS editor.
- **Flatpak evaluated, not shipped** ([dev/FLATPAK.md](dev/FLATPAK.md)): worth doing after 0.6.0 as a Flathub app with a small sandbox (Wayland, GPU, network, audio, the Secret Service name, `xdg-videos` read-only), in-window playback with a bundled mpv, no host players (VLC is not reachable without a host-spawn permission that Flathub rejects), and the in-app updater switched off. About 3 to 5 working days to a first build plus review time.

## Next: 0.7, add-ons

Add-ons in the style of media-center add-on catalogs: an add-on is a web address that answers a few JSON requests (manifest, catalogs, item details, streams, subtitles). No add-on code runs inside NOCTURNE; it only reads data, as it reads a Jellyfin server.

- An add-on is another source next to local folders and servers: its catalogs become Home rows, its items get detail pages, its streams play in the player.
- An Add-ons page: tabs Official, Community and My add-ons (installed, including ones added by URL), a type filter (movies and series, subtitles, anime, ...) and search; one card per add-on with its logo, name, version, what it provides and a one-line description; Install / Uninstall, a settings button for add-ons that have a configuration page (opened in the browser, which returns the configured add-on URL to paste back; no web view in the app), and Share (copies the add-on URL). The curated catalog is read from an index on the website. The 0.6 themes gallery uses the same layout.
- Add-ons can be written in any language and hosted anywhere (also on the same machine); remote add-ons over https only, with the same size and time limits as theme fetching.
- The spec, a small SDK, examples and the catalog index live in this repository (`addons/`): pull requests welcome, the maintainer merges. Only add-ons for sources the user is entitled to (own media, public-domain catalogs, trailers, subtitles) are listed. The catalog is a curated list, not a gate: "Add by URL" installs any add-on, listed or not (including one running on the same machine), marked "not reviewed".

## Then: 0.8, gateway

`nocturne-gateway`: an optional companion service (in this workspace, off by default) that merges all of a user's servers, local folders and add-ons into one catalog and serves it at one address speaking the subset of the Jellyfin API that the official Jellyfin phone and TV apps need, so they can connect to it as one server; streams are passed through from the item's own server.

- Its own sign-in, local network (or a private overlay network) only, read-only toward the servers it merges unless the user turns writes on.
- It holds sign-ins for every merged server, so it gets a security review before release.
- Controlled from Settings > Gateway (NOCTURNE starts and stops the service, no separate setup): Gateway Off (default) / On; status (Online, Stopped, or the error); Allow connections from: this computer only (default), the local network, or remote over HTTPS (needs a certificate, off by default); caching: none (default) or a size limit; the gateway address with Copy and a QR code for phones; the gateway's own sign-in for phones and TVs (never a server's password). No torrent features.

## Then: 0.9, living room

Brought back from the old "dropped" list (maintainer, 2026-11-20).

- **Live TV and DVR** from Jellyfin: channels, guide, live playback with time-shift where the server allows it, recordings as a library; scheduling and cancelling recordings are server writes, behind `allow_server_writes`.
- **DLNA**: browse DLNA/UPnP media servers on the local network as another source (read-only), and act as a DLNA renderer so a phone or another app can send media to NOCTURNE.
- **Cast target**: besides the Jellyfin remote control NOCTURNE already supports and the DLNA renderer above, be listed as a playback target for the official Jellyfin apps on the network. Google Cast receiving stays out unless an open, licence-compatible path exists.
- **Gapless music playback** (mpv's gapless audio with the next track prefetched), together with the music depth under "Later".

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
- Flatpak (evaluated, recommended after 0.6.0; see above) and other distribution packages; the AppImage stays experimental (see [PACKAGING.md](PACKAGING.md))

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
- Windows and macOS: native in-window video on macOS (Windows plays in the window since 0.5.2), media keys, and more real-machine testing; the unsigned previews on the GitHub release have been tried on a real PC and Mac ([PLATFORMS.md](PLATFORMS.md))

## Reconsidered

Nothing is dropped for good. Earlier decisions to revisit when users ask for them:

- **Theme effects without a native equivalent yet**: conic gradients, `filter: blur()`, `saturate()` and `contrast()`, `backdrop-filter` blur (now a translucent fill), masks, animations on virtualised library cards, and native versions of what popular themes do with JavaScript.
- **Server UI**: plugin pages and custom tabs of the web client, shown natively where a plugin's data is readable.

## Dropped (will not be built)

- X11 sessions on Linux: NOCTURNE is Wayland-only since 0.5.
- A Homebrew tap or cask.
- Playing or saving video links with yt-dlp: it now needs a JavaScript runtime (Deno or Node) for YouTube and breaks whenever sites change (maintainer, 2026-11-21).
- Code signing for Windows and macOS: the builds stay unsigned previews (the system warns on first start). Linux packages and the in-app updates stay signed with NOCTURNE's own keys.

## Upstream reports

A review of the Jellyfin ecosystem (server, web and other official clients: issues, pull requests, API changes) found no critical problems in NOCTURNE; the items above come from it. Anything that belongs upstream is prepared as a draft and written up by hand by a person before it is sent; nothing is filed automatically.
