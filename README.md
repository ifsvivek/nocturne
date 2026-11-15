# NOCTURNE

**YOUR MEDIA. YOUR NIGHT.**

A native cinematic media experience for Linux, written in Rust. Jellyfin is the first supported media source.
UI with [GPUI](https://github.com/zed-industries/zed), playback with libmpv. Linux/Wayland first.

> Status: early development. Read-only browsing and playback are the first milestones.

## Requirements

- Rust (stable, 2024 edition)
- libmpv dev files (`pkg-config --modversion mpv`)
- A Jellyfin server (developed against 12.x)
- Vulkan-capable GPU

## Configuration

Copy `.env.example` to `.env` and fill it in. `.env` is git-ignored; never commit it.

| Variable | Meaning |
|---|---|
| `NOCTURNE_JF_URL` | Server URL (default `http://127.0.0.1:8096`) |
| `NOCTURNE_JF_TOKEN` | Jellyfin API key (Dashboard > API Keys) |

## Development

```sh
cargo build
cargo run -p nocturne-app        # the desktop client (binary: nocturne); reads .env
cargo test                       # offline tests
cargo test -- --ignored          # live tests against your server (read-only GETs)
```

**Throwaway Jellyfin for admin tests.** `scripts/test-jellyfin.sh {start|stop|reset|status}` runs the installed `/usr/sbin/jellyfin` (12.x) on `127.0.0.1:8097` with all state under `/tmp/nocturne-jf-test` (`--datadir/--configdir/--cachedir/--logdir`, a pre-written `network.xml` that binds loopback only and disables UDP autodiscovery/UPnP; it never touches `/var/lib/jellyfin`, `~/.config` or your server on 8096). First `start` completes the setup wizard, creates an admin with a random password (`admin-password`, temp dir only), an API key, users `alice`/`bob` and a library of three generated ffmpeg clips; URL and token go to `/tmp/nocturne-jf-test/test.env` (mode 600). Needs `jellyfin`, `ffmpeg`, `curl`, `jq`.

```sh
scripts/test-jellyfin.sh start
NOCTURNE_JF_TEST_ENV=/tmp/nocturne-jf-test/test.env cargo test -p nocturne-jellyfin-admin -- --ignored
scripts/test-jellyfin.sh stop     # or `reset` to delete everything
```

`crates/jellyfin-admin` holds all admin reads and writes. Destructive calls (`delete_*`, password set/reset, `restart_server`) require a `Confirmed` token that UI code mints only after the user confirms. Its live tests refuse to run (`guard::TestServerGuard`) unless the URL is loopback port 8097, differs from your real server, and the server reports a `/tmp` data path; they never read `.env`.

`vendor/xattr` is a one-line patched copy of the `xattr` crate (a transitive dependency of GPUI that no longer builds against current `libc`); see the `[patch.crates-io]` note in `Cargo.toml`.

**Read-only boundary.** `crates/jellyfin` is strictly GET-only (no POST/PUT/DELETE/PATCH): the server holds your personal media and the browsing/playback layer must not be able to change it. `crates/jellyfin/tests/readonly.rs` enforces this. Write/admin calls (users, libraries, tasks, sessions, settings) live in the separate crate `crates/jellyfin-admin`; every write action will require explicit UI confirmation and will only be tested against a throwaway Jellyfin instance, never your real server. Integration tests here only issue read-only requests.

## Roadmap

1. Jellyfin client (libraries, items, images, stream URLs)
2. libmpv playback
3. UI shell and design system
4. Polish: keyboard navigation, accessibility, image caching

See `CLAUDE.md` for the agent workflow used to build this project.
