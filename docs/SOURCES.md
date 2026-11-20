# Media sources: local library, several Jellyfin servers, one catalog

Status: design accepted 2026-11-16; implemented in milestones (bottom); Server UI (below) shipped in 0.5. NOCTURNE must be useful with no server at all, with one or several Jellyfin servers, or with both. Server media is never copied, moved or modified; local files are never renamed, moved, modified or deleted.

## Where we start

- `nocturne_core::source::Source` wraps one Jellyfin client + user; the UI holds a single `Option<Source>` (the 0.2.0 model).
- `Media` carries a per-server item id and nothing that says which server it came from.
- Credentials: keyring entries per (server URL, user id) (`crates/session`), plus `servers.toml` / `accounts.toml`. No database; settings are versioned TOML; images in a disk LRU (`imgcache.rs`).

## Model

```
Catalog (core)                      one per process, owned by the UI root
 ├─ Slot { id: SourceId, profile: Profile, backend: Backend, status: Status }
 │    Backend = Jellyfin(jellyfin::Source)   per server, own client, token, request gate, cache scope
 │            | Local(local::Library)        the on-disk index of the user's folders (crates/local)
 ├─ fan-out queries (libraries, latest, resume, next up, browse, search) -> per-slot results, merged
 └─ grouping: Entry { primary: Media, alternates: Vec<Media> } for detected duplicates
```

- **Enum dispatch, not a trait object.** Two backends exist; `Backend` is an enum with the methods the UI uses. A third provider later is one more variant. No plugin framework.
- **`SourceId`**: small stable id (`u32`) assigned when a profile is created, persisted in `sources.toml`. Every `Media` gains `origin: SourceId`; the pair `(origin, id)` is the item's identity everywhere (navigation, caches, image keys, playback, writes). Opening or playing an item always resolves through its own slot.
- **`Profile`** (persisted in `$XDG_CONFIG_HOME/nocturne/sources.toml`, versioned, atomic write, no secrets): `Jellyfin { url, user_id, user_name, label }` (token stays in the keyring under the existing (url, user id) key) or `Local { label, roots: Vec<PathBuf> }`; plus `enabled` and a priority order used to pick the preferred copy of a duplicate. Existing users: on first run, `servers.toml` / `accounts.toml` and the current keyring login migrate to one Jellyfin profile; old files are left in place (downgrade-safe).
- **`Status`** per slot: `Connecting`, `Online`, `Offline { retry_at }` (backoff, as today), `SignedOut` (401 / missing token -> sign in again for that server only), `Indexing { done, total }` (local). One slot failing never blocks others; fan-out results arrive per slot and render as they come, with per-source failure noted quietly.
- **Isolation**: each Jellyfin slot keeps its own `Client`, token, semaphore and image-cache scope. Writes (played, favourite, progress) go only to the item's origin server, through that server's user client, still gated by `allow_server_writes`. Admin and SyncPlay act on one chosen server.

## Duplicates

- Merge only on strong identity, never on title alone: movies by a shared provider id (IMDb / TMDB / TVDB); episodes by the series' provider id + season + episode number; series by provider id. Jellyfin supplies `ProviderIds` (requested as an extra field, GET only); local items get them from `.nfo` files or `[imdbid-tt…]` / `{tmdb-…}` tags in names (Jellyfin naming convention).
- Title + year matches without ids are *possible* duplicates: shown separately, never merged automatically.
- Server-side versions of one item (Jellyfin `MediaSources`, e.g. 4K and 1080p) and cross-source copies share one "Versions and sources" picker on the detail page.
- A merged `Entry` shows the preferred copy (first by profile priority among available slots, overridable per item and remembered locally) and lists the others as alternate sources on the detail page (source, quality, availability).
- Metadata conflicts: the entry displays the preferred copy's metadata; nothing is written back anywhere.

## Local library (`crates/local`, no UI deps)

