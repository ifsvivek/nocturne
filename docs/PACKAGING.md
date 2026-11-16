# Packaging

Built and checked on Arch Linux, x86_64, Wayland (Hyprland), mpv 0.41.0, Rust 1.99. The dependency list below is the
Iced build's (derived 2026-11-16 from `target/release/nocturne`). Other distributions and architectures: package
names were checked to exist in the default repos of Debian stable, Fedora and openSUSE Tumbleweed (containers), nothing
more. Debian/Fedora/openSUSE builds are UNVERIFIED.

**One variable names the app package.** `scripts/package.sh`, `scripts/perf.sh` and `packaging/PKGBUILD` default to
package `nocturne-app` (`crates/app`) / binary `nocturne`; `scripts/gen-notices.py` and `scripts/test-playback.sh` take the same `PKG`/`BIN`). The installed name is always `nocturne`.

Planned, not available yet: .deb, .rpm, AppImage, macOS and Windows packages. Per-platform libmpv handling and the footprint budget are in
[PLATFORMS.md](PLATFORMS.md).

## Install paths

| File | Path under `PREFIX` |
|---|---|
| binary | `bin/nocturne` |
| desktop entry | `share/applications/io.github.ifsvivek.nocturne.desktop` (`StartupWMClass=nocturne`, matching the window `app_id`) |
| AppStream | `share/metainfo/io.github.ifsvivek.nocturne.metainfo.xml` |
| icons | `share/icons/hicolor/{48x48,128x128,256x256,512x512}/apps/nocturne.png`, `scalable/apps/nocturne.svg` |
| licenses | `share/licenses/nocturne/{LICENSE,THIRD_PARTY_NOTICES.md}` |

## Runtime dependencies

How this was derived (all from the release binary, not from memory): `readelf -d` NEEDED shows only `libdbus-1.so.3`,
`libgcc_s.so.1`, `libm.so.6`, `libc.so.6`. Everything else is opened with `dlopen` (invisible to `readelf`/namcap): the
names are visible in the binary (`libEGL.so.1`, `libwayland-client.so.0`, `libwayland-egl.so.1`, `libX11.so.6`,
`libX11-xcb.so.1`, `libXcursor.so.1`, `libXi.so.6`, `libxkbcommon.so.0`, `libxkbcommon-x11.so.0`) or are assembled from
immediates by the compiler (`libvulkan.so.1` from `ash::Entry::load`, `libxcb.so.1` from x11rb's `dl-libxcb`; found as
the `libvulka`+`kan.so.1` / `libxcb.s`+`so.1` byte pairs) and confirmed in the crate sources under `~/.cargo/registry`
(`ash-0.38 entry.rs`, `x11-dl`, `xkbcommon-dl`, `wayland-sys`, winit `wayland-dlopen`). libmpv: `crates/player/src/mpv_ffi.rs`
(`SONAME = "libmpv.so.2"`, `libloading`), verified at run time in the container (`scripts/test-playback.sh`).

