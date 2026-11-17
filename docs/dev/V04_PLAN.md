# 0.4 plan (local branch `v0.4-local`; never pushed, no release until the maintainer says so)

Scope = everything the maintainer approved (requests log #13, #18, #20) + the 0.3.1 fix batch (`deea022` is already in).
Rules: CLAUDE.md applies unchanged (read-only by default, writes gated, local files untouched, run-isolated.sh for
every test launch, throwaway servers only). Workers use worktrees off `v0.4-local`; max three at once.

## Waves

**Wave 1 (independent areas)**
- W1 Playback depth (`crates/app/src/player/`, core `player_logic`/`video`, `crates/player` options):
  dual subtitles; audio/subtitle language remembered per show across restarts incl. "original audio" (local store,
  never on the server); HDR mode: detect HDR stream, prompt "Play here in SDR / Open in HDR pop-out window /
  remember", setting, pop-out = external `Player` with `vo=gpu-next` + `target-colorspace-hint=yes` fullscreen, IPC
  control + "Playing in HDR" panel (docs/HDR.md).
- W2 Server correctness + home (`crates/jellyfin*`, core `sync`/`profile`/`source`, `app/home.rs`, `app/detail` for
  "Included in"): truthful playback reports (play method, media source, play session id), visible non-modal error on
  failed report + one retry of the final "stopped"; Latest row via `Items/Latest` incl. new episodes, Play on a show
  card = next unwatched episode; optional extra HTTP header per server (stored in the keyring if it is a secret);
  "Included in" collections on detail pages.
- W3 Linux packages (`packaging/`, `scripts/`, docs/PACKAGING.md only): .deb and .rpm built inside their distro
  containers (glibc of that distro), install-tested in clean containers; AppImage evaluation (honest: no GPU drivers
  bundled; decide ship/not with size + hwdec test).

**Wave 2**
- Online metadata for local media (opt-in, user TMDB key in keyring; MusicBrainz + CAA; TVmaze; attribution UI;
  docs/METADATA.md) and subtitle download for local files (opt-in, user OpenSubtitles key).
- Download from a server into the local library (offline copies; GET only; resumable; shows up as a local copy).
- One watched state across sources (opt-in, write-gated) and Trakt sync (opt-in, user's own Trakt app/OAuth).
- Smart collections across sources (local rules, e.g. unwatched 4K movies).
- Remote control from Jellyfin apps (`Sessions/Capabilities` + session WebSocket general/playstate commands).

**Wave 3**
- Couch / TV mode (10-foot layout, gamepad + remote/keyboard navigation).
- macOS and Windows ports behind the platform seams (docs/PLATFORMS.md): keyring backends, named-pipe IPC, paths,
  media keys, bundled libmpv, .dmg/.msi packaging scripts. Windows: cross-compile check only; macOS: written, not built
  (no hosted CI by maintainer decision).

**Polish**: player menus reachable by keyboard (Subtitles menu has no keyboard path); trim surrounding quotes/whitespace from pasted API keys (TMDB, OpenSubtitles, Trakt); carried from 0.3: friendly message instead of a panic when no display is available; header nav overflow
hint; admin dashboard session label.

## After 0.4 (not this release)

0.5: per-server choice of interface: NOCTURNE's own UI or a native rendering of the server's UI (no web view, by maintainer decision;
exact meaning to be agreed when 0.5 starts); plus UI refresh and user customization comparable to Jellyfin's display settings (themes / accent, home sections and
their order, density and scale, backdrops, per-library views). Wish list to be agreed with the maintainer first.

## Progress
- [x] W1 (merged; real HDR output unverified: no HDR display, mpv 0.41)  - [x] W2 (merged; player hooks wired)  - [x] W3 (.deb + .rpm verified in clean containers; AppImage NOT shipped: 107 MB, libmpv/glibc conflict; Flatpak to evaluate)  - [x] Wave 2 (merged: online metadata + subtitles, remote control, watched sync + Trakt (Trakt live sign-in untested), download-to-local + subtitle hook, smart collections)  - [~] Wave 3 (running: macOS/Windows code ports; couch mode done, real gamepad / TV untested; left: installers)  - [~] polish (menus by keyboard, key trimming, nav overflow done; no-display panic message and admin session label left)  - [ ] 0.4 QA walkthrough
