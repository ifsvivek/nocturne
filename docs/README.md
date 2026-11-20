# Documentation

## Using NOCTURNE

- [Features](FEATURES.md): what the app does, by area.
- [Sources](SOURCES.md): how local folders and several servers become one catalog.
- [Platforms](PLATFORMS.md): supported systems, package formats and what each platform lacks.
- [Writes](WRITES.md): which actions change data on a server, and the switches that gate them.
- [HDR](HDR.md): how HDR video is handled and what an HDR setup needs.
- [Trakt](TRAKT.md): connecting a Trakt account.
- [Metadata](METADATA.md): online metadata providers for local media, and their credits.
- [Watch together](syncplay.md): SyncPlay groups.
- [Buffering](buffering.md): network buffering settings.

## Project

- [Roadmap](ROADMAP.md): what is planned.
- [Changelog](../CHANGELOG.md): what changed in each release.
- [Security](../SECURITY.md): reporting a vulnerability.
- [Contributing](../CONTRIBUTING.md): building, testing and the ground rules.

## Internals

- [Architecture](ARCHITECTURE.md): crates and data flow.
- [Decisions](DECISIONS.md): why things are the way they are.
- [Design](DESIGN.md): colours, type and layout.
- [Video embedding](video-embedding.md): how video is drawn inside the window.
- [Packaging](PACKAGING.md): package formats, dependencies, build profiles and releasing.
- [QA report](QA_REPORT.md): what was verified and how.

## Configuration files

- Settings: `$XDG_CONFIG_HOME/nocturne/settings.toml` (default `~/.config/nocturne/`). Preferences only. Saved themes are in `themes.toml` next to it.
- Sources: `sources.toml` in the same directory lists servers (address, user name, label) and local folders, their order and whether each is enabled. No secrets.
- Sign-in tokens: the system keyring (Secret Service), one per server account. Never a file.
- Local library: the index, manual matches and local watch state are under `$XDG_DATA_HOME/nocturne/local/`. Deleting that directory only makes NOCTURNE forget what it learned; media files are untouched.
- Developer mode: `NOCTURNE_JF_URL` and `NOCTURNE_JF_TOKEN` (a server API key) add a transient, read-only source that is never saved. Debug builds also read them from a git-ignored `.env`; release builds never read `.env`.

## Troubleshooting

- **"NOCTURNE needs a Wayland session".** You are on an X11 session. Log in to the Wayland session of your desktop. X11 is not supported.
- **The window does not open or is blank.** The interface renders with Vulkan. Check that a driver is installed (`vulkaninfo --summary`) and run `nocturne` from a terminal to see its messages.
- **Playing says libmpv is missing.** The app falls back to the `mpv` program if it is installed, and otherwise says what to install: `mpv` on Arch, `libmpv2` on Debian and Ubuntu, `mpv-libs` on Fedora. Browsing keeps working.
- **Video opens in a separate mpv window.** This happens with `NOCTURNE_VIDEO=external`, when the Wayland surface cannot be set up (the terminal says `video: embedded unavailable`), and on Ubuntu 24.04. `NOCTURNE_VO` picks that window's video output.
- **HDR video looks like SDR.** Video inside the window is tone-mapped to SDR. HDR needs the pop-out window, an HDR display, a compositor with colour management and mpv 0.42 or newer. See [HDR](HDR.md).
- **"No system keyring found".** No Secret Service provider is running, so you sign in again next time. Start gnome-keyring, KWallet or KeePassXC with Secret Service enabled.
- **A source shows "Signed out".** That server rejected the login. Use Sign in again in Settings > Sources. Other sources are not affected. A server that does not answer shows "Offline" and is retried with backoff.
- **A local folder shows as unavailable.** Its drive is probably not mounted. Items return when it is, and nothing is removed while a folder is away.
- **The update banner offers a download instead of installing.** Package-managed installs update through the package manager, and a per-user install whose directory you cannot write only offers the download. Settings > Updates turns the daily check off.
- **Progress, watched or favourite changes do nothing.** They are off by default. Enable server changes in Settings; see [Writes](WRITES.md).
- **The video looks washed out in a floating window.** Some compositors apply a window opacity rule to every window. Exempt the app id `nocturne` ([Video embedding](video-embedding.md)).
- **Pacman says "target not found: nocturne".** The `[nocturne]` section is missing from `/etc/pacman.conf`. Repeat step 2 of the install. gpg's note about rejected SHA1 third-party key signatures concerns Arch's own keyring and is harmless.
- **Windows or macOS warns about an unknown publisher.** The previews are unsigned. See [Platforms](PLATFORMS.md).
- **Reporting a bug.** Use the issue template and remove access tokens, server addresses and user names from any logs first.

The UI uses the embedded Noto Sans and JetBrains Mono fonts. Keyboard navigation and visible focus work everywhere; screen-reader support is limited by what Iced exposes.
