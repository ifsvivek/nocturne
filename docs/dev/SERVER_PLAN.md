# NOCTURNE server (0.7.5)

Status: in progress on the 0.7.5 branch. Maintainer decisions of 2026-11-28 (in order): a native *arr stack in Rust with an integrated torrent client (movies, shows, music, books); then a client + server architecture; then one binary and one service with in-process modules. Research and the first decision: [REQUESTS_NATIVE.md](REQUESTS_NATIVE.md). API contract: [docs/api/openapi.yaml](../api/openapi.yaml), its changes: [docs/api/CHANGES.md](../api/CHANGES.md).

## 1. Process model

One headless binary, `nocturne-server` (`crates/server`), run by one systemd user unit, `nocturne-server.service`. No child daemons: every feature is a module inside the process. The same binary is the CLI. The desktop app, the web dashboard and the CLI are clients of one API.

| Piece | Where | Owner |
|---|---|---|
| API, auth, listeners, module framework, config, store, CLI | `crates/server/src/{api,auth,http,module,state,db,cli,web,tls}.rs` | worker A |
| Library manager (titles, profiles, indexers, search, queue, import, jobs) | `crates/server/src/{library,decide,import,naming,meta}.rs`, `modules/library.rs` | worker A |
| Downloads (torrent engine) | `modules/downloads.rs` over `crates/torrent` (`nocturne-torrent`, librqbit) | A (module), B (engine) |
| Indexers | `real.rs` over `crates/indexer` (`nocturne-indexer`: Torznab / Newznab, release parser) | A (adapter), C (client) |
| Requests (native list) | `modules/requests.rs` | worker A |
| Gateway, tunnel (cloudflared), discovery, sources | `modules/{gateway,tunnel,discovery,sources}.rs` | gateway worker |
| Storage (local dirs, SMB/NFS mounts, remote agents) | `modules/storage.rs` | storage worker |
| Dashboard files | `crates/server/web/**` (embedded by `build.rs`, served at `/`) | dashboard worker |
| Desktop client | `crates/app` | app worker |

`crates/arr-types` (`nocturne-arr-types`) is the contract between the library manager, the torrent engine and the indexer client: the `Downloader` and `Indexer` traits and the release / quality model. `dl.rs` makes them object-safe; `real.rs` adapts `nocturne-torrent` and `nocturne-indexer` to them; `fake.rs` has in-memory versions for tests and for `NOCTURNE_SERVER_FAKE=1` debug runs.

Idle cost: modules start only when enabled. The queue loop polls the torrent engine every 2 s only while a grab is active, else sleeps until a grab wakes it; the job loop sleeps until the next job is due (at most 15 min). The torrent engine and its network traffic exist only while `[downloads] enabled = true`.

## 2. Modules

A module implements `module::Module` (`name`, `routes`, `start`, `stop`, `reload`, `health`) and is listed in `modules::all()`. Its name is its `server.toml` section and its route prefix `/api/v1/<name>/`. `Registry::apply` starts newly enabled modules, stops disabled ones and reloads running ones whose section changed; it runs at startup and after `PUT /api/v1/config`, and publishes a `health` event per change. While a module is not running its routes answer 503. Cross-module state goes through `sh.ext::<T>()` slots: `DlSlot` (the running torrent engine, set by downloads), `LibSlot` (the library engine, used by requests), `CatalogSlot` (what the sources already hold, set by the sources module), `JobSlot` and `Counters` (for `/status`). The how-to is at the top of `module.rs`.

## 3. API and auth

REST JSON under `/api/v1/`, the WebSocket `/api/v1/events` (queue progress at most every 2 s while downloading, history, title, job, request, health and module events), and the dashboard's static files at `/`. Every registered route must be in `openapi.yaml`: `tests/openapi.rs` fails otherwise.

- First-run setup (`POST /api/v1/setup`, only while no user exists) creates the admin (Argon2id, the gateway's parameters) and signs in.
- Session tokens (`nst_…`) from setup and login, named API keys (`nsk_…`) from `/apikeys`; 256 random bits, only the SHA-256 stored, each revocable; `Authorization: Bearer` or `X-Api-Key`, `?token=` on the WebSocket only. Failed sign-ins are answered one at a time after a second. A password change revokes every session (API keys stay).
- The unix socket `$XDG_RUNTIME_DIR/nocturne/server.sock` (0600 in a 0700 directory) serves the same API; a peer with the server's uid is the local admin without a credential (the CLI).
- Listeners like the gateway's: `local` (loopback, default), `lan` (private-range peers only), `remote` (any peer, HTTPS with the user's certificate and key, or `local` behind a reverse proxy named in `trusted_proxy`). Default port 8100.

## 4. Configuration and state

