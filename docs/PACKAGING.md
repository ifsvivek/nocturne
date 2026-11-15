# Packaging

Everything here was tested on Arch Linux, x86_64, Wayland with Xwayland, mpv 0.41.0, Rust 1.99.
Other distributions and architectures are UNVERIFIED.

## Install paths

| File | Path under `PREFIX` |
|---|---|
| binary | `bin/nocturne` |
| desktop entry | `share/applications/io.github.ifsvivek.nocturne.desktop` (`StartupWMClass=nocturne`, matching the window `app_id`) |
| AppStream | `share/metainfo/io.github.ifsvivek.nocturne.metainfo.xml` |
| icons | `share/icons/hicolor/{48x48,128x128,256x256,512x512}/apps/nocturne.png`, `scalable/apps/nocturne.svg` |
| licenses | `share/licenses/nocturne/{LICENSE,THIRD_PARTY_NOTICES.md}` |

## Runtime dependencies

The binary links `libmpv.so.2`, `libEGL.so.1`, `libdbus-1.so.3`, `libxcb.so.1`, `libxkbcommon.so.0`,
`libxkbcommon-x11.so.0`, glibc and libgcc_s directly (`readelf -d`); libmpv pulls in about 200 more libraries
(ffmpeg, libass, ...). Wayland, Vulkan (GPUI renders through it, so a Vulkan driver is required), fontconfig and
freetype are needed at run time too. A Secret Service provider (gnome-keyring, KWallet, KeePassXC) stores the login.

| Distribution | Packages | Status |
|---|---|---|
| Arch / CachyOS | `mpv dbus fontconfig freetype2 gcc-libs glibc libglvnd libxcb libxkbcommon libxkbcommon-x11 vulkan-icd-loader vulkan-driver wayland` | VERIFIED (package built, `pacman -T` satisfied, binary launched) |
| Debian / Ubuntu / Fedora / openSUSE | `libmpv`, Vulkan loader + driver, xkbcommon(-x11), wayland, dbus, fontconfig under the distribution's names | UNVERIFIED (the prebuilt binary also needs a glibc at least as new as the build host's) |

The tarball binary is built on a rolling distribution and is not an "any Linux" artifact.

## Build

```sh
# Release tarball -> dist/nocturne-<version>-x86_64-linux.tar.gz (+ .sha256)
scripts/package.sh tarball

# Install from the extracted tarball (PREFIX default /usr/local, DESTDIR optional)
sudo ./install.sh            # or: DESTDIR=/tmp/root PREFIX=/usr ./install.sh
sudo ./install.sh uninstall

# Arch package (set the real sha256 first; builds from the GitHub source archive of the v<version> tag)
cd packaging && makepkg -si
```

`packaging/PKGBUILD` sets `options=(!lto)`: makepkg's default C `-flto` breaks linking of aws-lc-sys's C objects.
Release profile: fat LTO, one codegen unit, stripped, `panic = "unwind"` (a panic in a worker thread must not abort
the UI). Clean build on 16 cores: about 4.5 minutes, about 24 MB binary (thin LTO: 2.3 minutes, 31 MB).

## Not shipped

AppImage: not provided. libmpv's closure is about 205 shared libraries (about 240 MB uncompressed) including GPL
codecs, the GPU stack (libEGL/libvulkan and driver ICDs) must come from the host and cannot be bundled, and a
binary built on a rolling glibc would not be self-contained on older systems. Use the tarball or the distribution
package, which take libmpv from the system.

## Vendored xattr

`vendor/xattr` is the crates.io `xattr` 0.2.3 with one line changed (`ENOATTR` is defined as `ENODATA`).
GPUI pulls it in through `zed-async-tar`, which requires the 0.2 series, and 0.2.3 is the last 0.2 release. It
uses `libc::ENOATTR`, which current `libc` (0.2.190 and later) no longer exports for Linux, so the unpatched crate does
not compile (checked in a clean copy without the `[patch.crates-io]` entry: `error[E0425]: cannot find value ENOATTR`).
Remove the patch and the directory once GPUI or `zed-async-tar` moves to a fixed `xattr`.

## Checks

`scripts/ci-checks.sh` runs fmt, clippy `-D warnings`, tests, `cargo deny` (`deny.toml`, if installed) and a secret
scan of tracked files and history. `scripts/gen-notices.py > THIRD_PARTY_NOTICES.md` regenerates the notices after
a dependency change. Replace the `SKIP` checksum in the PKGBUILD before a release.
