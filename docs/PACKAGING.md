# Packaging

Built and checked on Arch Linux, x86_64, Wayland (Hyprland), mpv 0.41.0, Rust 1.99. The dependency list below is the Iced build's (derived 2026-11-16 from `target/release/nocturne`). Other distributions and architectures: package names were checked to exist in the default repos of Debian stable, Fedora and openSUSE Tumbleweed (containers), nothing more. Debian/Fedora/openSUSE builds are UNVERIFIED.

**One variable names the app package.** `scripts/package.sh`, `scripts/perf.sh` and `packaging/PKGBUILD` default to package `nocturne-app` (`crates/app`) / binary `nocturne`; `scripts/gen-notices.py` and `scripts/test-playback.sh` take the same `PKG`/`BIN`). The installed name is always `nocturne`.

Linux formats: Arch package, tarball, **.deb** and **.rpm** (0.4, built in distro containers, see below). AppImage was evaluated and is not shipped (below). The Windows installer is built (wine-tested; first native run is the manual CI workflow) and the macOS script is written, first run also via that workflow (below). Per-platform libmpv handling and the footprint budget are in [PLATFORMS.md](PLATFORMS.md).

**libmpv version the app really needs: mpv 0.40 or newer.** The embedded player sets `background=color` (`crates/player/src/embedded.rs`), which mpv 0.35 and 0.37 reject (`mpv_set_option_string` returns -7, verified on Debian 12's 0.35.1 and Ubuntu 24.04's 0.37.0; accepted by Debian 13's 0.40.0 and Arch's 0.41). With an older libmpv the app does not crash: it logs `embedded unavailable (mpv: init: error setting option), using the external player` and plays through the external `mpv` binary (hence `mpv` is a Recommends in the .deb). A code change (set that option after init and ignore the error, or drop it when unsupported) would make in-window video work on Ubuntu 24.04; it is not part of the packaging.

## Install paths

| File | Path under `PREFIX` |
|---|---|
| binary | `bin/nocturne` |
| desktop entry | `share/applications/io.github.ifsvivek.nocturne.desktop` (`StartupWMClass=nocturne`, matching the window `app_id`) |
| AppStream | `share/metainfo/io.github.ifsvivek.nocturne.metainfo.xml` |
| icons | `share/icons/hicolor/{48x48,128x128,256x256,512x512}/apps/nocturne.png`, `scalable/apps/nocturne.svg` |
| licenses | `share/licenses/nocturne/{LICENSE,THIRD_PARTY_NOTICES.md}` |

## Runtime dependencies

How this was derived (all from the release binary, not from memory): `readelf -d` NEEDED shows only `libdbus-1.so.3`, `libEGL.so.1` (player's EGL glue), `libudev.so.1` (gamepad input, `gilrs`, since 0.4), `libgcc_s.so.1`, `libm.so.6`, `libc.so.6`; these three are load-time requirements (missing = loader error). Everything else is opened with `dlopen` (invisible to `readelf`/namcap): the names are visible in the binary (`libEGL.so.1`, `libwayland-client.so.0`, `libwayland-egl.so.1`, `libX11.so.6`, `libX11-xcb.so.1`, `libXcursor.so.1`, `libXi.so.6`, `libxkbcommon.so.0`, `libxkbcommon-x11.so.0`) or are assembled from immediates by the compiler (`libvulkan.so.1` from `ash::Entry::load`, `libxcb.so.1` from x11rb's `dl-libxcb`; found as the `libvulka`+`kan.so.1` / `libxcb.s`+`so.1` byte pairs) and confirmed in the crate sources under `~/.cargo/registry` (`ash-0.38 entry.rs`, `x11-dl`, `xkbcommon-dl`, `wayland-sys`, winit `wayland-dlopen`). libmpv: `crates/player/src/mpv_ffi.rs` (`SONAME = "libmpv.so.2"`, `libloading`), verified at run time in the container (`scripts/test-playback.sh`).