**Dropped with GPUI** (not in NEEDED, no string in the binary): `libfontconfig`, `libfreetype` (the Iced text stack,
cosmic-text/fontdb, parses `fonts.conf` in pure Rust; `fontconfig-parser`), `libxcb` as a linked library (now dlopen,
X11 sessions only). **Added:** `libX11`, `libX11-xcb`, `libXcursor`, `libXi` (winit's X11 backend, dlopen). The folder picker
(`rfd`, `xdg-portal` feature, pure-Rust zbus) needs no GTK; it talks to `xdg-desktop-portal` at run time (optional).
`libwayland-cursor` is not used (client-side decorations are drawn by sctk-adwaita).

A Vulkan driver (Mesa `vulkan-*` or NVIDIA) is also required (wgpu; `libEGL` is the GL fallback and the video path); it
depends on the GPU, so the installer does not pick one. libmpv pulls in about 200 more libraries (ffmpeg, libass, ...)
that the distribution's mpv package resolves. A Secret Service provider (gnome-keyring, KWallet, KeePassXC) stores the
login. The UI font is "Noto Sans" by name (not bundled; any sans is the fallback).

Because libmpv is not linked, a missing `libmpv.so.2` does not stop the app from starting.
`nocturne_player::libmpv_available()` reports it, and the app can fall back to the external `mpv` child-process
player (needs the `mpv` binary, see `mpv_binary_available()`) or show `install_hint()`, a distro-specific command
from `/etc/os-release`. libmpv is not needed at build time, so `mpv` is not a makedepend.

| Distribution | Packages (all needed at run time) | Status |
|---|---|---|
| Arch / CachyOS / Manjaro | `mpv libglvnd vulkan-icd-loader wayland libxkbcommon libxkbcommon-x11 libxcb libx11 libxcursor libxi dbus gcc-libs glibc` + `vulkan-driver`; optional `xdg-desktop-portal noto-fonts gnome-keyring` (PKGBUILD) | VERIFIED names (`pacman -Qo` on every soname); package built and launched in a clean container |
| Debian / Ubuntu | `libmpv2 mpv libegl1 libvulkan1 libwayland-client0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 libxcb1 libdbus-1-3 libx11-6 libx11-xcb1 libxcursor1 libxi6` + `mesa-vulkan-drivers`; optional `xdg-desktop-portal fonts-noto-core` | names exist in debian:stable (apt-cache); install not run end to end |
| Fedora | `mpv-libs mpv libglvnd-egl vulkan-loader libwayland-client libwayland-egl libxkbcommon libxkbcommon-x11 libxcb dbus-libs libX11 libX11-xcb libXcursor libXi` + `mesa-vulkan-drivers`; optional `xdg-desktop-portal google-noto-sans-fonts` | names exist in fedora:latest (dnf repoquery); mpv itself comes from RPM Fusion |
| openSUSE | `libmpv2 mpv libglvnd libvulkan1 libwayland-client0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 libxcb1 libdbus-1-3 libX11-6 libX11-xcb1 libXcursor1 libXi6` + `libvulkan_intel`/`libvulkan_radeon`; optional `xdg-desktop-portal google-noto-sans-fonts` | names exist in Tumbleweed (zypper info); `libEGL.so.1` is provided by `libglvnd` (the old `libEGL1` name does not exist) |

How each artifact handles them:

- **Arch package (PKGBUILD)**: `depends` lists all of the above, including `mpv` for the dlopen'd libmpv.
- **Tarball `install.sh`**: on `install` it checks each soname with `ldconfig -p` and the `mpv` binary, installs the
  missing packages with the distro's package manager (via `sudo` unless root; the exact command is printed first),
  then runs `ldd` on the installed binary and warns about anything "not found". The soname-to-package table is the
  `pkg_for` function at the top of the script, the single place to edit. `--no-deps` skips this; `NOCTURNE_YES=1`
  makes the package manager non-interactive; an unrecognised distribution fails with the list of what is missing;
  `DESTDIR` (staging) skips dependency handling.
- **AppImage**: not provided (see below).

The tarball binary is built on a rolling distribution and is not an "any Linux" artifact.

## Build

```sh
# Release tarball -> dist/nocturne-<version>-x86_64-linux.tar.gz (+ .sha256)
scripts/package.sh tarball

# Install from the extracted tarball (PREFIX default /usr/local, DESTDIR optional)
sudo ./install.sh            # installs missing runtime libraries first (--no-deps to skip); or: DESTDIR=/tmp/root PREFIX=/usr ./install.sh
sudo ./install.sh uninstall

# Arch package (set the real sha256 first; builds from the GitHub source archive of the v<version> tag)
cd packaging && makepkg -si
```

`packaging/PKGBUILD` sets `options=(!lto)`: makepkg's default C `-flto` breaks linking of aws-lc-sys's C objects.
Release profile: fat LTO, one codegen unit, stripped, `panic = "unwind"` (a panic in a worker thread must not abort
the UI). Clean build on 16 cores: about 4.5 minutes, about 24 MB binary (thin LTO: 2.3 minutes, 31 MB).

## Pacman repository

Served from the website at `https://nocturne.ifsvivek.in/repo/arch/x86_64/` (`nocturne.db`, `nocturne.files`, the two
newest `nocturne-*.pkg.tar.zst`, each with a `.sig`); `repo/ARCH-GPG-KEY` is the public key, `repo/index.txt` the
instructions. The directory `website/static/repo/` is generated and git-ignored; it ships when the website is deployed.
Package, database and files database are all signed, so users set `SigLevel = Required TrustedOnly` (setup commands in
the README). Static hosting cannot serve symlinks, so `nocturne.db`/`nocturne.files` are real copies.

Signing key: "NOCTURNE Package Signing", fingerprint `5204B42AF768DE210D4979E397FA02EFCF1A4E3B`, ed25519, no passphrase.
The public key is committed at `packaging/repo/ARCH-GPG-KEY`. The secret key exists only in the maintainer's GPG
keyring: back it up offline (`gpg --export-secret-keys` to encrypted removable storage, never to the repository or
the website), and keep the revocation certificate from `~/.gnupg/openpgp-revocs.d/` offline too. Rotating the key means
a new fingerprint, a new `ARCH-GPG-KEY`, and users re-running the two `pacman-key` commands.

Release procedure:

1. Tag `vX.Y.Z`, set `pkgver` and the real `sha256sums` in `packaging/PKGBUILD` (sha256 of the GitHub tag archive).
2. `scripts/publish-repo.sh` builds the package in a clean `archlinux:base-devel` container (deps installed as root,
   `makepkg --nocheck` as a normal user, so it targets current Arch rather than the host), then on the host signs it,
   runs `repo-add --sign --remove`, prunes to the newest two versions and verifies every signature.
3. Deploy the website. The server never holds the key.

## Not shipped

AppImage: not provided yet (planned, to be evaluated: [PLATFORMS.md](PLATFORMS.md)). libmpv's closure is about 205 shared libraries (about 240 MB uncompressed) including GPL
codecs, the GPU stack (libEGL/libvulkan and driver ICDs) must come from the host and cannot be bundled, and a
binary built on a rolling glibc would not be self-contained on older systems. Use the tarball or the distribution
package, which take libmpv from the system.

## Checks

`scripts/test-playback.sh` is the clean-container release gate (package build, install, run on the host Wayland session
with software rendering, headless libmpv clip); what it does and does not prove is in its header and in
[QA_REPORT.md](QA_REPORT.md). `scripts/ci-checks.sh` runs fmt, clippy `-D warnings`, tests, `cargo deny` (`deny.toml`, if installed) and a secret
scan of tracked files and history. `scripts/gen-notices.py > THIRD_PARTY_NOTICES.md` regenerates the notices after
a dependency change. Replace the `SKIP` checksum in the PKGBUILD before a release.
