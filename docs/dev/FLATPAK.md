# Flatpak evaluation (0.6.0)

Research only: nothing here ships. Facts about NOCTURNE come from this repository; facts about Flatpak and Flathub are from the maintainers' public documentation as known at the time of writing and must be re-checked against the Flathub submission guide and the current runtime release notes before a real attempt (marked "verify" where I am less sure). Nothing was built or run in a sandbox for this report.

## Recommendation

Worth doing, after 0.6.0, as a Flathub app with a deliberately small sandbox hole list: Wayland, GPU, network, audio, the Secret Service name and `xdg-videos` read-only. In-window playback only (bundled libmpv and mpv, no host players), the in-app updater and PackageKit "Update now" switched off, yt-dlp bundled. Effort: about 3 to 5 working days to a first working local build (manifest, cargo sources, mpv module, the `InstallKind::Flatpak` change, a QA pass on an Arch/GNOME and a KDE session with Intel/AMD and NVIDIA), plus 1 to 3 weeks of calendar time for Flathub review. Ongoing cost: each release needs a manifest bump (automatable with Flathub's bot), the mpv and yt-dlp modules need version bumps, and bug reports will arrive tagged "Flatpak".

Why it is worth it: it is the "any distribution" format PACKAGING.md already names as the better successor to the AppImage; the runtime supplies Mesa (EGL and Vulkan), Wayland libraries and glibc, so the "libmpv newer than the distribution carries" problem disappears; updates come from Flathub with no signing infrastructure of ours. Why it is not free: libmpv is not in the runtime and must be built as a module; the sandbox changes where settings and data live; and the app has three features that talk to the host (external players, PackageKit, the self-updater) that must be switched off or redesigned.

## Manifest sketch

Application id: `io.github.ifsvivek.nocturne` (already the id of `packaging/io.github.ifsvivek.nocturne.desktop` and the metainfo; Flathub accepts the `io.github.<user>` form for a repository the user owns).

```yaml
app-id: io.github.ifsvivek.nocturne
runtime: org.freedesktop.Platform
runtime-version: '25.08'          # the current stable release at submission time; verify
sdk: org.freedesktop.Sdk
sdk-extensions: [org.freedesktop.Sdk.Extension.rust-stable]
add-extensions:
  org.freedesktop.Platform.ffmpeg-full:     # H.264/HEVC/AAC decoders that the plain runtime ffmpeg leaves out
    directory: lib/ffmpeg
    version: '25.08'
    add-ld-path: .
    autodownload: true
    autodelete: false
command: nocturne
finish-args:
  - --share=network
  - --share=ipc                   # X11 shared memory; harmless on Wayland, needed by the X11 fallback only
  - --socket=wayland
  - --socket=pulseaudio           # PipeWire's pulse socket
  - --device=dri                  # EGL (embedded video) and Vulkan (the UI)
  - --device=input                # gamepads (gilrs); needs Flatpak 1.15.6+, else drop the gamepad
  - --talk-name=org.freedesktop.secrets
  - --filesystem=xdg-videos:ro
  - --env=NOCTURNE_FLATPAK=1      # or read /.flatpak-info; see "Updates"
modules:
  - name: libplacebo               # mpv's renderer; see "libmpv in the sandbox"
    ...
  - name: mpv                      # libmpv.so.2 plus the mpv binary, built with -Dlibmpv=true -Dgpl=true
    ...
  - name: yt-dlp
    ...
  - name: nocturne
    buildsystem: simple
    build-options:
      append-path: /usr/lib/sdk/rust-stable/bin
      env: { CARGO_HOME: /run/build/nocturne/cargo }
    build-commands:
      - cargo --offline fetch --manifest-path Cargo.toml --verbose
      - cargo --offline build --release --locked -p nocturne-app --features ...
      - install -Dm755 target/release/nocturne /app/bin/nocturne
      - install -Dm644 packaging/io.github.ifsvivek.nocturne.desktop /app/share/applications/io.github.ifsvivek.nocturne.desktop
      - install -Dm644 packaging/io.github.ifsvivek.nocturne.metainfo.xml /app/share/metainfo/io.github.ifsvivek.nocturne.metainfo.xml
      - install -Dm644 packaging/nocturne.svg /app/share/icons/hicolor/scalable/apps/io.github.ifsvivek.nocturne.svg
    sources:
      - type: git            # a tagged release, pinned by commit, so Flathub's bot can bump it
        url: https://github.com/ifsvivek/nocturne.git
        tag: v0.6.0
        commit: ...
      - cargo-sources.json   # generated, see below
```

## Build: Rust offline

Flathub builds have no network access after sources are fetched. The standard route is `flatpak-cargo-generator.py` (from the `flatpak-builder-tools` repository): run it on `Cargo.lock` to produce `cargo-sources.json`, which lists every crate as an archive source with its checksum, and `cargo --offline build --locked` uses the vendored copy. NOCTURNE already builds with `--locked`; there are no git dependencies to special-case except the `wayland-*` `=` pins (crates.io, fine). The icon in the metainfo is `nocturne` today; Flatpak requires the file name to match the app id, hence the `install -D` lines above. Build time is the 4 to 6 minute figure in PACKAGING.md plus module builds; Flathub's builders have enough memory for fat LTO but use `fast-release` or a custom profile if the buildbot times out (verify). `aws-lc-sys` needs a C compiler and CMake in the SDK; both are there.

## Wayland and GPU

`--socket=wayland` and `--device=dri` are enough for the in-window player. EGL: the embedded player creates a `wl_subsurface` from the window's raw `wl_display` and `wl_surface` and renders with libmpv's OpenGL render API through EGL. Inside the sandbox that EGL comes from the runtime's Mesa; the compositor socket is the same one the UI uses, so subsurfaces work as on the host (to be proven in a test run: this is the one place a surprise is plausible, because the app hands pointers to libwayland across the dlopen boundary and the `wayland-*` crates are pinned for exactly this reason). The "never create an mpv Vulkan context in the app process" rule still holds. NVIDIA proprietary drivers reach the sandbox through the `org.freedesktop.Platform.GL.nvidia-*` extension, which Flatpak installs to match the host driver: the earlier segfault story (two VkInstances) is a driver matter, so the NVIDIA QA pass must be repeated inside the sandbox. X11 sessions: add `--socket=fallback-x11` only if the external-mpv fallback needs a window; today the app uses the external `mpv` child on non-Wayland sessions, and a sandboxed child process opening an X11 window needs that socket. Cheap, and Flathub accepts `fallback-x11` with `--socket=wayland`.

## Keyring

`nocturne-session` uses the `keyring` crate's Secret Service backend (D-Bus `org.freedesktop.secrets`), with no file fallback by design. In a sandbox the session bus is filtered, so `--talk-name=org.freedesktop.secrets` is required and keeps the behaviour identical: tokens end up in GNOME Keyring or KWallet's Secret Service. The alternative, the Secret portal (`org.freedesktop.portal.Secrets`), does not store items: it hands the app a per-app master secret, which then protects a file the app keeps. That is exactly the file fallback the project refuses, and it would need a different backend. Flathub accepts the `talk-name` on a short justification (the keyring is the standard place for a login token). Existing users who move from a native package start empty: the token is not copied, they sign in again; the sandbox does not read `~/.config/nocturne` (it uses `~/.var/app/io.github.ifsvivek.nocturne/{config,data,cache}`), so settings do not carry over either. Document it rather than request `--filesystem=xdg-config/nocturne`.

## Local media folders

Two options:

- `--filesystem=xdg-videos:ro` gives read-only access to `~/Videos` with no prompt, which suits the scanner (it never writes to media folders anyway) and the "Add a folder" flow for the default location. It is a normal, accepted permission. Music and Pictures are separate (`xdg-music:ro`, `xdg-pictures:ro`) and would be needed for the audio and photo libraries; the likely set is all three read-only. `--filesystem=home` or `host` will be refused or questioned at review.
- The file chooser: `rfd` is built with the `xdg-portal` feature, so Choose folder already goes through the portal. In a sandbox the portal returns a path under `/run/user/<uid>/doc/<id>/<name>`, mounted through the document portal for this app only, and the choice persists across restarts (the portal remembers the grant). Plain `read_dir` and mpv's file access work on those paths. Two catches to test: the scanner's change watching (inotify) on a FUSE-backed document-portal mount may not deliver events, and `sources.toml` would store the portal path, so a folder chosen while the app was a native build has to be added again. A path outside the three XDG directories on another disk therefore works only through the portal, with no automatic access and no permission list to tell the user about.

Recommended: ship the three XDG `:ro` permissions and rely on the portal for everything else. Save to library (yt-dlp downloads) needs a writable folder: that one must come from the portal (a folder picked with Choose folder is read-write), so the downloads queue should ask for its folder through the portal instead of assuming `~/Videos`.

## libmpv in the sandbox

The freedesktop runtime has FFmpeg but no mpv and no libplacebo. The design needs both `libmpv.so.2` (dlopened from `/app/lib`; `deps.rs` already probes the usual library names and `LD_LIBRARY_PATH` is set by the sandbox, so the probe finds it) and the `mpv` binary (the external fallback and the "mpv window" player choice). Build mpv as a module with `-Dlibmpv=true` and a GPL build (`-Dgpl=true`, the default; NOCTURNE is GPL-3.0-or-later and mpv is GPL-2.0-or-later or LGPL-2.1-or-later when built with `-Dgpl=false`, both compatible). Dependencies to add as modules or take from the runtime: libplacebo (module), libass (runtime has it; verify), fribidi, harfbuzz, uchardet, lua or luajit (optional; mpv without scripting is fine and smaller), shaderc or glslang for libplacebo's shader compiler (a module; the largest piece). Flathub already carries working mpv manifests (`io.mpv.Mpv` and several players that bundle libmpv); start from those rather than from scratch, and take FFmpeg from the runtime plus `org.freedesktop.Platform.ffmpeg-full` for the codecs patents keep out of the base runtime (the extension is the supported path; Flathub does not allow bundling a codec-complete FFmpeg yourself). Size cost: about 10 to 20 MB installed for mpv, libplacebo and shaderc (estimate). Licence file: add mpv's and libplacebo's to the installed licences (Flathub expects them under `/app/share/licenses`; the build helpers do that).

## External players (VLC, mpv window)

Player choice offers VLC and an mpv window "only when installed". Inside the sandbox `vlc` is not found, so the VLC choice hides itself, and the "mpv window" would run the bundled mpv, which is a sandboxed child with the same GPU access: it works and costs nothing. Reaching a host VLC would need `flatpak-spawn --host`, which requires `--talk-name=org.freedesktop.Flatpak`: that permission lets the app run any command on the host outside the sandbox, which defeats the sandbox and which Flathub reviewers reject for ordinary apps. Do not do it. Recommendation: in-window and the bundled "mpv window" only; say in the Playback setting's help that VLC is not available in the Flatpak (a sandboxed VLC would be the `org.videolan.VLC` Flatpak, and nothing links the two without the host-spawn hole).

## yt-dlp

Not in the runtime, and not a Flathub extension. Options: bundle it as a module, or ask the user to install it on the host (invisible from the sandbox, so not an option). Bundle: yt-dlp is a Python program, the runtime has Python 3, and its release "binary" is a zipapp; a module that installs the pinned release file (checksum in the manifest, version bumped by Flathub's `x-checker-data` bot) is about 3 MB and licensed Unlicense. Sites change often, so a bundled yt-dlp goes stale between NOCTURNE releases: plan to bump the module on its own cadence, and show the version in the playback error ("update the Flatpak"). It also needs `ffmpeg`/`ffprobe` for merging formats and for saving files: the runtime ships them (verify the ffmpeg-full extension exposes the binaries on `PATH`, or call `/usr/bin/ffmpeg`). Recent yt-dlp releases want an external JavaScript runtime (Deno or Node) for some sites, YouTube first (verify the current requirement): not in the runtime; adding Deno costs tens of MB, so ship without and say which sites need it. The feature's "never bundled" rule in the 0.6 plan was about the release tarball and distribution packages (yt-dlp optional, the system's own); a Flatpak cannot use the system's, so bundling is the Flatpak exception and should be a decision of the maintainer's, taken with the "save only what you have the right to keep" note in the downloads UI.

## Updates

- Flathub updates the app (`flatpak update`, GNOME Software, Discover). The in-app self-updater and PackageKit "Update now" must be off: PackageKit is not reachable (no talk-name for it, and it would update the host's packages, not the Flatpak), and the updater's `detect()` would see `/app/bin/nocturne`, call it `PerUser`, find the directory read-only and open the release page, which is the wrong advice. Add `InstallKind::Flatpak`, chosen when `/.flatpak-info` exists (or `FLATPAK_ID` is set), that never checks and shows "Updates come from Flathub" in Settings > Updates, next to `Dev`. A small, testable change in `crates/core/src/update.rs`.
- The AppStream metainfo needs `<release>` entries for each version (the 0.5 one already has a version history; keep it in step with CHANGELOG).
- The Windows/macOS preview notes in the metainfo do not matter for Flathub.

## Size

Estimates, not measurements. Per app: the binary is about 24 MB (release) in PACKAGING.md's figures, mpv and its libraries 10 to 20 MB, yt-dlp 3 MB, assets and icons small: roughly 40 to 55 MB installed, about 15 to 20 MB to download (Flatpak compresses with deltas). The runtime (`org.freedesktop.Platform`, a few hundred MB, shared with every other Flatpak) and the ffmpeg extension are downloaded once if not already present; a user who has no other Flatpak pays that on first install. The NVIDIA GL extension, if needed, is another 300 MB or so on the user's side. Compare the AppImage at 107 MB and the .deb at 9.4 MB.

## Flathub submission steps and policies

1. Fork `flathub/flathub`, create a branch from `new-pr`, add the manifest `io.github.ifsvivek.nocturne.yml`, `cargo-sources.json` and any patches, open a pull request to the `new-pr` branch titled `Add io.github.ifsvivek.nocturne`. Run `flatpak-builder-lint` locally first; the reviewers run it.
2. Requirements the reviewers check: a valid AppStream file (`appstreamcli validate`, with a summary, description, screenshots, an OARS content rating, `<release>` entries, a licence `GPL-3.0-or-later`, project URLs, and `developer_name`); a `.desktop` file and icon named after the app id; build from source with no network at build time (that is the cargo sources file) and from tagged, pinned sources; permissions justified in the PR (each `finish-args` line above, especially `talk-name=org.freedesktop.secrets` and the three `xdg-*:ro` entries); no `--filesystem=host` or `home`, no `--talk-name=org.freedesktop.Flatpak`, no `--device=all` if `--device=dri` and `input` do; the app must be what it says (it is not endorsed by Jellyfin: the metainfo must keep "not affiliated"). Screenshots must be real and in a neutral setting: the website's demo-library captures qualify.
3. Verification of the app id: for `io.github.ifsvivek.*` Flathub verifies through the GitHub account (a login-provider flow on the Flathub developer page); a domain-based id such as `in.ifsvivek.nocturne` would need a file on the domain. The `github.io` id already in the repository is the simple route.
4. After the merge a repository `flathub/io.github.ifsvivek.nocturne` is created, with the maintainer as owner; the buildbot builds the stable branch; new releases are a pull request there (opened by Flathub's bot when `x-checker-data` is present on the git source).
5. Policies to keep in mind: the licence of every bundled component must be redistributable (mpv GPL build with NOCTURNE GPL-3.0-or-later is fine, yt-dlp Unlicense, check libplacebo LGPL-2.1+); no proprietary codecs outside the runtime's extension; telemetry and update checks must be honest: NOCTURNE's update check (website manifest) must be off in Flatpak, as above; the Flathub terms ask for a way to report bugs, which the GitHub issue tracker is.

## Work list if it goes ahead

1. `InstallKind::Flatpak` and the Settings text (small, tested without a sandbox).
2. Downloads and "Save to library" folder choice through the portal (depends on the yt-dlp worker's shape).
3. Manifest, cargo sources, mpv and libplacebo modules (the bulk of the time), a local `flatpak-builder` run, the checks in `flatpak-builder-lint`.
4. QA in the sandbox: embedded video on Wayland (Intel, AMD, NVIDIA), keyring on GNOME and KDE, portal folder chooser plus scanning, gamepad, a Jellyfin sign-in, yt-dlp, no PackageKit message.
5. Metainfo additions (OARS, screenshots, releases) and the Flathub pull request.
