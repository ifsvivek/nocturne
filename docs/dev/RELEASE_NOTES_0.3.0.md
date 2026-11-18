# NOCTURNE 0.3.0 (release notes draft)

Draft for the GitHub release. The maintainer fills in the checksums and publishes; delete this file or keep it as the record.

## Highlights

- **One library from local folders and Jellyfin servers.** Use NOCTURNE with only local media, with one or several Jellyfin servers (each with its own account), or both. A short first-run setup flow asks which; Settings > Sources manages them.
- **Duplicates become versions and sources.** A title found in several places shows once; one "Versions and sources" picker on its page lists a server's own versions (4K, 1080p) and the copies elsewhere. Matching uses provider ids, never the title alone.
- **Local library.** Folders are scanned from file names, `.nfo` files and sidecar art (plus `ffprobe` details if installed), with progress, unmatched items you can match by hand, and resume and watched state kept locally. Your files are never modified.
- **A new interface on Iced**, replacing GPUI: the same design and keys, with a Stremio-style loading overlay in the player when a video opens, a reduce-motion setting that follows your desktop, and a startup that does not wait for the network.
- **Guided first run and one window.** A short setup flow (Local library, a server, or both; rerun from Settings) and a second launch raises the running window.
- **libmpv is optional at start.** If it is missing the app still opens and browses, falls back to the `mpv` program, or tells you what to install. The tarball installer installs missing runtime libraries for you.
- Video still plays inside the window with hardware decoding on Wayland, and in an external mpv window on X11.
- Everything 0.2 had: playback, accounts and profile picker, watch together, playlists, music queue and the admin dashboard.

## Upgrading from 0.2

- Your server login becomes a source automatically on first start (`sources.toml`); you do not sign in again. The tokens stay in your system keyring.
- Your settings are kept. The old `servers.toml` and `accounts.toml` are left where they are, so you can go back to 0.2.
- Nothing is changed on your server. Server changes (progress, watched, favourites, admin actions) are still off until you turn them on, and each goes only to the server the item came from.
- Release builds no longer read a `.env` file.
- The local library is new: add folders in Settings > Sources. NOCTURNE only reads them.

## Install

- **Arch Linux**: build the `PKGBUILD` from the release (`makepkg -si`); pacman installs `mpv` and the other libraries.
- **Tarball** (`nocturne-0.3.0-x86_64-linux.tar.gz`): `sha256sum -c` the checksum, extract, run `sudo ./install.sh`. It checks the runtime libraries and installs missing ones with your package manager (it prints the command first).
- **From source**: `cargo build --release --locked` (Rust 1.89+); no libmpv development files are needed.

## Known limitations

- **HDR video plays in the window as SDR** (tone-mapped); mpv's embedding API cannot output HDR. An HDR pop-out window is planned for 0.4 ([docs/HDR.md](../HDR.md)).
- **X11 sessions use an external mpv window**; in-window video needs Wayland.
- **The tarball is built on Arch** and needs a current glibc; on other distributions it may refuse to start, so build from source there. .deb, .rpm and AppImage packages are planned for 0.4. Only Arch is tested.
- Developed and tested on NVIDIA proprietary drivers; AMD and Intel GPUs are untested.
- **Screen readers**: Iced's accessibility support is limited; keyboard navigation and visible focus work everywhere.
- Online metadata for local files is not in 0.3 (planned for 0.4, opt-in).
- Tested against Jellyfin 12.4 only. Several newer 0.2 player features have only been verified on a throwaway server ([docs/QA_REPORT.md](../QA_REPORT.md)).

## Checksums

```
SHA256  nocturne-0.3.0-x86_64-linux.tar.gz  <fill in at release>
SHA256  PKGBUILD                            <fill in at release>
```