- `$XDG_CONFIG_HOME/nocturne/server.toml` (`nocturne_core::server::ServerConfig`, version 1, atomic writes): typed sections `http`, `library`, `downloads`, `requests`; every other module's section is kept verbatim (`sections`, read with `section::<T>()`, written with `set_section`). Every module is off by default.
- `$XDG_DATA_HOME/nocturne/server/state.redb`: one redb file (pure Rust, MIT/Apache; 3.1 keeps MSRV 1.89) with one table per record type (JSON values under `u64` ids, ids never reused) and a `meta` table (schema version, counters, job runs). A file from a newer schema is refused. Modules add tables with `rec!`.
- Secrets only in the keyring (`server:` prefix): indexer API keys (`server:indexer:<id>`), the sources' tokens. Never in `server.toml`, the store or a response.
- Torrent engine state: `$XDG_DATA_HOME/nocturne/server/torrent/`; downloads: `[downloads] dir` (default `$XDG_DATA_HOME/nocturne/server/downloads`, put it on the libraries' file system so imports are hardlinks).

## 5. Library manager

Data model (`core/server.rs`): `Library` (kind, mode, root, folder and file templates), `Profile`, `IndexerDef` (user-added Torznab / Newznab URLs only, no shipped definitions), `Title` (movie, series, artist, author), `Unit` (the wanted leaf: the movie, an episode, an album, a book; `FileRecord` when NOCTURNE imported its file), `Grab` (queue), `HistoryEntry`, `BlockEntry`. Unit lists come from TVmaze (episodes), MusicBrainz (albums) and Open Library (books) without keys; movies need no lookup (the client sends title, year and ids from Discover).

Decision rules (`decide.rs`, pure, unit-tested):

- Wanted: monitored, released (an episode without a date is not), and no file or (upgrades on) a file below the profile's cutoff. Units a source already holds are not wanted.
- A release maps to units by ids (IMDb, TVDB, TMDB) or normalised name (movies: year within one), season and episodes; a full-season pack maps to the season's monitored episodes; albums and books by title.
- Rejections: Usenet (no Usenet client), blocklisted (same indexer entry, info hash, or name for the title), already downloading, quality not allowed, a unit not monitored / not out / not improved, required / ignored terms, size per unit outside min / max, too few seeders, language not wanted, and (automatic search) units not asked for.
- Upgrade: a better allowed quality while below the cutoff, or a PROPER / REPACK of the same quality.
- Ranking: quality rank, then revision, preferred-term score, seeders; ties go to the release filling more units (season pack over single episodes). The chosen set never grabs a unit twice.
- Searches: one per movie, album or book; per season when two or more episodes of it are wanted, else per episode.
- Downloads: a torrent error, a vanished torrent, or six hours without progress (not paused) fails the grab: blocklist, remove with its data, search again. Seeding ends at `seed_ratio` or `seed_hours` (default 1.0 or a week), then the torrent and its download data are removed.

Import (`import.rs`): movie and book take the largest matching file (samples skipped), episodes match by `SxxEyy` (a single episode with one unnamed video takes it), albums take every audio file (disc folders kept apart). Names come from the library's templates (`{Movie Title}`, `{Release Year}`, `{Series Title}`, `{season:00}`, `{episode:00}`, `{Episode Title}`, `{Artist Name}`, `{Album Title}`, `{Author Name}`, `{Book Title}`, `{Quality Full}`, `{Release Group}`, `{Original Name}`, `{ImdbId}`); every value is sanitised so no component can be `..`, hidden or contain a separator.

Safety of the import (CLAUDE.md rule 3):

- It writes only below the library root (checked again for every file) through a temporary name and a rename.
- A hardlink when download and library share a file system, else a copy; the download keeps seeding.
- It never overwrites or deletes a file it did not import: an existing file at the target that is not in the unit's `FileRecord` stops that unit's import (`import_blocked`, retry after fixing).
- An upgrade replaces only files NOCTURNE imported, and removes the old file only after the new one is in place.
- Managed mode: a folder NOCTURNE owns (created by it; the app adds it as a local source). Existing mode: the user's folder, same rules.

Jobs: RSS sync every 15 min; missing search and upgrade search daily (the first ones 15 min after the first start); refresh metadata every 12 h; each also on demand (`POST /library/jobs/{job}/run`).

## 6. Requests

`POST /api/v1/requests` records a request (kind, name, year, ids, seasons, requester). With `auto_approve` (default) and the library module running it becomes a monitored title in the first library and profile of its kind (or the ones given) and is searched at once; otherwise an admin approves it. Its status follows the title: downloading while a grab is active, available once every monitored, released unit has a file or is on a source. Seerr and the external Radarr / Sonarr connections stay in the app until their move to the server later in 0.7.5.

## 7. CLI and service

`nocturne-server install [--linger] | uninstall | start | stop | restart | status | logs [-f] | update | open | config get [key] | config set <key> <value> | apikey create|list|revoke | user add|password | serve` (no argument: serve). `install` writes `nocturne-server.service` through `core/user_unit.rs` (`SERVER`: `ProtectSystem=full`, `RuntimeDirectory=nocturne` with `RuntimeDirectoryPreserve=yes`, no home confinement because library folders can be anywhere), enables and starts it, optionally turns on lingering (the keyring stays locked until login, so stored keys are unavailable before it), and prints the dashboard address and the first-run setup link. `update` only reports a newer version for now; installing stays with the package manager or the app.

Upgrade from 0.7.0: the `nocturne-gateway.service` user unit keeps working until the server's gateway module has imported its data (settings, users, tokens, id map; the gateway worker's import writes `$XDG_DATA_HOME/nocturne/server/gateway-imported`). `install` then disables and removes the old unit; before that it says so and leaves it.

## 8. Tests

- Unit: decision rules, import planning and placing (hardlink, refusal of foreign files, upgrade order, escapes), naming, store, config sections, unit file text (and `systemd-analyze verify`), parser adapters, CLI key handling.
- `tests/api.rs`: setup, login, sessions, API keys, the local admin over the socket and through the CLI client, module gating and config sections over real sockets.
- `tests/flow.rs`: through the REST API with fakes: indexer, libraries, profiles; a movie, an episode, an album and a book from search to import; a failed download (blocklist, next release); a request; a title already on a source.
- `tests/openapi.rs`: every registered route is documented.
- Live: none against real indexers or trackers in CI (no shipped definitions); the torrent engine has its own loopback tests in `crates/torrent`.
