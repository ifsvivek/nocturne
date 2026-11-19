# Decisions

Short records of choices that shape the code, newest first. Each says what was decided, why, and what would change it.

## 2026-11-19 Release artifacts are built in CI and signed locally

Pushing a `vX.Y.Z` tag builds the tarball, .deb, .rpm, Windows installer and zip, macOS .dmg and a best-effort AppImage on GitHub runners and creates a draft release; CI never publishes and never holds a signing key. The maintainer downloads the artifacts, signs the package repositories (GPG) and the update artifacts (ed25519) on a local machine, deploys the website and publishes the draft. Why: runners give clean, reproducible builds of formats that cannot be built on one Linux machine (Windows, macOS), while a leaked CI secret would otherwise sign binaries for every user. Details: [PACKAGING.md](PACKAGING.md).

## 2026-11-19 Update signing uses a separate ed25519 key

In-app updates verify the announced size, sha256 and a detached ed25519 signature over the artifact bytes before the install directory is touched. The key is not the repository's GPG key: a leaked repository key must not be able to push binaries to Windows and macOS users, and the other way round. The manifest is unsigned, so a compromised website can only point at artifacts, which then fail verification; a manifest that is not newer than the running version is ignored. Two public keys are compiled in so a rotation ships in a normal release. Package-managed installs never update themselves. Plan: [dev/UPDATER_PLAN.md](dev/UPDATER_PLAN.md).

## 2026-11-19 Server UI: read the server's layout, write only home sections

Per server, NOCTURNE can follow that server's own Home (its home sections, library order and hidden libraries, branding CSS, login disclaimer) instead of the unified Home. It is off by default, uses only GET calls, and the server's branding is a theme layer that is never saved. The single write is the Sections editor, behind `allow_server_writes`: one read-modify-write of the user's display preferences that replaces only the ten home-section entries and sends every other key back as read, never with a developer token. Why so narrow: display preferences hold unrelated settings of other apps, and a wider write could clobber them. Plan: [dev/SERVER_UI_PLAN.md](dev/SERVER_UI_PLAN.md), matrix in [WRITES.md](WRITES.md).

## 2026-11-19 Native CSS without a browser engine

Themes are CSS because people already have it (Jellyfin branding, community themes), but NOCTURNE reads only what it can map to its own look: colours, radii, window background, a font family, and layout on a closed vocabulary of named app parts. Everything else is listed back to the user with line and column; nothing in the stylesheet is executed or fetched. A web view or browser engine would add a large dependency, a second renderer and a script-execution surface to a media centre for the sake of styling. The next stage (0.6 or later) is a native flex/grid layout over NOCTURNE's own components (researched, not released); it still adds no HTML, no web view and no arbitrary elements. Plan: [dev/CUSTOMIZATION_PLAN.md](dev/CUSTOMIZATION_PLAN.md).

## 2026-11-19 Linux is Wayland-only

In-window video is a Wayland subsurface under a transparent window; on X11 it needed a different, separately tested embedding and the toolkit's X11 backend pulled in several more libraries. Maintaining two display-server paths for one maintainer and one tested video design was not worth it, so 0.5 builds Iced without its X11 backend and an X11-only session gets a message and exit code 1. The external mpv window stays as the fallback when in-window video cannot start. Would change if a second video path becomes cheap to test.

## 2026-11-16 Several targets, Arch first

NOCTURNE will ship for Arch (PKGBUILD), other Linux formats (.deb, .rpm, AppImage where honest), macOS (.dmg) and Windows (.msi/.exe), in that order, without degrading the Linux experience. Platform code stays behind a few seams (video embedding, player IPC, keyring backend, media keys, signals, paths). Footprint is measured per release against the 0.2.0 baseline. Details: [PLATFORMS.md](PLATFORMS.md). Replaces the earlier "Linux only" rule. State since 0.5: signed pacman, apt and dnf repositories, Windows (x64) and macOS (Apple Silicon) as unsigned previews.

## 2026-11-16 One catalog over local folders and several Jellyfin servers

Sources are a two-variant enum (`Jellyfin`, `Local`), not a trait-object plugin system: two implementations exist and enum dispatch keeps it simple; a third provider is one more variant. Every `Media` carries its origin; identity is (origin, id). Duplicates merge only on provider ids (IMDb / TMDB / TVDB, episodes by series id + numbers), never on title alone, because a false merge hides content while a missed merge only shows two cards. Local index is versioned JSON held in memory (fine to tens of thousands of files); SQLite only if that stops being true. Online metadata is opt-in. Details: [SOURCES.md](SOURCES.md).

## 2026-11-16 libmpv loaded at run time

The binary used to link `libmpv.so.2`; when it was missing the dynamic loader killed the process before `main`, so no error could be shown and browsing was impossible. libmpv is now `dlopen`ed (`crates/player/src/mpv_ffi.rs`, a small hand-written binding for the ~18 functions used, checked against the mpv headers). Missing libmpv falls back to the external `mpv` player; missing both shows a per-distribution install hint. The tarball installer installs runtime packages with the distribution's package manager; the PKGBUILD declares `mpv` explicitly (namcap cannot see dlopen).

## 2026-11-16 Iced replaces GPUI

GPUI is published irregularly, its release lacks accessibility support, and the in-window video glue depended on its internals, forcing an exact version pin and a vendored `xattr` patch. Iced 0.14 is a stable, documented release with an Elm-style architecture that fits a state-heavy client, exposes raw window handles (`window::run`), supports transparent windows, and has built-in animation primitives. A spike (`spikes/iced-embed`) showed the existing Wayland-subsurface video path works unchanged under Iced (Vulkan backend; the GL backend darkens video, so it is not forced). Framework-free logic moved to `crates/core` first so the UI can be rewritten without touching it. Revisit if Iced loses maintenance or a native video widget appears upstream.

## Earlier (0.1-0.2)

- **Read-only by default**: one setting gates every server write; confirmation tokens for destructive calls ([WRITES.md](WRITES.md)).
- **In-window video via a Wayland subsurface with libmpv's OpenGL render API**, never an in-process Vulkan mpv context (NVIDIA crash); Linux is Wayland-only since 0.5, X11 support was removed ([video-embedding.md](video-embedding.md)).
- **Tokens only in the system keyring**, no file fallback.
- **One tokio runtime** behind a bridge, because the UI framework runs its own executor.
