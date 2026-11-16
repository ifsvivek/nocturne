# Packaging

Everything here was tested on Arch Linux, x86_64, Wayland with Xwayland, mpv 0.41.0, Rust 1.99 (the 0.2.0 release;
the link list below is that GPUI build's, re-check it for the Iced build). Other distributions and architectures are
UNVERIFIED.

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

**Linked** (`readelf -d`): `libEGL.so.1`, `libdbus-1.so.3`, `libxcb.so.1`, `libxkbcommon.so.0`,
`libxkbcommon-x11.so.0`, glibc, libgcc_s.

**Loaded at run time with `dlopen`** (invisible to `readelf`/namcap, so they are listed explicitly in the PKGBUILD
and in `install.sh`): `libmpv.so.2` (in-window video, `crates/player/src/mpv_ffi.rs`), `libvulkan.so.1` (the UI
renderer), `libwayland-client.so.0`, `libwayland-egl.so.1`. A Vulkan driver (Mesa or NVIDIA) is also required;
it depends on the GPU, so the installer does not pick one. libmpv pulls in about 200 more libraries (ffmpeg, libass,
...) that the distribution's mpv package resolves. A Secret Service provider (gnome-keyring, KWallet, KeePassXC)
stores the login.

Because libmpv is not linked, a missing `libmpv.so.2` does not stop the app from starting.
`nocturne_player::libmpv_available()` reports it, and the app can fall back to the external `mpv` child-process
player (needs the `mpv` binary, see `mpv_binary_available()`) or show `install_hint()`, a distro-specific command
from `/etc/os-release`. libmpv is not needed at build time, so `mpv` is not a makedepend.

| Distribution | Packages (all needed at run time) | Status |
|---|---|---|
| Arch / CachyOS / Manjaro | `mpv libglvnd vulkan-icd-loader wayland libxkbcommon libxkbcommon-x11 libxcb dbus` + `vulkan-driver`, `gcc-libs glibc`, `fontconfig freetype2` (PKGBUILD) | VERIFIED names (`pacman -Qo`); package built, binary launched |
| Debian / Ubuntu | `libmpv2 mpv libegl1 libvulkan1 libwayland-client0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 libxcb1 libdbus-1-3` + `mesa-vulkan-drivers` | names UNVERIFIED in a container |
| Fedora | `mpv-libs mpv libglvnd-egl vulkan-loader libwayland-client libwayland-egl libxkbcommon libxkbcommon-x11 libxcb dbus-libs` + `mesa-vulkan-drivers` | UNVERIFIED (mpv comes from RPM Fusion) |
| openSUSE | `libmpv2 mpv libEGL1 libvulkan1 libwayland-client0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 libxcb1 libdbus-1-3` + `libvulkan_intel`/`libvulkan_radeon` | UNVERIFIED |

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

## Not shipped

AppImage: not provided yet (planned, to be evaluated: [PLATFORMS.md](PLATFORMS.md)). libmpv's closure is about 205 shared libraries (about 240 MB uncompressed) including GPL
codecs, the GPU stack (libEGL/libvulkan and driver ICDs) must come from the host and cannot be bundled, and a
binary built on a rolling glibc would not be self-contained on older systems. Use the tarball or the distribution
package, which take libmpv from the system.

## Vendored xattr

Only the legacy GPUI crate needs this; it goes away with GPUI at the cut-over.

`vendor/xattr` is the crates.io `xattr` 0.2.3 with one line changed (`ENOATTR` is defined as `ENODATA`).
GPUI pulls it in through `zed-async-tar`, which requires the 0.2 series, and 0.2.3 is the last 0.2 release. It
uses `libc::ENOATTR`, which current `libc` (0.2.190 and later) no longer exports for Linux, so the unpatched crate does
not compile (checked in a clean copy without the `[patch.crates-io]` entry: `error[E0425]: cannot find value ENOATTR`).
Remove the patch and the directory once GPUI or `zed-async-tar` moves to a fixed `xattr`.

## Checks

`scripts/ci-checks.sh` runs fmt, clippy `-D warnings`, tests, `cargo deny` (`deny.toml`, if installed) and a secret
scan of tracked files and history. `scripts/gen-notices.py > THIRD_PARTY_NOTICES.md` regenerates the notices after
a dependency change. Replace the `SKIP` checksum in the PKGBUILD before a release.
