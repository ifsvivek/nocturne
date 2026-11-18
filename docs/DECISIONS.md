# Decisions

Short records of choices that shape the code, newest first. Each says what was decided, why, and what would change it.

## 2026-11-16 Several targets, Arch first

NOCTURNE will ship for Arch (PKGBUILD), other Linux formats (.deb, .rpm, AppImage where honest), macOS (.dmg) and Windows (.msi/.exe), in that order, without degrading the Linux experience. Platform code stays behind a few seams (video embedding, player IPC, keyring backend, media keys, signals, paths). Footprint is measured per release against the 0.2.0 baseline. Details: [PLATFORMS.md](PLATFORMS.md). Replaces the earlier "Linux only" rule.

## 2026-11-16 One catalog over local folders and several Jellyfin servers

Sources are a two-variant enum (`Jellyfin`, `Local`), not a trait-object plugin system: two implementations exist and enum dispatch keeps it simple; a third provider is one more variant. Every `Media` carries its origin; identity is (origin, id). Duplicates merge only on provider ids (IMDb / TMDB / TVDB, episodes by series id + numbers), never on title alone, because a false merge hides content while a missed merge only shows two cards. Local index is versioned JSON held in memory (fine to tens of thousands of files); SQLite only if that stops being true. Online metadata is opt-in. Details: [SOURCES.md](SOURCES.md).

## 2026-11-16 libmpv loaded at run time

The binary used to link `libmpv.so.2`; when it was missing the dynamic loader killed the process before `main`, so no error could be shown and browsing was impossible. libmpv is now `dlopen`ed (`crates/player/src/mpv_ffi.rs`, a small hand-written binding for the ~18 functions used, checked against the mpv headers). Missing libmpv falls back to the external `mpv` player; missing both shows a per-distribution install hint. The tarball installer installs runtime packages with the distribution's package manager; the PKGBUILD declares `mpv` explicitly (namcap cannot see dlopen).

## 2026-11-16 Iced replaces GPUI

GPUI is published irregularly, its release lacks accessibility support, and the in-window video glue depended on its internals, forcing an exact version pin and a vendored `xattr` patch. Iced 0.14 is a stable, documented release with an Elm-style architecture that fits a state-heavy client, exposes raw window handles (`window::run`), supports transparent windows, and has built-in animation primitives. A spike (`spikes/iced-embed`) showed the existing Wayland-subsurface video path works unchanged under Iced (Vulkan backend; the GL backend darkens video, so it is not forced). Framework-free logic moved to `crates/core` first so the UI can be rewritten without touching it. Revisit if Iced loses maintenance or a native video widget appears upstream.

## Earlier (0.1-0.2)

- **Read-only by default**: one setting gates every server write; confirmation tokens for destructive calls ([WRITES.md](WRITES.md)).
- **In-window video via a Wayland subsurface with libmpv's OpenGL render API**, never an in-process Vulkan mpv context (NVIDIA crash); X11 uses an external mpv window ([video-embedding.md](video-embedding.md)).
- **Tokens only in the system keyring**, no file fallback.
- **One tokio runtime** behind a bridge, because the UI framework runs its own executor.
