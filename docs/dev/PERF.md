# Performance and footprint log

Budget and rules: [PLATFORMS.md](../PLATFORMS.md#footprint-budget-and-measurements). Script: `scripts/perf.sh`.
Each release appends a column here; the Iced build must not regress against the GPUI baseline without a written reason.

## Baseline: GPUI app (`nocturne-app`), 2026-11-16

Built from the `iced-migration` branch head (commit `c63d096`; `nocturne-app` is the GPUI code, same as the 0.2.0
release apart from libmpv, see notes) with `cargo build --release -p nocturne-app` (`lto = "fat"`,
`codegen-units = 1`, `strip = true`).

Machine: AMD Ryzen 7 5800H (8C/16T), NVIDIA GeForce RTX 3060 Mobile (GA106M) plus AMD Cezanne iGPU, 22 GiB RAM,
Linux 7.3.1 (CachyOS), Hyprland 0.57.1 on Wayland. The machine was not otherwise idle (other builds and an unrelated
Iced debug window were running), so treat the numbers as roughly +/- 10 to 20 percent.

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

The tarball size was measured afterwards from the tarball `scripts/test-install.sh` built (no `dist/` tarball existed
when the script ran; the script reports `n/a` in that case).

### Method

- Every run: throwaway `XDG_CONFIG_HOME` / `XDG_DATA_HOME` / `XDG_CACHE_HOME`, `NOCTURNE_KEYRING_SERVICE=nocturne-test`,
  working directory outside the repo (so no repo `.env` is read), `NOCTURNE_TIMING=1` set.
- Installed size: `install.sh install` into a `DESTDIR` staging root from a tarball-shaped directory; sum of file sizes.
- Start to window mapped: wall time from `exec` until `hyprctl clients -j` lists a mapped client with the process's
  pid (polled every 10 ms), median of 5 runs on an empty config (sign-in screen). Matching by pid matters: other
  `nocturne` windows may exist (a first version matched on class and reported 7 ms because of a stale window). Needs
  Hyprland, else `n/a`. "Interactive" is read from a timing line printed with `NOCTURNE_TIMING=1` (any line containing
  `interactive` and a `... ms` value); GPUI prints none.
- Idle RSS: `VmRSS` from `/proc/<pid>/status` 10 s after the window appears. Idle CPU: (utime + stime) delta from
  `/proc/<pid>/stat` over the next 30 s, as percent of one core.
- Sign-in row: empty config. Loaded home row: dev-token mode (`NOCTURNE_JF_TOKEN` = the throwaway demo server's API
  key, `NOCTURNE_JF_URL=http://127.0.0.1:8097`) against the library built by `scripts/make-demo-library.sh build`.
  Never a real server.
- 1080p playback CPU is not automated (it needs driving the UI): play a 1080p demo clip embedded with hwdec on, then
  take the (utime + stime) delta of the app pid over 60 s, as in the script's trailing comment.

### Notes

- The 0.2.0 release binary linked libmpv at build time; the current one `dlopen`s it. No material difference in size
  or start time is expected, and none was measured separately.
- A first pass with other builds running showed a 9 % home idle CPU outlier (the table is the second, quieter pass);
  repeat on a quiet machine for release numbers.
