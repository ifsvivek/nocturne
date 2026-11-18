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
