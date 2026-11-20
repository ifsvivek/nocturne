# Platforms, packaging and footprint

**Linux x86_64 under Wayland is the supported platform**, with signed pacman, apt and dnf repositories at `https://nocturne.ifsvivek.in/repo` and a tarball. Arch is the primary test target. Windows (x64) and macOS (Apple Silicon) are **unsigned previews** attached to the GitHub release; they have been tried on a real Windows PC and a Mac. X11, Homebrew packaging and Windows and macOS code signing are not planned. Nothing here may degrade playback quality, stability or security.

## Platform seams

Keep everything platform-specific behind these seams; `#[cfg(target_os = ...)]` appears only inside them.

Linux is built, run and tested. Windows and macOS have code behind each seam, compile checks (`scripts/check-cross.sh`) and native CI builds with a smoke test (`platforms.yml`, called by `release.yml` on a version tag; [PACKAGING.md](PACKAGING.md)) that produce the Windows setup.exe and zip and the macOS .dmg plus the `.tar.gz` the in-app updater installs. The whole app compiles for Windows (`x86_64-pc-windows-gnu`). On Linux only `nocturne-player` and `nocturne-local` can be checked for macOS: everything that depends on reqwest is blocked by `aws-lc-sys`, which compiles C against the Apple SDK headers, so those macOS paths are written but only built in CI and on a Mac.

| Concern | Linux | macOS | Windows |
|---|---|---|---|
| In-window video | libmpv GL render API in a Wayland subsurface. Linux is Wayland-only: X11 is not supported and the app exits with a message on an X11-only session. `crates/player/src/embedded.rs` | **external mpv window for now** (`embedded_stub.rs`: `EmbeddedPlayer` cannot be built, the app falls back). Native embedding: later (libmpv render API into a Metal/GL view) | same: external mpv window; native embedding later (libmpv render API / `wid` child window) |
| External player IPC | unix socket | unix socket | named pipe `\\.\pipe\nocturne-mpv-<pid>-<n>` (reader polls `PeekNamedPipe`, see `crates/player/src/ipc.rs`) |
| libmpv at runtime | distro package (dlopen `libmpv.so.2`) | dlopen `libmpv.2.dylib`: `Contents/Frameworks` next to the exe first, then default paths and Homebrew | dlopen `libmpv-2.dll` next to the exe first, then the DLL search path |
| external `mpv` binary | `PATH` | next to the app, `PATH`, Homebrew prefixes | `mpv.exe` next to the app, then `PATH` (no console window) |
| Credentials | Secret Service (`keyring`) | Keychain (`keyring` apple-native) | Credential Manager (`keyring` windows-native) |
| Media keys / now playing | MPRIS + idle inhibit (zbus, `core/player_mpris/dbus.rs`) | none yet (no-op; MPNowPlayingInfoCenter later) | none yet (no-op; SMTC later) |
| System package update | PackageKit over the system D-Bus (zbus, `core/update_pk.rs`) | none (per-user app update) | none (per-user update) |
| Signals / quit | SIGTERM/SIGINT | same | console ctrl handler (never fires in the GUI-subsystem build) |
| Single instance | `flock` + unix socket in `$XDG_RUNTIME_DIR` | same, socket in `$TMPDIR` | named mutex + named pipe |
| Paths | XDG | `~/Library/Application Support`, `~/Library/Caches` | `%APPDATA%` (config), `%LOCALAPPDATA%` (data, cache) |
| Local library identity | `(dev, inode)` + size/mtime | same | no stable file index in `std`: directories by canonical-path hash, files by size + content hash only |
| Window subsystem | n/a | n/a | `windows_subsystem = "windows"` in release builds |

One path helper: `nocturne_session::paths` (re-exported as `nocturne_core::paths`), pure `resolve(os, kind, env)` unit-tested for all three OSes. Code layout: `crates/player` keeps the per-OS IPC, the Windows child window (`surface.rs`) and libmpv loading; `platform.rs` and `single.rs` in the UI crate hold signals, window-handle extraction and the single-instance guard; `crates/session` picks the keyring backend by target.

Windows plays the picture in a child window of the app window (`VideoSurface`, `--wid`; see [video-embedding.md](video-embedding.md)) and falls back to mpv's own window. macOS plays in mpv's own window. Still open on both: in-window video on macOS, media keys and now playing, reduced-motion detection, and file picker and keyring behaviour checked on more machines. Because the previews are unsigned, SmartScreen and Gatekeeper warn on first start.

## Packages

| Format | Status | libmpv |
|---|---|---|
| Arch package | primary; signed pacman repository (`https://nocturne.ifsvivek.in/repo/arch`) and a PKGBUILD | `depends=(mpv)` |
| Release tarball + `install.sh` | shipping on the GitHub release; installs missing runtime libraries; updates itself from the app when its directory is writable | installer installs it via the distro package manager |
| `.deb` (Debian 13, Ubuntu 24.04+) | shipping; signed apt repository (`/repo/deb`); install-tested in clean containers | `Depends: libmpv2` (Ubuntu 24.04's 0.37 plays in an external mpv window) |
| `.rpm` (Fedora) | shipping; signed dnf repository (`/repo/rpm`); install-tested in a clean container | `Requires: mpv-libs` (RPM Fusion) |
| AppImage | experimental: a best-effort job in `release.yml`, not recommended or supported (large; libmpv and glibc conflicts, see PACKAGING.md) | bundled |
| Windows `.exe` setup + portable `.zip` | unsigned preview on the GitHub release; per-user install; built and smoke-tested in CI, tried on a real PC | bundles a pinned `mpv.exe` (drawn into a child window of the app, or its own window as the fallback) |
| macOS `.dmg` (Apple Silicon) + `.tar.gz` | unsigned preview on the GitHub release (ad-hoc signed bundle: right-click > Open); built and smoke-tested in CI, tried on a Mac; the `.tar.gz` is what the in-app updater installs | bundled libmpv + mpv (dylibbundler) |
| Flatpak | under evaluation | n/a |

Linux packages are built from one binary compiled in a `debian:stable` container. On a version tag `release.yml` builds every format on GitHub-hosted runners and drafts a release; the pacman, apt and dnf repositories and the update artifacts are signed on the maintainer's machine, never in CI ([PACKAGING.md](PACKAGING.md)). Verification is reported per target as built / installed / run / not tested.

Updates: package-managed installs update through the package manager; per-user installs (tarball, Windows installer, macOS app) update in the app with signed artifacts ([DECISIONS.md](DECISIONS.md)).

## Footprint budget and measurements

Track per release in `docs/dev/PERF.md` (script: `scripts/perf.sh`): release binary size, tarball and package size, installed size, cold start to first frame, idle RSS and idle CPU on the sign-in screen and on a loaded home page (demo library), RSS after browsing 500 items, CPU during 1080p playback (embedded, hwdec on). The Iced build must not regress against the recorded baseline without a written reason.

The theme engine adds about 2.8 MiB to the release binary (34.47 MiB without it, 37.27 MiB with it) and about 13 MiB of idle Home RSS, with no new run-time library and start and idle CPU unchanged within noise ([PERF.md](dev/PERF.md)). Build without it with `cargo build --no-default-features -p nocturne-app`; select the hand-written screens at run time with `NOCTURNE_ENGINE_UI=0`. The engine is pure Rust with no OS-specific code.

Rules: no polling where an event exists (frames subscription only while animating; timers only while a page needs them); one image cache (memory LRU over the disk LRU, decoded handles reused); lazy creation of clients, player and index; release profile stays `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "unwind"`.