- **Scan**: recursive walk of each root; follows symlinks (also ones pointing outside the root) with a visited (dev, inode) set so loops end; unreadable entries are reported, not fatal; video, audio and image extensions; skips hidden dirs and dirs marked with `.nomedia` / `.ignore`. Runs on a blocking thread with progress events; cancellable; re-scan is incremental (size + mtime unchanged -> skip).
- **Identity**: each file gets a stable `LocalId` at first sight. A file that disappears and reappears elsewhere with the same (dev, inode) or the same (size, partial content hash) keeps its id, so resume points and manual edits survive moves and renames. Deleted files are marked missing (kept with their edits until the user removes them or for 30 days), never silently dropped while their root is unmounted.
- **Naming**: movies `Title (Year)` / `Title.Year.…`; TV `Show/Season 01/… S01E02 …`, `1x02`, `S01E02-E03`, specials as season 0; provider tags `[imdbid-tt…]` `{tmdb-…}`; extras folders skipped. Unparsed files land in "Unmatched" for manual sorting.
- **Metadata, in order**: user overrides > `.nfo` (Kodi format) > sidecar art (`poster.jpg`, `folder.jpg`, `fanart.jpg`, `<name>-poster.jpg`, …) > embedded tags (`ffprobe` if installed: runtime, streams) > filename. Online metadata is optional and off by default; users supply their own TMDB key (decided 2026-11-16). Sources, licences, limits and attribution: [METADATA.md](METADATA.md). The app is fully usable without it.
- **Persistence**: `$XDG_DATA_HOME/nocturne/local/index.json` (versioned schema with in-code migrations, atomic write) and `overrides.json` (user edits keyed by `LocalId`, never touched by scans); resume positions and played state for local items in `state.json`. ponytail: JSON in memory is fine to ~50k files; move to SQLite if libraries outgrow that.
- **Playback**: the file path goes to the same player seam (`video.rs`) as a stream URL; no headers. Missing or unreadable files give a clear error on the detail page instead of starting the player.

## UI

- First run (no profiles): a welcome screen with three choices: Local library, Connect to a server, Both.
- Home and browse show the unified catalog by default; a source filter (All / each source) lives in the toolbar, and each server's or local folder's libraries are reachable on their own. A small source mark on cells and the detail page tells local from remote and shows availability; nothing else server-specific clutters browsing. With several sources the header lists the unified kinds (Movies, Shows, Music, Photos as available) and one **Libraries** menu grouping each source's libraries under its mark; with one source it lists that source's libraries inline. Unified lists merge duplicates into one cell marked "2 SOURCES".
- **Server UI** (per server, Settings > Sources > "UI:"): the default is the server's own look when it has custom branding CSS and NOCTURNE's look when it has none; an explicit choice (including NOCTURNE) is stored and always wins, and "never chosen" is not the same as NOCTURNE. "All sources" never chosen follows the look chosen for the first server, with the items of every source; choosing a look for "All" overrides that. With that server alone selected in the source filter, Home follows the server's own layout: its home sections in its order (`homesection0..9` of the user's display preferences, jellyfin-web defaults when unset), a My media row of its libraries in the user's `OrderedViews` order without `MyMediaExcludes`, and Latest rows without `LatestItemsExcludes`. With the filter on All the unified NOCTURNE rows are always used. The server's branding CSS (`/Branding/Configuration`) becomes that server's theme layer: user per-server theme, then server branding, then the global theme, then the defaults; it is never saved into the saved themes. Live TV rows and the audio and book continue rows are skipped without an error. Details: [dev/SERVER_UI_PLAN.md](dev/SERVER_UI_PLAN.md).
- Settings gains **Sources**: list of profiles with status, add / edit / remove / enable / reorder, sign in again, and the local folders screen (add / remove roots, rescan, indexing progress, unmatched items with manual edit).

## Milestones (each independently testable)

1. `crates/local`: scanner, name parser, index + migrations, move/rename/delete handling, NFO + sidecar metadata, overrides; unit tests on temporary fixture trees. (Independent of the UI.)
2. Core catalog: `SourceId` + `origin` on `Media`, `Profile` + `sources.toml` + migration from the single-server files, `Backend` enum with fan-out and per-slot status, provider ids from Jellyfin, duplicate grouping and preferred-source choice; tests with fixtures and two throwaway servers (ports 8097 and 8098).
3. (done, 2026-11-16; local playback and the cinematic startup screen are not part of it) Iced root on the catalog: first-run choice, Sources settings, source filter and marks, local playback + local resume state, cinematic startup screen driven by real init steps.
4. Remaining screens ported directly on the catalog model (home, browse, search, detail incl. alternate sources, player, account, admin per server, SyncPlay per server).
5. (done in 0.4) Optional online metadata provider for local items (opt-in, key supplied by the user where required); layer order: user overrides > .nfo > sidecar art > online > filename.