**Dropped with GPUI** (not in NEEDED, no string in the binary): `libfontconfig`, `libfreetype` (the Iced text stack, cosmic-text/fontdb, parses `fonts.conf` in pure Rust; `fontconfig-parser`), `libxcb` as a linked library (now dlopen, X11 sessions only). **Added:** `libX11`, `libX11-xcb`, `libXcursor`, `libXi` (winit's X11 backend, dlopen). The folder picker (`rfd`, `xdg-portal` feature, pure-Rust zbus) needs no GTK; it talks to `xdg-desktop-portal` at run time (optional). `libwayland-cursor` is not used (client-side decorations are drawn by sctk-adwaita).

A Vulkan driver (Mesa `vulkan-*` or NVIDIA) is also required (wgpu; `libEGL` is the GL fallback and the video path); it depends on the GPU, so the installer does not pick one. libmpv pulls in about 200 more libraries (ffmpeg, libass, ...) that the distribution's mpv package resolves. A Secret Service provider (gnome-keyring, KWallet, KeePassXC) stores the login. The UI font is "Noto Sans" by name (not bundled; any sans is the fallback).

Because libmpv is not linked, a missing `libmpv.so.2` does not stop the app from starting. `nocturne_player::libmpv_available()` reports it, and the app can fall back to the external `mpv` child-process player (needs the `mpv` binary, see `mpv_binary_available()`) or show `install_hint()`, a distro-specific command from `/etc/os-release`. libmpv is not needed at build time, so `mpv` is not a makedepend.

| Distribution | Packages (all needed at run time) | Status |
|---|---|---|
| Arch / CachyOS / Manjaro | `systemd-libs mpv libglvnd vulkan-icd-loader wayland libxkbcommon libxkbcommon-x11 libxcb libx11 libxcursor libxi dbus gcc-libs glibc` + `vulkan-driver`; optional `xdg-desktop-portal noto-fonts gnome-keyring` (PKGBUILD) | VERIFIED names (`pacman -Qo` on every soname); package built and launched in a clean container |
| Debian / Ubuntu | `libudev1 libmpv2 libegl1 libvulkan1 libwayland-client0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 libxcb1 libdbus-1-3 libx11-6 libx11-xcb1 libxcursor1 libxi6` + `mesa-vulkan-drivers \| vulkan-icd`; Recommends `mpv`; optional `xdg-desktop-portal fonts-noto-core` | VERIFIED: the .deb installs with `apt install ./nocturne_*.deb` in clean debian:stable (13), ubuntu:24.04 and ubuntu:latest (26.04); see "Linux packages" |
| Fedora | `systemd-libs mpv-libs libglvnd-egl vulkan-loader libwayland-client libwayland-egl libxkbcommon libxkbcommon-x11 libxcb dbus-libs libX11 libX11-xcb libXcursor libXi`; Recommends `mesa-vulkan-drivers mpv`; optional `xdg-desktop-portal google-noto-sans-fonts` | VERIFIED: the .rpm installs with `dnf install ./nocturne-*.rpm` in clean fedora:latest (45) with RPM Fusion free enabled (mpv-libs lives there) |
| openSUSE | `libudev1 libmpv2 mpv libglvnd libvulkan1 libwayland-client0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 libxcb1 libdbus-1-3 libX11-6 libX11-xcb1 libXcursor1 libXi6` + `libvulkan_intel`/`libvulkan_radeon`; optional `xdg-desktop-portal google-noto-sans-fonts` | names exist in Tumbleweed (zypper info); `libEGL.so.1` is provided by `libglvnd` (the old `libEGL1` name does not exist) |

How each artifact handles them:

- **Arch package (PKGBUILD)**: `depends` lists all of the above, including `mpv` for the dlopen'd libmpv.
- **Tarball `install.sh`**: on `install` it checks each soname with `ldconfig -p` and the `mpv` binary, installs the missing packages with the distro's package manager (via `sudo` unless root; the exact command is printed first), then runs `ldd` on the installed binary and warns about anything "not found". The soname-to-package table is the `pkg_for` function at the top of the script, the single place to edit. `--no-deps` skips this; `NOCTURNE_YES=1` makes the package manager non-interactive; an unrecognised distribution fails with the list of what is missing; `DESTDIR` (staging) skips dependency handling.
- **.deb / .rpm**: `Depends`/`Requires` list the dlopen()ed libraries by hand (the package tools only see the linked ones); libmpv comes from the distribution (`libmpv2` / `mpv-libs`). See "Linux packages".
- **AppImage**: not provided (see below).

The tarball binary is built on a rolling distribution and is not an "any Linux" artifact.

## Build

```sh
# Release tarball -> dist/nocturne-<version>-x86_64-linux.tar.gz (+ .sha256)
scripts/package.sh tarball

# Install from the extracted tarball (PREFIX default /usr/local, DESTDIR optional)
sudo ./install.sh            # installs missing runtime libraries first (--no-deps to skip); or: DESTDIR=/tmp/root PREFIX=/usr ./install.sh
sudo ./install.sh uninstall

# .deb (debian:stable container), .rpm (fedora:latest container), AppImage (debian:bookworm container, evaluation only):
scripts/package.sh deb        # -> dist/nocturne_<ver>_amd64.deb
scripts/package.sh rpm        # -> dist/nocturne-<ver>-1.fc<N>.x86_64.rpm
scripts/package.sh appimage   # -> dist/NOCTURNE-<ver>-x86_64.AppImage (not a release artifact, see below)

# Arch package (set the real sha256 first; builds from the GitHub source archive of the v<version> tag)
cd packaging && makepkg -si
```

`packaging/PKGBUILD` sets `options=(!lto)`: makepkg's default C `-flto` breaks linking of aws-lc-sys's C objects. Release profile: fat LTO, one codegen unit, stripped, `panic = "unwind"` (a panic in a worker thread must not abort the UI). Clean build on 16 cores: about 4.5 minutes, about 24 MB binary (thin LTO: 2.3 minutes, 31 MB).

## Pacman repository

Served from the website at `https://nocturne.ifsvivek.in/repo/arch/x86_64/` (`nocturne.db`, `nocturne.files`, the two newest `nocturne-*.pkg.tar.zst`, each with a `.sig`); `repo/ARCH-GPG-KEY` is the public key, `repo/index.txt` the instructions. The directory `website/static/repo/` is generated and git-ignored; it ships when the website is deployed. Package, database and files database are all signed, so users set `SigLevel = Required TrustedOnly` (setup commands in the README). Static hosting cannot serve symlinks, so `nocturne.db`/`nocturne.files` are real copies.

Signing key: "NOCTURNE Package Signing", fingerprint `5204B42AF768DE210D4979E397FA02EFCF1A4E3B`, ed25519, no passphrase. The public key is committed at `packaging/repo/ARCH-GPG-KEY`. The secret key exists only in the maintainer's GPG keyring: back it up offline (`gpg --export-secret-keys` to encrypted removable storage, never to the repository or the website), and keep the revocation certificate from `~/.gnupg/openpgp-revocs.d/` offline too. Rotating the key means a new fingerprint, a new `ARCH-GPG-KEY`, and users re-running the two `pacman-key` commands.

Release procedure:

1. Tag `vX.Y.Z`, set `pkgver` and the real `sha256sums` in `packaging/PKGBUILD` (sha256 of the GitHub tag archive).
2. `scripts/publish-repo.sh` builds the package in a clean `archlinux:base-devel` container (deps installed as root, `makepkg --nocheck` as a normal user, so it targets current Arch rather than the host), then on the host signs it, runs `repo-add --sign --remove`, prunes to the newest two versions and verifies every signature.
3. Deploy the website. The server never holds the key.

## Linux packages (.deb, .rpm)

Both are built **inside a container of the target distribution** (docker, no sudo, no hosted CI), so the binary links against that distribution's glibc; the working tree is mounted read-only. rustup installs the stable toolchain inside the container; the cargo registry and target directory live in a docker volume (`nocturne-deb-cache`, `nocturne-rpm-cache`, removed at the end unless `KEEP_CACHE=1`). A cold build takes about 7 minutes on 16 cores, a cached relink about 2. Build dependencies: a C toolchain, `pkg-config`, `libdbus-1-dev`/`dbus-devel`, `libudev-dev`/`systemd-devel` (the gamepad library `gilrs` links libudev) and `libegl-dev`/`libglvnd-devel` (the player links `libEGL` at build time, `#[link(name = "EGL")]`, so it is a linked dependency, not only a dlopen one; libmpv is not needed to build).

| | `scripts/package-deb.sh` | `scripts/package-rpm.sh` (`packaging/nocturne.spec`) |
|---|---|---|
| Build image | `debian:stable` (13 "trixie", glibc 2.41) | `fedora:latest` (45) |
| Artifact | `dist/nocturne_<ver>_amd64.deb`, 9.4 MB, installed 31.7 MB | `dist/nocturne-<ver>-1.fc45.x86_64.rpm`, 10.0 MB |
| glibc floor | `libc6 (>= 2.39)` from `dpkg-shlibdeps`: Debian 13, Ubuntu 24.04 and newer. Debian 12 / Ubuntu 22.04 are not supported by this build | the Fedora release it was built on or newer (rpm adds the `libc.so.6(GLIBC_x)` requires itself) |
| Linked libs (automatic) | `libc6 libdbus-1-3 libgcc-s1 libegl1` via `dpkg-shlibdeps` | rpm's soname requires |
| dlopen()ed libs (by hand) | `libmpv2 libvulkan1 libwayland-client0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 libx11-6 libx11-xcb1 libxcursor1 libxi6 libxcb1`, `mesa-vulkan-drivers \| vulkan-icd` | `mpv-libs vulkan-loader libwayland-client libwayland-egl libxkbcommon libxkbcommon-x11 libX11 libX11-xcb libXcursor libXi libxcb libglvnd-egl dbus-libs` |
| Recommends | `mpv` (external fallback), `gnome-keyring \| kwalletmanager \| keepassxc`, `xdg-desktop-portal`, `fonts-noto-core` | `mesa-vulkan-drivers mpv xdg-desktop-portal google-noto-sans-fonts gnome-keyring` |
| libmpv | distribution `libmpv2` (Debian 13: 0.40, Ubuntu 24.04: 0.37, Ubuntu 26.04: newer) | `mpv-libs` from RPM Fusion free (0.41 on Fedora 45); Fedora's own repositories have no mpv |

Contents of both: `/usr/bin/nocturne`, the desktop file, icons (48, 128, 256, 512 PNG and the SVG), AppStream metainfo, `LICENSE` and `THIRD_PARTY_NOTICES.md` (deb: `/usr/share/doc/nocturne/`, `copyright` is the GPL text; rpm: `/usr/share/licenses/nocturne/`). No maintainer scripts. The rpm builds no debuginfo package (the binary is already stripped by the release profile).

The GPU driver is a distribution choice, so it is soft: the .deb accepts any `vulkan-icd` provider, the .rpm only Recommends `mesa-vulkan-drivers` (Fedora has no virtual name), so NVIDIA users are not forced to pull Mesa.

Tested (2026-11-17, clean images, packages installed with `apt install ./x.deb` / `dnf install ./x.rpm`, nothing else added): the dependencies resolve; `ldd /usr/bin/nocturne` reports no missing library; every dlopen()ed soname is in `ldconfig -p`; started without a display, the app reaches the windowing stage and stops with winit's "neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set" panic, not a loader error (a panic instead of a friendly message is the known polish item).

| Image | Package | Result |
|---|---|---|
| debian:stable (13, glibc 2.41) | deb | installs, ldd clean, reaches the no-display stage |
| ubuntu:latest (26.04, glibc 2.43) | deb | same |
| ubuntu:24.04 (glibc 2.39) | deb | same; its libmpv2 is 0.37, so video uses the external `mpv` (see the 0.40 note above) |
| fedora:latest (45, glibc 2.44) + RPM Fusion free | rpm | installs, ldd clean, reaches the no-display stage |

Not tested: a window or playback inside these containers (the Arch gate `scripts/test-playback.sh` does that for the Arch package), other Fedora releases, RHEL/Alma (no RPM Fusion mpv), openSUSE.

## Windows installer (0.4, built; wine-tested only)

`scripts/package-windows.sh` (Docker only) cross-compiles `x86_64-pc-windows-gnu`, downloads the pinned mpv, and runs `makensis` (`packaging/windows/nocturne.nsi`). Output: `dist/nocturne-<ver>-x86_64-setup.exe` (about 40 MiB) and `nocturne-<ver>-x86_64-windows.zip` (portable, about 56 MiB; unzip anywhere and run `nocturne.exe`). `nocturne.exe` alone is 26 MiB.

- **Installer**: per-user (no administrator prompt) into `%LOCALAPPDATA%\Programs\NOCTURNE`, Start menu shortcut, uninstaller (Apps and Features entry under HKCU), licence page (GPL-3.0 text followed by the bundled-library notice), app icon (`nocturne.ico` made from `packaging/icons/nocturne-256.png` with ImageMagick). The uninstaller leaves settings, the library index and the stored login (Credential Manager) alone. Silent: `setup.exe /S`, `/D=<dir>` to choose the directory; `uninstall.exe /S`.
- **Bundles `nocturne.exe`, `mpv.exe`, notices, no `libmpv-2.dll`.** The in-window video path is Linux (Wayland) only, so on Windows playback always runs the external `mpv.exe` (`crates/player/src/deps.rs` looks next to the exe first). The libmpv DLL would be 120 MB of dead weight; when Windows in-window video exists, take `libmpv-2.dll` from the `mpv-dev-*` archive of the same pinned release (its hash is already in the pin file).
- **mpv provenance**: `packaging/windows/mpv.pin` pins shinchiro/mpv-winbuild-cmake release `20261111` (`mpv-x86_64-20261111-git-bcde6b93cc.7z`, mpv v0.42.0-38-gbcde6b93c, FFmpeg N-128766-gcd4e96eb3), the Windows build mpv.io's installation page points to. The script refuses to continue unless the archive's SHA256 is `f259b5fd4b5c395617c948a4fadcdf55bba1c5282fb7c13e88626b3f71072baf` (equal to the digest GitHub shows for the asset). The generic x86-64 build is used, not the `-v3` one. Upstream may prune old releases: keep a copy of the archive with the release artifacts, because the source offer below names that exact build.
- **Licence**: the build contains libx264 (so FFmpeg is `--enable-gpl`) and is distributed under the GNU GPL; NOCTURNE is GPL-3.0-or-later and starts `mpv.exe` as a separate process, so there is no licence conflict. Distributing the binary obliges us to offer the corresponding source: `THIRD_PARTY_NOTICES.md` (section "Bundled native libraries", generated by `scripts/gen-notices.py` from the pin file) and the installer's licence page name the mpv and FFmpeg commits and the build recipe repository (which pins every other library). Before a release, also host or link the matching source tarballs if the maintainer wants a durable offer.
- **Unsigned.** SmartScreen shows "unknown publisher" until a code-signing certificate is bought; signing is not scripted.
- **Tested** (`docs/QA_REPORT.md`, section "0.4 Windows installer"): built; installed, launched and uninstalled under **wine in a container, which is not Windows**. Never run on real Windows: SmartScreen, the shortcut, the Credential Manager login, the GPU (wgpu DX12/Vulkan), mpv.exe playback and the real uninstall are untested.
- The version in the file name is the workspace version (`Cargo.toml`, still 0.3.0 at the time of writing).

## macOS .dmg (0.4, written, not built)

`scripts/package-macos.sh` + `packaging/macos/{Info.plist,entitlements.plist}`. It exits at once on anything but macOS and has **never run**: there was no Mac, and the macOS app cannot even be cross-checked on Linux (`aws-lc-sys`, see PLATFORMS.md). Expect to fix small things on the first run. It builds one `.dmg` per architecture:

- `NOCTURNE.app` with `Contents/MacOS/nocturne` and the Homebrew `mpv`, `Contents/Frameworks/libmpv.2.dylib` plus every dylib they need (copied and re-pointed to `@executable_path/../Frameworks` by `dylibbundler`; the script fails if any `/opt/homebrew` or `/usr/local` path remains), an `.icns` made with `sips` + `iconutil`, bundle id `io.github.ifsvivek.nocturne`. The player's search order already looks in `Contents/Frameworks` and next to the exe.
- **Licence**: Homebrew's mpv is GPL-2.0-or-later as built, with many differently licensed dylibs. The same source-offer duty as Windows applies; the script writes `BUNDLED_LIBRARIES.txt` into the bundle, but the exact versions and a notices section for macOS still have to be added to `gen-notices.py` from a real build.
- **Optional, needs the maintainer's Apple Developer ID** (not scripted away): `SIGN_ID="Developer ID Application: ..."` signs dylibs, mpv, the bundle (hardened runtime) and the dmg; `NOTARY_PROFILE=<notarytool profile>` notarises and staples. Without them the bundle is ad-hoc signed and Gatekeeper requires right-click > Open.
- Homebrew dylibs target the Homebrew machine's macOS version; `LSMinimumSystemVersion` is 12.0 as a guess, to be set from `otool -l` (`minos`) of the bundled dylibs.

## CI build of the Windows and macOS packages

`.github/workflows/platforms.yml` is `workflow_dispatch` only and is meant to be **run manually once per release** (there is no push/PR/schedule trigger; `ci.yml` is the separate manual Linux check). No secrets, no publishing: artifacts (`windows`: setup.exe, portable zip, `.sha256` files; `macos`: .dmg and `.sha256`) are kept 7 days and the maintainer attaches them to a release by hand.

- **windows** (`windows-latest`): native `x86_64-pc-windows-msvc` release build (the Docker script cross-compiles `-gnu`; CI uses MSVC because the runner has it), pinned mpv fetched and SHA256-verified from `mpv.pin`, `makensis` installer and portable zip with the same layout as `scripts/package-windows.sh` (icon via Pillow instead of ImageMagick), then a smoke test: silent install, `mpv.exe --version`, launch `nocturne.exe` (a GPU/window exit is fine, a loader error code is not), silent uninstall with file and registry checks. Unsigned.
- **macos** (`macos-latest`, arm64 only): Homebrew `mpv` + `dylibbundler`, `scripts/package-macos.sh` (ad-hoc signed, plain `hdiutil` dmg because `create-dmg` needs Finder), then `otool -L` over the bundle (no `/opt/homebrew` or `/usr/local`), bundled `mpv --version`, libmpv `dlopen`, brief launch (dyld errors fail, GPU errors do not).
- `cargo test --workspace --locked` runs last in each job so a test failure cannot hide the packaging result.

## AppImage: evaluated, not shipped

`scripts/package-appimage.sh` builds it (debian:bookworm container, hand-assembled AppDir plus `appimagetool`, which is downloaded unverified at build time) so the experiment is reproducible. It bundles the app and the `libmpv.so.2` closure (201 libraries: ffmpeg, libass, libplacebo, ...) and leaves glibc, libstdc++, the libGL/libEGL/libvulkan loaders, every GPU driver, libdrm/libgbm, libwayland-*, libX11, libxcb, libva/libvdpau and libxkbcommon to the host. `PROFILE=debug` makes a debug-binary variant (the player preview exists only in debug builds; testing only).

Measured (2026-11-17): **107 MB** release AppImage (squashfs), 263 MB unpacked, against 9.4 MB for the .deb, which uses the distribution's libmpv. It starts on the Arch host.

Findings:

1. **No base is both glibc-old and libmpv-new.** The app needs libmpv >= 0.40 for in-window video (see above). Bookworm (glibc 2.36) ships 0.35: the bookworm AppImage on the Arch host logs `embedded unavailable (mpv: init: error setting option), using the external player` and uses the host `mpv` instead of its bundled libmpv. A base with mpv 0.40 is trixie (glibc 2.41); its AppImage would run only on glibc >= 2.41 hosts, the ones that already have native packages. The portability argument disappears.
2. **Decoder hardware acceleration through the bundled libmpv works**: on the host (NVIDIA RTX 3060 + AMD Vega) the bundled mpv 0.35.1 reports `hwdec-current = vaapi-copy` and `nvdec-copy` for a 1080p H.264 clip with `vo=null`, like the host libmpv 0.41 (host libva and NVDEC are found). In-window rendering (render API + EGL, the app's real path) was tested with a trixie-based debug AppImage (bundled libmpv 0.40, `IMG=debian:stable PROFILE=debug`, player preview on a 1080p clip, `scripts/run-isolated.sh`): it logs `hwdec-current nvdec` on the NVIDIA GPU. The AMD iGPU path was not run.
3. **The "from the host" libraries are a real requirement.** In a clean ubuntu:24.04 image the bundled libmpv has about 20 unresolved libraries (`libva*`, `libvdpau`, `libXv`, `libXrandr`, `libXss`, `libXinerama`, `libXpresent`, `libwayland-cursor`, `libxcb-shape`, `libGL`, `libdrm`, `libgbm`); the app itself needs `libEGL` before it starts at all. A desktop install has them, a minimal one does not, and libmpv then fails to load. The script bundles the X11 extension libraries after this finding and keeps libva/libvdpau on the host on purpose (a bundled old libva can reject a newer host VA-API driver).
4. **`LD_LIBRARY_PATH` leaks.** `AppRun` has to put the bundled libraries on `LD_LIBRARY_PATH` because the app opens `libmpv.so.2` by soname; the external `mpv` fallback and any child process inherit it and may load the bundled ffmpeg instead of the host's. A code change (an explicit libmpv path, e.g. `NOCTURNE_LIBMPV`) would avoid it.
5. The bundled `libfontconfig` (Debian 12) warns about the host's newer `fonts.conf` (`invalid constant used : math`): harmless, typical of mixing bundled and host data.

**Recommendation: do not ship an AppImage in 0.4.** Cost: about 100 MB per release plus a downloaded build tool; benefit: nothing the .deb, .rpm, Arch package and tarball do not give, because the app needs a libmpv newer than any glibc-old distribution carries and the GPU stack must come from the host anyway. Revisit if the libmpv requirement is relaxed and the maintainer wants a one-file download for distributions without a native package; a Flatpak (the runtime supplies mpv-compatible ffmpeg and the GL/Vulkan stack) is the better "any distribution" format and is the one to evaluate next.

## Checks

`scripts/test-playback.sh` is the clean-container release gate (package build, install, run on the host Wayland session with software rendering, headless libmpv clip); what it does and does not prove is in its header and in
[QA_REPORT.md](QA_REPORT.md). `scripts/ci-checks.sh` runs fmt, clippy `-D warnings`, tests, `cargo deny` (`deny.toml`, if installed) and a secret
scan of tracked files and history. `scripts/gen-notices.py > THIRD_PARTY_NOTICES.md` regenerates the notices after a dependency change. Replace the `SKIP` checksum in the PKGBUILD before a release.
