# Network buffering (watermarks)

`nocturne_player::Buffering` (in `Options.buffering`, and `EmbeddedPlayer::with_buffering`) is applied identically to the child-mpv `Player` (startup args) and `EmbeddedPlayer` (`set_option` before init). Defaults: high 300 s, low 120 s, `max_bytes` 1.5 GiB, `back_bytes` 64 MiB.

| field | mpv option |
|---|---|
| (always) | `cache=yes` |
| `high_secs` | `demuxer-readahead-secs`, and `cache-secs` (same value) |
| `low_secs` | `demuxer-hysteresis-secs` |
| `max_bytes` | `demuxer-max-bytes` |
| `back_bytes` | `demuxer-max-back-bytes` |

Clamps: non-finite -> default; high 2..=86400; low 1..=high-1; max_bytes >= 16 MiB; back_bytes <= max_bytes. `State.buffered` is mpv `demuxer-cache-duration` (seconds buffered ahead of the position).

## Two facts that differ from the mpv docs' obvious reading (measured on mpv 0.41)

1. `demuxer-hysteresis-secs` is the **absolute remaining level at which reading resumes**, not `readahead - hysteresis`. high 30 + hysteresis 12 refills at 12 s (first attempt with `high - low` = 18 refilled at 18 s). `0` disables it (continuous top-up, the old behaviour). So hysteresis = `low_secs`.
2. `cache-secs` must be set too. With `cache=yes` its default (3600000 s) overrides the readahead: the buffer grew to 590 s (the whole clip) on a 30 s readahead.

## Evidence (`cargo test -p nocturne-player --test buffering -- --nocapture`)

Local std-only HTTP range server, 10 min 320x240 H.264 clip (~400 kbit/s), headless child mpv at 4x speed, policy scaled down to 30/12 s, played to position 70 s, sampled every 50 ms over IPC (344 samples). "read event" = a 50 ms sample in which the demuxer cache (`demuxer-cache-state.total-bytes`) grew after the first fill.

| run | max buffered | lowest after fill | refills | read events / samples |
|---|---|---|---|---|
| watermark 30/12 | 30.0 s | 12.1 s | 3 | 4 / 344 |
| hysteresis=0 (old) | 30.1 s | 29.8 s | 0 | 341 / 343 |
| cache-secs left at default | 590.4 s | 29.3 s | 0 | (whole clip read) |

Old behaviour: buffer pinned at the high mark, the cache grows in nearly every sample (continuous tiny reads). Watermark: no reads between 30 s and 12 s, then one burst back to 30 s. The HTTP server sees one persistent connection either way (kernel socket buffers hide the pause from server-side byte counts), so the client-side cache growth is the honest metric. Live values read back via IPC `get_property` on the real child: `cache=yes`, `cache-secs=30`, `demuxer-readahead-secs=30`, `demuxer-hysteresis-secs=12`, `demuxer-max-bytes=1610612736`, `demuxer-max-back-bytes=67108864`.

## RAM caveat

`max_bytes` caps the buffer whatever the seconds say. Seconds buffered = `max_bytes` / bitrate: a 4K remux at ~80 Mbit/s (10 MB/s) holds only ~150 s in 1.5 GiB, so it never reaches 300 s; 1080p at 5 Mbit/s needs ~190 MiB for 300 s. Peak RAM per player is about `max_bytes + back_bytes` (cache is in memory, not on disk). Lower `max_bytes` on small machines; raise it only if 4K remuxes must reach the high mark.

## Playing dry

mpv defaults are kept: `cache-pause=yes`, `cache-pause-wait=1` (pause and rebuffer 1 s when the buffer runs dry, `paused-for-cache` while waiting), `cache-pause-initial=no`.

## Plumbing user settings (app side)

Build `Buffering { high_secs, low_secs, max_bytes, back_bytes }` from settings and pass it as `Options { buffering, ..Default::default() }` to `Player::new`, and call `EmbeddedPlayer::with_buffering(display, parent, buffering)` instead of `new`. Settings are read at startup of the player instance; changing them needs a new player (or `Player::command(&["set", ...])`).
