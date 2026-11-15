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

`vendor/xattr` is a one-line patched copy of the `xattr` crate (a transitive dependency of GPUI that no longer builds against current `libc`); see the `[patch.crates-io]` note in `Cargo.toml`.

**Read-only boundary.** `crates/jellyfin` is strictly GET-only (no POST/PUT/DELETE/PATCH): the server holds your personal media and the browsing/playback layer must not be able to change it. `crates/jellyfin/tests/readonly.rs` enforces this. Write/admin calls (users, libraries, tasks, sessions, settings) will live in a separate crate, `crates/jellyfin-admin` (to be created later); every write action will require explicit UI confirmation and will only be tested against a throwaway Jellyfin instance, never your real server. Integration tests here only issue read-only requests.

## Roadmap

1. Jellyfin client (libraries, items, images, stream URLs)
2. libmpv playback
3. UI shell and design system
4. Polish: keyboard navigation, accessibility, image caching

See `CLAUDE.md` for the agent workflow used to build this project.
