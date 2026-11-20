# Performance and footprint log

Budget and rules: [PLATFORMS.md](../PLATFORMS.md#footprint-budget-and-measurements). Script: `scripts/perf.sh`. Each release appends a column here; the Iced build must not regress against the GPUI baseline without a written reason.

## Baseline: GPUI app (`nocturne-app`), 2026-11-16

Built from the `iced-migration` branch head (commit `c63d096`; `nocturne-app` is the GPUI code, same as the 0.2.0 release apart from libmpv, see notes) with `cargo build --release -p nocturne-app` (`lto = "fat"`, `codegen-units = 1`, `strip = true`).

Machine: AMD Ryzen 7 5800H (8C/16T), NVIDIA GeForce RTX 3060 Mobile (GA106M) plus AMD Cezanne iGPU, 22 GiB RAM, Linux 7.3.1 (CachyOS), Hyprland 0.57.1 on Wayland. The machine was not otherwise idle (other builds and an unrelated Iced debug window were running), so treat the numbers as roughly +/- 10 to 20 percent.

| Metric | GPUI (nocturne-app, iced-migration HEAD) |
|---|---|
| Binary | 26941688 B (25.69 MiB) |
| Tarball (`scripts/package.sh`, separate remapped build) | 11451356 B (10.92 MiB) |
| Installed (install.sh, DESTDIR) | 27833241 B (26.54 MiB) |
| Start to window mapped, median of 5 (ms) | 606 |
| Start to interactive, NOCTURNE_TIMING=1 (ms) | n/a (GPUI prints no timing line) |
| Idle sign-in: RSS MiB after 10 s / CPU % over 30 s | 342.9 / 1.57 |
| Idle loaded home (demo, dev token): RSS MiB / CPU % | 414.0 / 2.00 |
| CPU during 60 s 1080p embedded playback | manual (not measured) |

The tarball size was measured afterwards from the tarball `scripts/test-install.sh` built (no `dist/` tarball existed when the script ran; the script reports `n/a` in that case).

### Method

- Every run: throwaway `XDG_CONFIG_HOME` / `XDG_DATA_HOME` / `XDG_CACHE_HOME`, `NOCTURNE_KEYRING_SERVICE=nocturne-test`, working directory outside the repo (so no repo `.env` is read), `NOCTURNE_TIMING=1` set.
- Installed size: `install.sh install` into a `DESTDIR` staging root from a tarball-shaped directory; sum of file sizes.
- Start to window mapped: wall time from `exec` until `hyprctl clients -j` lists a mapped client with the process's pid (polled every 10 ms), median of 5 runs on an empty config (sign-in screen). Matching by pid matters: other `nocturne` windows may exist (a first version matched on class and reported 7 ms because of a stale window). Needs Hyprland, else `n/a`. "Interactive" is read from a timing line printed with `NOCTURNE_TIMING=1` (any line containing `interactive` and a `... ms` value); GPUI prints none.
- Idle RSS: `VmRSS` from `/proc/<pid>/status` 10 s after the window appears. Idle CPU: (utime + stime) delta from `/proc/<pid>/stat` over the next 30 s, as percent of one core.
- Sign-in row: empty config. Loaded home row: dev-token mode (`NOCTURNE_JF_TOKEN` = the throwaway demo server's API key, `NOCTURNE_JF_URL=http://127.0.0.1:8097`) against the library built by `scripts/make-demo-library.sh build`. Never a real server.
- 1080p playback CPU is not automated (it needs driving the UI): play a 1080p demo clip embedded with hwdec on, then take the (utime + stime) delta of the app pid over 60 s, as in the script's trailing comment.

### Notes

- The 0.2.0 release binary linked libmpv at build time; the current one `dlopen`s it. No material difference in size or start time is expected, and none was measured separately.
- A first pass with other builds running showed a 9 % home idle CPU outlier (the table is the second, quieter pass); repeat on a quiet machine for release numbers.

## 0.3.0 (Iced) vs 0.2.0 (GPUI), 2026-11-16

Same machine and method (`scripts/perf.sh`, release builds, throwaway XDG dirs, demo server in dev-token mode for the loaded-home row). Machine quiet apart from the desktop session.

| Metric | 0.2.0 GPUI (baseline) | 0.3.0 Iced | Change |
|---|---|---|---|
| Binary | 25.69 MiB | 30.13 MiB | +4.4 MiB |
| Tarball | 10.92 MiB | 12.48 MiB | +1.6 MiB |
| Installed (install.sh) | 26.54 MiB | 30.92 MiB | +4.4 MiB |
| Start to window mapped (median of 5) | 606 ms | 387 ms | -36 % |
| Idle sign-in RSS / CPU | 342.9 MiB / 1.57 % | 286.7 MiB / 1.43 % | -16 % RSS |
| Idle loaded home RSS / CPU | 414.0 MiB / 2.00 % | 297.7 MiB / 1.70 % | -28 % RSS |
| CPU during 60 s 1080p embedded playback | manual | manual | hwdec = nvdec, 0 dropped frames (QA) |

The larger binary is wgpu (Vulkan + GL backends), Iced's tiny-skia software fallback and the multi-source/local library code. Accepted for 0.3: startup and memory improved, and the software renderer keeps the app usable on machines without a working Vulkan driver. ponytail: if size matters later, measure dropping tiny-skia or the GL backend first.

## 0.4.0 (unreleased) vs 0.3.0, 2026-11-17

Same machine and method (`scripts/perf.sh target/release/nocturne 0.4`, release build of `v0.4-local` at `7db009e` plus the QA fixes, throwaway XDG dirs, a demo server on `127.0.0.1:8108` in dev-token mode for the loaded-home row). The desktop session was running (a browser and a terminal), so treat the numbers as roughly +/- 10 percent.

| Metric | 0.3.0 Iced | 0.4.0 | Change |
|---|---|---|---|
| Binary | 30.13 MiB | 32.24 MiB (33806384 B) | +2.1 MiB |
| Tarball | 12.48 MiB | n/a (`scripts/package.sh` not run for this row) | |
| Installed (install.sh) | 30.92 MiB | 33.04 MiB | +2.1 MiB |
| Start to window mapped (median of 5) | 387 ms | 387 ms | 0 |
| Idle sign-in RSS / CPU | 286.7 MiB / 1.43 % | 285.9 MiB / 1.50 % | no change |
| Idle loaded home RSS / CPU | 297.7 MiB / 1.70 % | 298.8 MiB / 1.83 % | no change (noise) |
| CPU during 60 s 1080p embedded playback | manual | manual | not measured |

The +2.1 MiB is the 0.4 features (online metadata clients, downloads, Trakt, remote control, smart collections, couch mode with the gamepad library `gilrs`, Windows/macOS seams). Startup, idle memory and idle CPU did not move.

## Build cost, 2026-11-18 (lean profiles, shared cache)

Clean `cargo build -p nocturne-app` (dev), 16 cores, machine shared with other builds: before 158 s and 5.3 GB target, after (`debug = "line-tables-only"`, `debug = false` for dependencies) 67 s and 2.6 GB. `fast-release` (thin LTO, 16 codegen units) clean: 181 s on the busy machine (135 s quiet, see PACKAGING.md), 46 MB binary; fat-LTO `release` clean, no sccache, Wayland-only iced (2026-11-18): 6 min 0 s (the run began with another build finishing, load 6.5 falling to 1.7, so a fully quiet run may be somewhat faster), 33.0 MB; `fast-release` on the same machine 2 min 52 s, 46.6 MB (a load spike from outside the run appeared at its end). A second worktree with sccache and `SCCACHE_BASEDIRS`: 92 % hits, dev build 67 s to 41 s (32 s fully warm). Container packaging: first run (image, rustup, 15.5 min compile on 6 niced CPUs over a slow link) then a repeat `package.sh deb` in 10 s and the rpm from the same binary installs on `fedora:latest` with no missing libraries.

## 0.5.2 theme engine on by default, 2026-11-20

Two fat-LTO `release` builds of the same commit (`main` at the theme fidelity round 3 merge), one with the old default (hand-written screens, no `nocturne-style`) and one with the `theme-engine` feature on (the new default). Run-time rows: the invisible sway session (`HS_NAME=prep`, 1600x900, no GPU, so the software renderer), dev-token mode against a throwaway demo server on loopback, Home loaded, throwaway XDG dirs, 10 s settle then 10 s of CPU; `NOCTURNE_TIMING=1` "first frame" for the cold start. The machine was shared with other builds and servers, so the timings are noisy (a single run varied by more than the difference); `scripts/perf.sh` was not used because it opens windows on the real desktop and needs `hyprctl`.

| Metric | Engine off | Engine on | Change |
|---|---|---|---|
| Binary | 36139648 B (34.47 MiB) | 39076928 B (37.27 MiB) | +2.80 MiB (+8 %) |
| Needed shared libraries (`ldd`) | same list | same list | no new run-time library |
| Idle Home RSS (2 runs each) | 311.8 / 312.2 MiB | 326.0 / 324.6 / 325.6 MiB | +13 MiB |
| Idle Home CPU over 10 s (one core, software renderer) | 48.0 / 46.1 % | 52.4 / 47.3 / 50.1 % | no clear change |
| Cold start to first frame | 816 / 773 ms | 782 / 928 / 1417 ms | within noise |
| Engine on, `NOCTURNE_ENGINE_UI=0` (fallback) | | RSS 324 to 326 MiB, first frame 1.9 to 2.4 s (noisy) | the fallback runs |

The +2.8 MiB is the engine crate with Taffy and `cssparser`, the default stylesheets and the engine screens (two crates under MPL-2.0 are new in the notices: `cssparser`, `dtoa-short`). The idle CPU of about 50 % appears in both builds in the GPU-less session (software rendering of an animating Home), so it is not caused by the engine; confirm on a real GPU session before quoting an idle CPU. The package builds and the Windows/macOS cross checks could not run (no container daemon), see the 0.5.2 release notes.

## Idle redraws, 2026-11-20

An idle app redrew about 6 times a second on Home and 10 times a second in the Library. Cause: the root `Message::Tick` subscription (`time::every(200 ms)`) ran for the whole life of the app, and Iced redraws the window after every message that reaches `update`, so a polling tick is a redraw (the Library, with its engine scene, drew twice per tick). The tick polls state that other messages change (toast expiry, offline retry, downloads, hooks, onboarding), so it was never needed while nothing happens.

Fix (`crates/app/src/app/mod.rs`, `redraw.rs`): `App::tick_period()` returns `None` when idle. The tick runs (200 ms) for 15 s after any real message (`keeps_awake`: not a tick, frame, hover, picture or no-op), and while a player exists, a toast shows, onboarding is visible, a download runs or waits, a server is offline or being probed, or the first local scan is pending. Terminate signals and a second launch (`single::activate_pending`) are read by a message-free stream (`redraw::wakes`, polls two atomics every 500 ms and sends only when one is set). The Home hero wakes the app at the time its slide changes (`Hero::next_in`); only when the theme draws the progress hairline (`hero_hairline`, not hidden by `display: none`) does it wake once a second to move it. The hairline edit no longer runs for themes that hide it.

Method: debug build, headless session (`HS_NAME=idle`), the demo server in developer-token mode, settled 12 to 30 s, then counted over a window. `NOCTURNE_REDRAW_LOG=1` (debug builds) prints one line per message (`Message` is not `Debug`: the variant index in `app/message.rs`; `Eng` events by name) and per drawn frame of an engine screen with what keeps it animating. `NOCTURNE_TICK_ALWAYS=1` (debug) restores the old always-on tick for the "before" rows. "Hero off" is `hero_autoplay = false` in `settings.toml`. CPU is utime plus stime clock ticks (100 per second) of the whole process over the window; it is a debug build, so only the ratio means anything.

| Screen (default theme unless noted) | Before: draws / 10 s | After: draws / 10 s | CPU ticks / 10 s before to after |
|---|---|---|---|
| Home | 69 (hero on; the always-on tick) | 0 with the hero off | 1625 to 1 |
| Library (Movies) | 100 | 0 | 619 to 1 |
| Search overlay | not measured, same tick | 0 | n/a |
| Item page | not measured, same tick | 0 | n/a |
| Settings (hand-written Iced page; 0 messages) | 50 messages | 0 messages | 195 to 1 |
| Home, `NOCTURNE_ENGINE_UI=0` | 50 messages | 0 | 946 to 1 |
| Library, `NOCTURNE_ENGINE_UI=0` | not measured | 0 | n/a |
| Glass: Home (hero off) and Library | not measured | 0 and 0 | n/a |
| Home, hero on, 30 s window (3 slide changes) | 333 draws, 150 ticks | 151 to 159 draws (the three slide fades, 0.8 s each), 3 ticks | 4170 to 1111 (default), 1238 (Glass) |

With the hero on, the only wake-ups are the slide changes (one per interval, 10 s by default) and the 380 ms crossfade they start. Playback keeps its own 200 ms player tick (a visible progress bar). A window that just received a message still ticks for 15 s, so measure after that.

On a `fast-release` build (hero off, 30 s window after settling): Home and Library use 1 CPU tick (10 ms) of the whole process in 30 s, i.e. about 0.03 % of a core. The old always-on tick cannot be measured on that profile (`NOCTURNE_TICK_ALWAYS` is debug only); the 0.5.2 table above shows 46 to 52 % of a core idle on Home with the software renderer at 6 draws a second. `scripts/headless-parity.sh`: keys-2 and keys-5 differ by 0.3 % and 0.03 % of pixels with this change and also with `NOCTURNE_TICK_ALWAYS=1` (old behaviour), so they are not caused by it.

## Memory with a 5,000-item library (QA 0.5.2 M3), 2026-11-20

QA saw 920 to 950 MiB RSS while browsing a 5,012-item local library, against 325 MiB idle Home. Method: `fast-release`, headless sway at 1920x1080 (software GL, so GPU textures are process memory too), `RssAnon`/`RssFile`/`VmHWM` from `/proc/<pid>/status`, a local folder of 3,000 movies and 200 shows of 10 episodes (5,000 empty files named by the usual rules), scripted browsing (open the library, 120 page-downs, every tab and view, End/Home, twice). Scripts are throwaway, in `/tmp`.

First, what the library itself costs (no artwork; the numbers are RSS after the run):

| Build | Idle onboarding | After browsing | Peak |
|---|---|---|---|
| Engine on | 372 MiB | 398 MiB | 398 |
| `NOCTURNE_ENGINE_UI=0` | 368 | 330 | 386 |
| `--no-default-features` | 369 | 388 | 388 |
| Engine on plus a throwaway server (dev token) | 376 | 473 | 473 |

So 3,000 to 5,000 items without pictures cost about 25 MiB over idle, the engine about 12 MiB over the hand-written screens at the peak, and a server's pictures about 75 MiB. The `RssFile` part (about 160 MiB) is Mesa's software renderer and the libraries mapped in this session; on a GPU session it is mostly not there. None of this reproduces 600 MiB, which points at pictures: the QA library did not have artwork on the local items but the servers did, and the memory crept.

With artwork (a `poster.jpg` of 1000x1500 beside each of the 3,000 movies) the same run before the fix:

| | Idle onboarding | Library opened | After 120 page-downs | After the run | Peak |
|---|---|---|---|---|---|
| Before | 445 MiB | 566 | 589 | 692 | 692 |
| Allocator tuned | 369 | 411 | 515 | 537 | 570 |
| Allocator tuned and far pictures released | 369 | 409 | 444 | 424 | 457 |

Causes, in order of size:

1. glibc malloc. Pictures are decoded on worker threads (a 1000x1500 source is a 6 MiB RGBA buffer, freed after the shrink). glibc keeps one arena per thread and raises its mmap threshold to the largest block freed, so those buffers stay resident as free heap: about 150 MiB after opening one library, and the idle baseline carried 70 MiB (445 to 369) of the same from the renderer's startup. `platform::tune_allocator` sets one arena and fixed 1 MiB mmap and trim thresholds (`mallopt`, glibc only, before any thread starts; `MALLOC_ARENA_MAX` in the environment wins). Probed first with the environment variables: `MALLOC_ARENA_MAX=1` plus a 1 MB trim threshold gave 414 to 436 MiB after the run, `MALLOC_MMAP_THRESHOLD_=1048576` alone 463.
2. The engine's library grid kept every card's decoded picture for as long as the page lived (the card, the scene's picture table and the in-memory cache all hold the handle), so scrolling a long library added about 0.2 MiB per card shown. `eng_trim_pics` (called from `browse_visible`) drops the pictures of cards more than 120 items from the rows in view; a card that comes back is fetched again from the disk cache like a new one (checked by scrolling away and back: the pictures return).

Floor: with software GL the process is about 370 MiB at the onboarding screen (190 MiB anonymous), so "within 150 MiB of idle" holds for browsing a 5,000-item library with or without pictures: +25 MiB without pictures, +55 to +75 MiB with a screenful of pictures and the lookahead held. The idle Home figure of 325 MiB in this table's earlier columns was a different session (hardware GL); compare within a session only. Not measured: a real GPU session, the QA's two servers, and the stuck player (libmpv loaded, which QA saw add about 20 MiB over its last samples).
