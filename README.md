# NOCTURNE

A native, dark, cinematic Jellyfin desktop client written in Rust.
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
cargo test                       # offline tests
cargo test -- --ignored          # live tests against your server (read-only GETs)
```

Integration tests only issue read-only requests and never modify the server.

## Roadmap

1. Jellyfin client (libraries, items, images, stream URLs)
2. libmpv playback
3. UI shell and design system
4. Polish: keyboard navigation, accessibility, image caching

See `CLAUDE.md` for the agent workflow used to build this project.
