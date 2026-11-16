# Platforms, packaging and footprint

Status: plan accepted 2026-11-16. **Arch Linux first**; other Linux formats next; macOS and Windows after the Iced
migration and the multi-source work ship on Linux. Today only Arch is built and tested. The other targets are scheduled for 0.4 (maintainer decision 2026-11-16),
after 0.3 ships for Arch. Nothing here may degrade playback quality, stability or security.

## Platform seams (keep everything platform-specific behind these; `#[cfg(target_os = ...)]` only inside them)

| Concern | Linux | macOS | Windows |
|---|---|---|---|
| In-window video | libmpv GL render API in a Wayland subsurface (X11: external mpv) | to investigate: libmpv render API into an offscreen GL/Metal target, or external mpv | to investigate: libmpv `wid` child window / render API, or external mpv |
| External player IPC | unix socket | unix socket | named pipe |
| libmpv at runtime | distro package (dlopen `libmpv.so.2`) | bundled `libmpv.2.dylib` in the .app | bundled `libmpv-2.dll` next to the exe |
| Credentials | Secret Service (`keyring`) | Keychain (`keyring` apple-native) | Credential Manager (`keyring` windows-native) |
| Media keys / now playing | MPRIS (zbus) | MPNowPlayingInfoCenter (later) | SMTC (later) |
| Signals / quit | SIGTERM/SIGINT | same | console ctrl handler |
| Paths | XDG | `~/Library/...` (`dirs`-style resolution) | `%APPDATA%` / `%LOCALAPPDATA%` |

Code layout: `crates/player` keeps the per-OS video/IPC backends; a `platform` module in the UI crate holds
signals, window-handle extraction and media-key integration; `crates/session` picks the keyring backend by target.

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
