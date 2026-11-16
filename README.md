<p align="center"><img src="packaging/nocturne.svg" alt="NOCTURNE" width="96"></p>

# NOCTURNE

**YOUR MEDIA. YOUR NIGHT.**

A native, cinematic desktop client for Jellyfin, written in Rust and playing video in the app window through libmpv.
Linux first, Wayland first. The released 0.2.0 uses [GPUI](https://www.gpui.rs/) for its interface; 0.3 moves to
[Iced](https://iced.rs/).

[Website](https://nocturne.ifsvivek.in) · [Download](https://github.com/ifsvivek/nocturne/releases/latest) · [Features](docs/FEATURES.md) · [Roadmap](docs/ROADMAP.md) · [Changelog](CHANGELOG.md)

Status: version 0.2.0, early software, Linux (tested on Arch only); macOS and Windows are planned, not available yet
([docs/PLATFORMS.md](docs/PLATFORMS.md)). **0.3 is in development**: a new Iced interface replacing GPUI,
local media folders and several Jellyfin servers in one library, and an installer that sets up libmpv for you.
See [docs/ROADMAP.md](docs/ROADMAP.md) for what is coming and what is missing.

## Screenshots

Of the released 0.2.0 (GPUI interface), captured at 3840x2160 against a generated demo library, not personal media.

|                                                           |                                                               |
| --------------------------------------------------------- | ------------------------------------------------------------- |
| ![Who's watching](docs/screenshots/picker.webp)           | ![Home](docs/screenshots/home.webp)                           |
| ![Library](docs/screenshots/library.webp)                 | ![Movie detail](docs/screenshots/detail.webp)                 |
| ![Series](docs/screenshots/series.webp)                   | ![Season episodes](docs/screenshots/series-episodes.webp)     |
| ![Search](docs/screenshots/search.webp)                   | ![Player](docs/screenshots/player.webp)                       |
| ![Settings](docs/screenshots/settings.webp)               | ![Settings drop-down](docs/screenshots/settings-dropdown.webp) |
| ![Account menu](docs/screenshots/account-menu.webp)       | ![Admin dashboard](docs/screenshots/admin.webp)               |
| ![Admin users](docs/screenshots/admin-users.webp)         | ![Admin scheduled tasks](docs/screenshots/admin-scheduled-tasks.webp) |

## What it does

The full checklist is [docs/FEATURES.md](docs/FEATURES.md); verification notes are in
[docs/QA_REPORT.md](docs/QA_REPORT.md).

- **Browse**: home with Continue Watching and Next Up, paged libraries with sort, filters, A-Z and poster / thumb / list views,
  genres, studios, years, tags, collections, playlists and favourites, global search (Ctrl+K or `/`), people pages.
- **Details**: movie, series, season and episode pages with cast, chapters, similar items, extras and media info.
- **Playback**: in-window GPU video via libmpv with hardware decoding, chapters and trickplay on the seek bar,
  audio and subtitle menus with delays and styling, speed and picture controls, skip intro and credits,
  next-episode autoplay, queue, quality selector with transcode fallback, mini player, MPRIS and idle inhibit.
- **Accounts**: username and password or Quick Connect (and authorizing other devices); switch between remembered accounts without
  a password, with a "Who's watching?" profile picker at launch (and an optional default profile); the token lives in your
  system keyring.
- **Watch together**: SyncPlay groups on your server; play, pause, seek and skip stay in step for everyone (press `g`).
- **Playlists and music**: create, edit, reorder and delete your playlists; a music queue with play next, shuffle and repeat.
- **Admin** (administrator accounts): dashboard, users (access and limits), libraries, providers, scheduled tasks and triggers,
  devices, activity, logs, API keys, branding, networking, transcoding, trickplay, backups and plugins, each change behind a
  confirmation dialog.

Not implemented: Live TV, DLNA, casting, gapless music, playable books and audiobooks, and a few admin settings (HTTPS and
proxy editing, access schedules). Windows and macOS are not supported yet (planned, see [docs/PLATFORMS.md](docs/PLATFORMS.md)).

## Install

NOCTURNE plays video with mpv's library, libmpv (package `mpv` on Arch, `libmpv2` on Debian/Ubuntu, `mpv-libs` on
Fedora), and needs a Vulkan driver, an OpenGL/EGL stack, and a Secret Service provider (gnome-keyring, KWallet or
KeePassXC) to remember your login. The Arch package depends on `mpv`, so pacman installs it. From 0.3 the release
installer checks for every runtime library and installs missing ones with your distribution's package manager;
with 0.2.0, install libmpv yourself first. Packages are tested on Arch Linux only; see
[docs/PACKAGING.md](docs/PACKAGING.md) for the full dependency list.

**Release tarball** (0.2.0; from the [latest release](https://github.com/ifsvivek/nocturne/releases/latest)):

```sh
sha256sum -c nocturne-0.2.0-x86_64-linux.tar.gz.sha256
tar xf nocturne-0.2.0-x86_64-linux.tar.gz && cd nocturne-0.2.0-x86_64-linux
sudo ./install.sh              # PREFIX=/usr/local by default; `sudo ./install.sh uninstall` removes it
```

**Arch Linux** (builds from the source archive of the release tag; the `PKGBUILD` is attached to the release and lives in `packaging/`):

```sh
git clone https://github.com/ifsvivek/nocturne && cd nocturne/packaging && makepkg -si
```

**From source**:

```sh
# Arch: sudo pacman -S rust mpv pkgconf libxkbcommon-x11 vulkan-icd-loader
# Debian/Ubuntu (names may differ): pkg-config libxkbcommon-x11-dev libvulkan1 libwayland-dev libdbus-1-dev
git clone https://github.com/ifsvivek/nocturne && cd nocturne
cargo build --release --locked      # needs stable Rust 1.89+; libmpv is only needed at run time
./target/release/nocturne
```

A clean release build takes several minutes (fat LTO).

## Supported environments

| Area | Status |
|---|---|
| OS and architecture | Linux, x86_64. macOS and Windows are planned, not available yet ([docs/PLATFORMS.md](docs/PLATFORMS.md)). |
| Packages | Tested on Arch Linux only (see [docs/PACKAGING.md](docs/PACKAGING.md)). Other distributions should build from source but are unverified. |
| Wayland | In-window video (libmpv in a subsurface). Developed and tested here, on NVIDIA proprietary drivers; AMD and Intel GPUs are untested. |
| X11 | The interface works; video plays in an external mpv window that closes with the app. Tested through Xwayland. |
| Server | Jellyfin 12.4 (a real server and a throwaway one). Other versions are untested; other media servers are not supported. Local media folders arrive with 0.3 (in development). |
| Needed at run time | libmpv, a Vulkan driver, EGL, a Secret Service provider to remember logins |

## Troubleshooting

- **The window opens but no video appears, or the app fails to start.** The interface renders with Vulkan: check that a Vulkan
  driver is installed (`vulkaninfo --summary`). Run `nocturne` from a terminal to see its messages.
- **Video opens in a separate mpv window.** That is the fallback when in-window video is unavailable (X11, or
  `NOCTURNE_VIDEO=external`, or a failure to set up the Wayland surface; the terminal says
  `video: embedded unavailable`). To force a different mpv video output set `NOCTURNE_VO`.
- **"libmpv.so.2: cannot open shared object file" at start (0.2.0).** Install your distribution's libmpv package
  (`mpv` on Arch, `libmpv2` on Debian/Ubuntu, `mpv-libs` on Fedora). From 0.3 NOCTURNE starts without it and tells you
  what to install.
- **"No system keyring found".** No Secret Service provider is running, so you will sign in again next time. Start gnome-keyring, KWallet or KeePassXC with
  the Secret Service integration enabled; NOCTURNE never writes tokens to a file.
- **"Your session ended".** The server revoked the login (for example the device was removed in the dashboard);
  sign in again.
- **Progress, watched or favourite changes do nothing.** They are off by default: enable
  **Settings > Allow NOCTURNE to change data on the server** ([docs/WRITES.md](docs/WRITES.md)).
- **Reporting a bug.** Use the issue template and remove access tokens, server addresses and usernames from any
  logs before posting.

## Usage notes

- **X11.** In-window video needs Wayland. On other sessions NOCTURNE plays in an external mpv window, which closes
  with the app. `NOCTURNE_VIDEO=external` forces this on Wayland too.
- **HDR.** Video inside the window is shown in SDR (HDR is tone-mapped): mpv's embedding API cannot output HDR yet.
  A planned HDR mode (0.4) lets you choose, per video, between playing in the window in SDR and opening an HDR
  pop-out window ([docs/HDR.md](docs/HDR.md)).
- **Fonts.** The UI uses Noto Sans and JetBrainsMono Nerd Font if installed, and falls back to system fonts.

## Configuration and keyring

- Settings: `$XDG_CONFIG_HOME/nocturne/settings.toml` (default `~/.config/nocturne/`). It holds preferences only.
- Remembered servers: `$XDG_CONFIG_HOME/nocturne/servers.toml`, URLs only.
- Login token: your system keyring (Secret Service), never a file. If no Secret Service is running, NOCTURNE cannot
  remember the login.
- Developer mode: `NOCTURNE_JF_URL` and `NOCTURNE_JF_TOKEN` (a Jellyfin API key, read from the environment or a
  git-ignored `.env`) skip sign-in. This mode is strictly read-only.

## Safety model: read-only by default

NOCTURNE changes nothing on your server unless you say so. One setting,
**Settings > Allow NOCTURNE to change data on the server** (`allow_server_writes`), is off by default and gates
everything that writes: playback progress, watched and favourite flags, own password, and every admin action.
Admin actions additionally need an administrator account and a confirmation dialog. Signing in and out only creates
and ends this app's own session. The browsing and playback crate is GET-only and a test enforces it.
Details and the full table: [docs/WRITES.md](docs/WRITES.md).

## Contributing and AI-agent friendly

Contributions are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md) for setup, the throwaway-server test workflow and
the ground rules. The repository is set up so coding agents can work in it safely: [CLAUDE.md](CLAUDE.md) (also
referenced from [AGENTS.md](AGENTS.md)) has the crate map, commands and safety rules, and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the pieces fit together.

## Documentation

[Features](docs/FEATURES.md) | [Architecture](docs/ARCHITECTURE.md) | [Decisions](docs/DECISIONS.md) |
[Sources](docs/SOURCES.md) | [Platforms](docs/PLATFORMS.md) | [Design](docs/DESIGN.md) | [Roadmap](docs/ROADMAP.md) | [Writes](docs/WRITES.md) | [Packaging](docs/PACKAGING.md) | [QA report](docs/QA_REPORT.md) |
[Contributing](CONTRIBUTING.md) | [Security](SECURITY.md) | [Changelog](CHANGELOG.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE); third-party licences are listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Acknowledgements and attribution

- **[Jellyfin](https://jellyfin.org/)** is the media server NOCTURNE talks to. "Jellyfin" and its logo are trademarks of
  the Jellyfin project. NOCTURNE is an unofficial third-party client; it is not affiliated with, endorsed by or
  sponsored by Jellyfin, and uses the name only to say which server it works with.
- **[Iced](https://iced.rs/)**, the UI framework of 0.3 (MIT), and **[GPUI](https://github.com/zed-industries/zed)** by Zed
  Industries, the UI framework of the released 0.2.0 (Apache-2.0), which Iced replaces.
- **[mpv](https://mpv.io/) / libmpv** for playback and hardware decoding.
- **[Rust](https://www.rust-lang.org/)** and the crates NOCTURNE builds on, including tokio, reqwest, rustls, serde, zbus,
  keyring, wayland-client and image. The full list with licences is in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- `vendor/xattr` is a patched copy of the [xattr](https://github.com/Stebalien/xattr) crate (MIT / Apache-2.0), needed only
  by GPUI and removed with it.

---

> **Unofficial. Not affiliated with, endorsed by or sponsored by the Jellyfin project.**
> NOCTURNE is an independent client that talks to your own Jellyfin server over its public API.
