# Platforms, packaging and footprint

Status: plan accepted 2026-11-16. **Arch Linux first**; other Linux formats next; macOS and Windows after the Iced
migration and the multi-source work ship on Linux. Today only Arch is built and tested. The other targets are scheduled for 0.4 (maintainer decision 2026-11-16),
after 0.3 ships for Arch. Nothing here may degrade playback quality, stability or security.

## Platform seams (keep everything platform-specific behind these; `#[cfg(target_os = ...)]` only inside them)

Implemented state (0.4): Linux is built, run and tested. macOS and Windows are **written, cross-checked to compile,
never run** (`scripts/check-cross.sh`); no installers yet.

| Concern | Linux | macOS | Windows |
|---|---|---|---|
| In-window video | libmpv GL render API in a Wayland subsurface (X11: external mpv). `crates/player/src/embedded.rs` | **external mpv window for now** (`embedded_stub.rs`: `EmbeddedPlayer` cannot be built, the app falls back). Native embedding: later (libmpv render API into a Metal/GL view) | same: external mpv window; native embedding later (libmpv render API / `wid` child window) |
| External player IPC | unix socket | unix socket | named pipe `\\.\pipe\nocturne-mpv-<pid>-<n>` (reader polls `PeekNamedPipe`, see `crates/player/src/ipc.rs`) |
| libmpv at runtime | distro package (dlopen `libmpv.so.2`) | dlopen `libmpv.2.dylib`: `Contents/Frameworks` next to the exe first, then default paths and Homebrew | dlopen `libmpv-2.dll` next to the exe first, then the DLL search path |
| external `mpv` binary | `PATH` | next to the app, `PATH`, Homebrew prefixes | `mpv.exe` next to the app, then `PATH` (no console window) |
| Credentials | Secret Service (`keyring`) | Keychain (`keyring` apple-native) | Credential Manager (`keyring` windows-native) |
| Media keys / now playing | MPRIS + idle inhibit (zbus, `core/player_mpris/dbus.rs`) | none yet (no-op; MPNowPlayingInfoCenter later) | none yet (no-op; SMTC later) |
| Signals / quit | SIGTERM/SIGINT | same | console ctrl handler (never fires in the GUI-subsystem build) |
| Single instance | `flock` + unix socket in `$XDG_RUNTIME_DIR` | same, socket in `$TMPDIR` | named mutex + named pipe |
| Paths | XDG | `~/Library/Application Support`, `~/Library/Caches` | `%APPDATA%` (config), `%LOCALAPPDATA%` (data, cache) |
| Local library identity | `(dev, inode)` + size/mtime | same | no stable file index in `std`: directories by canonical-path hash, files by size + content hash only |
| Window subsystem | n/a | n/a | `windows_subsystem = "windows"` in release builds |

One path helper: `nocturne_session::paths` (re-exported as `nocturne_core::paths`), pure `resolve(os, kind, env)`
unit-tested for all three OSes. Code layout: `crates/player` keeps the per-OS IPC and libmpv loading; `platform.rs` and
`single.rs` in the UI crate hold signals, window-handle extraction and the single-instance guard; `crates/session` picks
the keyring backend by target.

Remaining for macOS/Windows: installers (.dmg/.msi), bundling libmpv, native in-window video, media keys / now playing,
reduced-motion detection, file picker and keyring behaviour verified on real machines, code signing.

## Packages

| Format | Status | libmpv |
|---|---|---|
| Arch PKGBUILD (AUR-ready) | primary, shipping | `depends=(mpv)` |
| Release tarball + `install.sh` | shipping | installer installs it via the distro package manager |
| `.deb` (Debian/Ubuntu) | planned | `Depends: libmpv2` (verify per release) |
| `.rpm` (Fedora/RHEL) | planned | `Requires: mpv-libs` (verify repo: Fedora vs RPM Fusion) |
| AppImage | planned, evaluate | bundling libmpv + ffmpeg is possible but large and fragile with hw decoding (VA-API/NVDEC driver ABI); never bundle GPU drivers/libGL/libvulkan. Decide after a size and hwdec test. |
| macOS `.dmg` | later | bundled, signed/notarised if the maintainer has an Apple Developer ID |
| Windows `.msi`/`.exe` | later | bundled DLL; code signing needs a certificate |

Builds run locally (maintainer decision 2026-11-16: no hosted CI runs): Linux formats in Docker containers of each
distribution; Windows as a cross-compile check only; macOS cannot be built on Linux and needs a real Mac. Verification
is reported per target as built / installed / run / not tested.

## Footprint budget and measurements

Track per release in `docs/dev/PERF.md` (script: `scripts/perf.sh`, to be added): release binary size, tarball and
package size, installed size, cold start to first frame, idle RSS and idle CPU on the sign-in screen and on a
loaded home page (demo library), RSS after browsing 500 items, CPU during 1080p playback (embedded, hwdec on).
Baseline = 0.2.0 (the earlier GPUI interface) measured before the cut-over; the Iced build must not regress without a written reason.

Rules: no polling where an event exists (frames subscription only while animating; timers only while a page needs
them); one image cache (memory LRU over the disk LRU, decoded handles reused); lazy creation of clients, player and
index; release profile stays `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "unwind"`.
