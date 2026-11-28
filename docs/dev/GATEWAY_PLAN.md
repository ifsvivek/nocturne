# Gateway plan (0.7)

Status: plan, no code. Decisions by the maintainer (2026-11-27): `nocturne-gateway` is an optional companion service in this workspace, off by default, controlled from Settings > Gateway. It serves ONE Jellyfin-compatible address that merges all Jellyfin servers and local folders, for the official Jellyfin phone and TV apps and for the maintainer's own third-party app, which signs in with a Jellyfin token and needs both (a) browse and play the merged library and (b) remote control (list sessions, send play, pause, seek and "play this item", across all servers). No torrent features, no add-ons yet (they come in 0.8 and plug in as another source). This document answers the seven questions of the brief and ends with the work split and the open questions.

## 0. Method and what is verified

- Contract baseline: the OpenAPI document served by the throwaway server itself (`GET /api-docs/openapi.json`, Jellyfin 12.4.0 as installed here: 294 paths, 364 operations). Behaviour was probed on the throwaway server (`scripts/test-jellyfin.sh`, 127.0.0.1:8097, started and stopped by this task; nothing else was contacted). jellyfin-web is not installed here (the Arch `jellyfin-server` package ships no web client) and no browser exists on this machine, so "what jellyfin-web requests" could not be recorded. The client column below comes from reading knowledge of jellyfin-web (`apiclient`, `playbackmanager`, `serverNotifications`), the Kotlin SDK (`jellyfin-sdk-kotlin`, used by jellyfin-android and jellyfin-androidtv) and Swiftfin, and is marked as UNVERIFIED until the recording task in section 7 is done.
- Verified on the throwaway server:

| # | Finding | Consequence for the gateway |
|---|---|---|
| V1 | Token forms: `Authorization: MediaBrowser Client=..., Device=..., DeviceId=..., Version=..., Token="..."` works, also with only `Token="..."`; query `ApiKey=<token>` works. `X-Emby-Token`, `X-MediaBrowser-Token`, `X-Emby-Authorization` and query `api_key` all returned 401 on `/Users/Me` and `/UserViews`. | The gateway accepts ALL of these forms from clients (older apps and the maintainer's app may use any), but always talks to servers with the `Authorization` header, never with a query token. Side finding for NOCTURNE itself: `Client::stream_url_with_key` (used for VLC) appends `api_key=`, which this server version rejects on normal routes; check it against 12.x (stream routes themselves answered 400 for a fake id with both spellings, so that test was inconclusive). |
| V2 | `POST /Users/AuthenticateByName` returns `AccessToken`, `ServerId`, `SessionInfo`, `User`. | Gateway returns the same shape with its own token and its own ServerId. |
| V3 | WebSocket `GET /socket` upgrades (101) with the `Authorization` header or `?ApiKey=&deviceId=`; with `api_key` it is 403. The server immediately sends `{"MessageType":"ForceKeepAlive","Data":60}`. | Gateway accepts both spellings, sends the same first frame. `SessionsStart` with `"0,1500"` produced no `Sessions` frame within 4 s for a non-admin test user (unverified why); the worker must re-check with a controlling user. |
| V4 | `GET /Sessions` for a non-admin returned `[]` until the client posted capabilities; after `POST /Sessions/Capabilities/Full` (204) the session was listed with `SupportsRemoteControl: false`. `SupportsRemoteControl` only turns true while a WebSocket is connected. | The gateway computes `SupportsRemoteControl` from its own socket table; a session without a socket is listed but not controllable. |
| V5 | `POST /Items/{id}/PlaybackInfo` returns `MediaSources` (each with `Path`, `RequiredHttpHeaders`, `TranscodingUrl`) and `PlaySessionId`. `TranscodingUrl` is root-relative, uses the DASHED item id in the path (`/videos/xxxxxxxx-xxxx-.../master.m3u8`), the undashed id in `MediaSourceId`, and contains `ApiKey=<the caller's token>` and `PlaySessionId`. The HLS master playlist's `main.m3u8?...&MediaSourceId=...&PlaySessionId=...&ApiKey=<token>` lines repeat the token. | A pass-through proxy would leak the server token to clients through playlists. The gateway must rewrite `TranscodingUrl` and every HLS playlist (ids in both dashed and undashed form, case-insensitive; token replaced by the client's own gateway token), and strip `Path`. |
| V6 | Legacy routes `/Users/{userId}/Items`, `/Views`, `/Items/Resume` still resolve (a bogus user id gave 400, not 404) although they are absent from the OpenAPI document; `/Videos/{id}/master.m3u8` also exists although absent from the document. | The gateway serves both the modern (`/Items`, `/UserViews`, `/UserItems/Resume`) and the legacy per-user forms as aliases of one handler. The OpenAPI document alone is not a complete contract. |
| V7 | `/System/Info/Public`, `/Branding/Configuration`, `/Users/Public`, `/QuickConnect/Enabled` answer without auth; item images (`/Items/{id}/Images/...`) are anonymous. | Same for the gateway (Users/Public returns an empty list, see section 5); images are served with the client's token if it sends one, anonymous only for loopback. |

## 1. The Jellyfin API subset

Client key: W = jellyfin-web / jellyfin-androidtv / jellyfin-android class of full clients (Kotlin SDK, Swiftfin behave alike), M = the maintainer's app (token sign-in; needs browse, play, remote control), R = remote control flows, X = only some clients. Handling: G = answered by the gateway itself, P = proxy to the item's origin server with id rewriting, F = fan-out to all servers and merge, L = local-library item built from `nocturne-local`. Everything not in this table answers `404` (an allow-list router; the admin, user-management, library, plugin, task, device, log, backup and Live TV families are never exposed).

| Method | Path | Purpose | Client | Handling |
|---|---|---|---|---|
| GET | `/System/Info/Public` | name, version, id, `ProductName: "Jellyfin Server"`, `StartupWizardCompleted: true`; the first call every client makes | W M | G |
| GET, POST | `/System/Ping` | liveness | W | G |
| GET | `/System/Info` | version check by some clients (with auth) | X | G (reduced) |
| GET | `/Branding/Configuration`, `/Branding/Css`, `/Branding/Css.css` | login disclaimer, custom CSS; empty | W | G |
| GET | `/Users/Public` | login picker; empty list (no user enumeration) | W | G |
| POST | `/Users/AuthenticateByName` | login with the gateway's own user, returns token | W M | G |
| GET | `/QuickConnect/Enabled`, POST `/QuickConnect/Initiate`, GET `/QuickConnect/Connect?secret=`, POST `/Users/AuthenticateWithQuickConnect`, POST `/QuickConnect/Authorize?code=` | pairing without typing a password; approval comes from Settings > Gateway or from an already signed-in gateway client | W M | G |
| GET | `/Users/Me`, `/Users/{userId}` | current user, policy summary (`IsAdministrator: false`, `EnableRemoteControlOfOtherUsers`, `EnableMediaPlayback`, ...) | W M | G |
| GET, POST | `/DisplayPreferences/{id}` | client layout prefs; stored by the gateway per user and client | W | G (own file) |
| POST | `/Sessions/Logout` | end this gateway token | W | G |
| GET | `/UserViews` and legacy `/Users/{id}/Views` | merged libraries: one view each for Movies, Shows, Music (and Photos/Books when present), optional per-source views | W M | F (G for the list) |
| GET | `/Items` and legacy `/Users/{id}/Items` | browse, filter, sort, page, search by `SearchTerm`, `Ids`, `ParentId`, `IncludeItemTypes`, `Recursive`, `Fields`, `Filters`, `Genres`, `Years`, `NameStartsWith`, `IsPlayed`, `IsFavorite`, `SortBy`, `SortOrder`, `StartIndex`, `Limit` | W M | F merged; P when `ParentId`/`Ids` name one origin |
| GET | `/Items/{id}` and legacy `/Users/{id}/Items/{id}` | detail DTO | W M | P (merged entry: preferred copy plus union of sources) |
| GET | `/Items/Latest`, `/UserItems/Resume`, `/Shows/NextUp`, `/Items/Suggestions`, `/Movies/Recommendations` | Home rows | W M | F |
| GET | `/Shows/{seriesId}/Seasons`, `/Shows/{seriesId}/Episodes`, `/Items/{id}/Similar`, `/Items/{id}/Ancestors`, `/Items/{id}/SpecialFeatures`, `/Items/{id}/LocalTrailers`, `/Items/{id}/ThemeMedia`, `/Items/{id}/Intros` | detail page content | W M | P or F for merged series (union of seasons and episodes) |
| GET | `/Genres`, `/MusicGenres`, `/Studios`, `/Persons`, `/Artists`, `/Artists/AlbumArtists`, `/Items/Filters`, `/Items/Counts` | filter lists | W | F (names merged) |
| GET | `/Search/Hints` | search box | W | F |
| GET, HEAD | `/Items/{id}/Images/{type}[/{index}]` (and the long `.../{tag}/{format}/{maxWidth}/...` form), `/Artists|Genres|Persons|Studios/{name}/Images/...`, `/Users/{id}/Images/{type}` | posters, backdrops, logos, avatars | W M | P (disk-cached) or L |
| POST | `/Items/{id}/PlaybackInfo` (GET too) | which sources and which of direct play, direct stream, transcode; returns `MediaSources`, `PlaySessionId` | W M | P with rewrite (V5) or L |
| GET, HEAD | `/Videos/{id}/stream`, `/Videos/{id}/stream.{container}`, `/Audio/{id}/stream[.{container}]`, `/Audio/{id}/universal` | direct play, direct stream, audio; Range supported | W M | P streaming, L file with Range |
| GET | `/Videos/{id}/master.m3u8`, `/main.m3u8`, `/hls1/{playlistId}/{segment}`, `/Videos/{id}/live.m3u8`, `/Audio/{id}/master.m3u8` | HLS transcode (server transcodes; gateway only forwards); not in the OpenAPI document (V6) | W M | P with playlist rewrite |
| GET | `/Videos/{id}/{sourceId}/Subtitles/{index}/Stream.{fmt}` (and `/{startTicks}/` form, `subtitles.m3u8`), `/Videos/{id}/{sourceId}/Attachments/{n}`, `/Videos/{id}/Trickplay/{width}/...`, `/MediaSegments/{id}` | subtitles, fonts, scrub thumbnails, skip-intro markers | W M | P |
| GET | `/Items/{id}/Download` | download button | X | P |
| POST | `/Sessions/Playing`, `/Sessions/Playing/Progress`, `/Sessions/Playing/Stopped`, `/Sessions/Playing/Ping` | playback reporting | W M R | G always (session table); forwarded to the origin server only if `allow_server_writes` and `sync_progress` (WRITES.md rules); otherwise kept in the gateway overlay (section 2) |
| POST, DELETE | `/UserPlayedItems/{id}`, `/UserFavoriteItems/{id}`; GET, POST `/UserItems/{id}/UserData` | watched, favourite | W M | G overlay plus forward to origin only with `allow_server_writes` |
| POST | `/Sessions/Capabilities`, `/Sessions/Capabilities/Full` | a client declares it is controllable (playable types, supported commands) | W R | G (own session table) |
| GET | `/Sessions` (query `controllableByUserId`, `deviceId`, `activeWithinSeconds`) | list devices | W M R | F: gateway sessions plus every server's sessions, merged (section 4) |
| POST | `/Sessions/{id}/Playing` (`playCommand=PlayNow|PlayNext|PlayLast`, `itemIds`, `startPositionTicks`, `mediaSourceId`, `audioStreamIndex`, `subtitleStreamIndex`, `startIndex`) | "play this item" on a device | W M R | G (native session) or forwarded (upstream session) |
| POST | `/Sessions/{id}/Playing/{command}` (`PlayPause`, `Pause`, `Unpause`, `Stop`, `Seek` with `seekPositionTicks`, `NextTrack`, `PreviousTrack`, `Rewind`, `FastForward`) | transport control | W M R | same |
| POST | `/Sessions/{id}/Command` (body `GeneralCommand`) and `/Sessions/{id}/Command/{command}` | SetVolume, Mute, Unmute, ToggleMute, SetAudioStreamIndex, SetSubtitleStreamIndex, DisplayMessage, GoHome, GoToSettings, Move*, Select, Back, ... | W M R | same; only the list NOCTURNE already supports (`remote.rs`) plus the navigation ones |
| POST | `/Sessions/{id}/Message`, `/Sessions/{id}/System/{command}`, `/Sessions/Viewing`, `/Sessions/{id}/Viewing`, `/Sessions/{id}/User/{userId}` | message box, legacy forms | X | `Message` same as DisplayMessage; the rest 404 |
| GET (upgrade) | `/socket?ApiKey=&deviceId=` (also `/websocket` that the SyncPlay client in this repo tries as a fallback) | push channel: `ForceKeepAlive`/`KeepAlive`, `Sessions`, `Play`, `Playstate`, `GeneralCommand`, `LibraryChanged`, `UserDataChanged`; needed by remote control (a device is controllable only while it holds a socket, V4) and by clients that refresh Home on `LibraryChanged` | W M R | G |
| POST | `/ClientLog/Document` | client crash logs | X | G (accept, drop) |
| GET | `/Playback/BitrateTest` | network test before playback | W | G (random bytes of the requested size, capped) |
| UDP | port 7359 `"Who is JellyfinServer?"` | LAN discovery used by the Android and TV apps | W | G (optional, LAN mode only) |

Not in v1 (404): SyncPlay (it is per server and stays in NOCTURNE), Live TV and DVR (0.9), playlists and collections writes, admin, user management, library and plugin routes, image upload, subtitle search and download, lyrics.

Sources to cite in code comments and tests: the 12.4.0 OpenAPI document above (store a copy of the route list, not the 1.9 MB file, in `crates/gateway/tests/fixtures/`), the Jellyfin server source (`Jellyfin.Api/Controllers/SessionController.cs`, `ItemsController.cs`, `DynamicHlsController.cs`, `MediaInfoController.cs`; `Emby.Server.Implementations/Session/SessionManager.cs`, `SessionWebSocketListener.cs`; `MediaBrowser.Model/Session/*` for `PlayRequest`, `PlaystateRequest`, `GeneralCommand`, `SessionInfoDto`), and the existing `crates/core/src/remote.rs`, which already encodes the same `Play`, `Playstate` and `GeneralCommand` shapes from the server source.

## 2. Merged ids, duplicates, user data

**Id format.** Clients parse ids as GUIDs (the Kotlin SDK type is `UUID`), so every gateway id must be 32 hex characters; a server id plus an item id (2 x 128 bits) cannot be encoded reversibly in 128 bits. Recommendation: deterministic hash plus a persisted reverse map.
- `gid = first 16 bytes of SHA-256("nocturne-gw-v1" | key)`. `key` for a single-origin item is `source_uuid:upstream_id` (the source's stable id is a UUID stored in its profile, NOT the url or the u32 `SourceId`, so a changed server address or reorder keeps the ids); for a local item `local:<LocalId>`; for a merged title `merge:<imdb|tmdb|tvdb>:<kind>:<id>` (episodes `merge:<series ids>:S:E`). Stable across restarts and across which server happens to be the preferred copy.
- Reverse map `gid -> members [(source_uuid, upstream_id)]` kept in memory and persisted in `$XDG_DATA_HOME/nocturne/gateway/ids.json` (versioned, atomic write, rebuilt lazily as items stream through the rewriter; entries not seen for 90 days are dropped). A client holding an id older than the map answers 404 and refetches; clients tolerate that.
- Media source ids and `PlaySessionId`: same scheme with key `gid:upstream_media_source_id`; session ids: `session:<source_uuid>:<upstream_session_id>`.
- JSON bodies are rewritten by walking the value and replacing, in an explicit field list (`Id`, `ParentId`, `SeriesId`, `SeasonId`, `AlbumId`, `ChannelId`, `ItemId`, `DisplayPreferencesId`, `PrimaryImageItemId`, `ParentBackdropItemId`, `ParentLogoItemId`, `People[].Id`, `ArtistItems[].Id`, `GenreItems[].Id`, `MediaSources[].Id`, `UserId`, `ServerId`; image tags and `ChapterImageTags` stay as they are and `AccessToken` is never forwarded), not by regex over all 32-hex strings (image tags and ETags look the same). Strip `Path`, `MediaSources[].Path`, `RequiredHttpHeaders` that carry server tokens, `RemoteEndPoint`.
- Path and query ids in requests are translated the other way (dashed and undashed, case-insensitive); an unknown id is a 404 with a body clients accept.

**Same title on several servers.** Reuse NOCTURNE's rules (`crates/core/src/merge.rs`, `versions.rs`; SOURCES.md): merge only on strong identity (provider ids; episodes by series ids plus season and episode), never on title alone. One gateway item per merged title: metadata from the preferred copy (profile priority, same order as the app's sources list), `MediaSources` = the union of every copy's sources, each source `Name` prefixed with its origin ("Server A · 4K HEVC") so any client's version picker doubles as the source picker, and the client-chosen `MediaSourceId` decides which server is used. Series merge the same way; `Seasons` and `Episodes` of a merged series fan out to all members and merge episodes by (season, episode). Items without strong identity stay separate (possible duplicates are not merged). A local copy is just another member with a synthetic `MediaSource` (`Protocol: File`, direct play only).

**List endpoints (the hard part).** Recommendation for v1: scatter-gather with over-fetch and a short result cache, not a persistent catalog index.
- Each query is sent to every online server with the same filters and sort, `StartIndex=0`, `Limit=start+limit`, `Fields=ProviderIds,...`; results are merge-sorted by the requested key (SortName, DateCreated, PremiereDate, ProductionYear, CommunityRating, Runtime, PlayCount, DatePlayed, Random), de-duplicated by merge key, then sliced. Local items answer from the in-memory index. `TotalRecordCount` is the sum minus the duplicates seen so far (approximate on deep pages; documented).
- Cache the per-server pages for 30 s keyed by (user, normalised query) so a client paging through 100-item pages does not refetch; refresh on the servers' own `LibraryChanged` socket messages (one gateway socket per server, as `remote.rs` already does).
- Ceiling and upgrade path (marked `ponytail:` in code): deep offsets cost O(offset) per server; if real libraries hit that, replace with a persisted light index (id, type, sort name, year, dates, provider ids, genres, played) refreshed by paging `/Items`, and answer all list endpoints from it. Do this only after measuring.

**User data (played, resume, favourite).** The gateway user is one identity; each server has its own account that NOCTURNE already signs in with.
- Read: a merged item's `UserData` is computed from all copies (each upstream `UserData` is already in its DTO) plus the gateway overlay. `Played` = any copy played (or the overlay says so); `PlaybackPositionTicks` = the position of the copy with the newest `LastPlayedDate`, scaled by the runtime ratio when copy lengths differ (this is exactly the existing rule in `crates/core/src/watched.rs`, reuse it); `IsFavorite` = any; `PlayCount` = max.
- Write: the gateway overlay (`gateway/userdata.json`, keyed by gid, atomic) always records what clients report, so the gateway works with writes off. Forwarding follows WRITES.md and nothing more: only with `allow_server_writes`; progress only with `sync_progress`; to the item's own server only; played/favourite on all copies only with `sync_watched`, one hop; never with a developer API-key source; local items write the existing local `state.json`.

## 3. Streams: proxy, not redirect

Recommendation: always proxy through the gateway. A redirect (302 to the item's own server) is rejected as the default.

| Aspect | Proxy (recommended) | Redirect |
|---|---|---|
| Server token | stays inside the gateway; the client only holds its gateway token | the client must be handed `ApiKey=<server token>` in the URL (V5 already shows servers embed it): leaks a long-lived credential to every TV and phone and into their logs |
| Reachability | one address and one forwarded port; servers can sit behind VPN, Tailscale, another LAN | the client must reach every server directly; breaks the "remote over HTTPS" case and any server that is only reachable from the gateway host |
| Local files | served by the gateway with Range | impossible |
| HLS | playlists rewritten (V5): ids and token replaced, relative segment URLs kept | playlist contains server URLs and tokens |
| Bandwidth | traffic passes the gateway host twice for a remote server (down and up) | direct, lowest |
| Complexity | streaming body copy, Range, HEAD, cancellation | trivial |

Implementation notes: forward `Range`, `If-Range`, `Accept-Encoding: identity`; answer 206 with `Content-Range`, `Accept-Ranges`, `Content-Length`; stream the body without buffering (hyper body to hyper body, backpressure from the client); drop the upstream request when the client disconnects (an abandoned transcode is stopped by the server's own timeout and by the forwarded `Sessions/Playing/Stopped` when allowed); reuse the per-source request gate (semaphore) but give streams their own larger limit so browsing is never starved; `Cache-Control: no-store` on media. HLS playlists (`.m3u8`, detected by content type and extension) are buffered (small, capped at 1 MiB) and rewritten; segments (`.ts`, `.mp4`, `.m4s`, `.vtt`) stream. Local files: `tokio::fs` + Range + `Content-Type` by extension, read-only open (SOURCES.md rule 3), no transcoding in v1 (direct play only, documented limitation). Optional later setting "Direct to server on the same network" is explicitly out of scope.

Disk cache (Settings > Gateway > Caching, default none): applies to images and small metadata responses only, reusing `core/imgcache.rs`'s size-capped LRU; never to video.

## 4. Remote control across servers

Two kinds of device appear in the merged `GET /Sessions`:

| Kind | What it is | Control path | Cross-server "play this item" |
|---|---|---|---|
| Native gateway session | a client signed in to the gateway (official app, the maintainer's app, jellyfin-web pointed at the gateway) with a `/socket` open and capabilities posted | the gateway delivers `Play`, `Playstate`, `GeneralCommand` messages over that client's own gateway socket; no server is involved | works for any item: the client resolves gateway ids through the gateway |
| Upstream session | a device signed in to one real server (a phone app talking to server A directly), seen in that server's `GET /Sessions` | the gateway forwards `POST /Sessions/{realId}/Playing...` to that server with the server account's token | only items of that same server (merged ids resolve to the member on that server); otherwise answer 400 with a clear message (`DisplayMessage`-style text), never guess |

- Merged list: native sessions (gateway ids) plus each online server's sessions (`controllableByUserId` = that account, `activeWithinSeconds` as asked), session `Id` rewritten, `UserId`/`UserName` set to the gateway user, `NowPlayingItem` and `NowPlayingQueue` rewritten (ids, `Path` stripped), `ServerId` = gateway. NOCTURNE itself with "Allow remote control from other devices" on is registered on every server (WRITES.md), so it appears once per server with the same `DeviceId`: sessions with equal `DeviceId` and `Client` are collapsed into one gateway session carrying its list of (server, real session id); a command for it goes to the member whose server owns the item (play) or to the member currently playing (transport). NOCTURNE needs no special case, it is just a device; if the app is also connected to the gateway as a native client (it is not, see open question 9) it would not be listed twice.
- Poll cost: `GET /Sessions` fans out per request with a 2 s cache. The `Sessions` WebSocket push (after `SessionsStart`) is produced by the gateway diffing the merged list every 1.5 s only while at least one client asked for it (no polling when nobody listens, per the footprint rules).
- WebSocket messages the gateway sends to clients: `ForceKeepAlive` (Data 60) on connect; `Sessions` (Data = merged list) after `SessionsStart`; `Play`, `Playstate`, `GeneralCommand` to the target native session (bodies shaped like `PlayRequest`, `PlaystateRequest`, `GeneralCommand` as in `core/remote.rs`); `LibraryChanged` and `UserDataChanged` when the underlying servers or the overlay change (coalesced, 5 s). Messages it accepts: `KeepAlive`, `SessionsStart`, `SessionsStop`, others ignored. Remote-control messages arriving from upstream servers (for the app's own listening) are unrelated and stay in `core/remote.rs`.
- Gating (two tiers, mirrors WRITES.md):
  - new setting `gateway_remote_control` (default off): without it `GET /Sessions` returns only the caller's own session, the session command routes answer 403 and no capability is accepted for control. It is a gateway-level feature switch, not a server write.
  - forwarding a command to an upstream session is a server call (WRITES.md's "Remote control" row is already gated by `allow_server_writes`), so it additionally needs `allow_server_writes`; with it off, native sessions stay controllable (nothing leaves the gateway) and upstream sessions are listed read-only (`SupportsRemoteControl: false`). Never in developer API-key mode.
  - A remote command only drives a player (play, transport, volume, tracks, message). The allow-list of `GeneralCommand` names is fixed; no admin action, no write to library data, no answer to a confirmation dialog, and never `Confirmed`/`UserConfirmed` (those stay minted only in the app's dialogs). Commands are rate-limited per token (20 per second).
  - Playback reports of a native session are the "playback reporting" row above, not remote control.

## 5. Process model and security

**Process.** A separate binary `nocturne-gateway` (crate `crates/gateway`), not a thread in the app.
- Why: the app's executor, keyring prompts and tokio bridge stay unaffected by a network-facing listener (a panic or a stuck client cannot freeze the UI, a crash cannot take the window down, the same argument as the external mpv player), the service can run with the window closed, it can be sandboxed independently, and its dependencies (axum, argon2) never grow the app binary (footprint rules in PLATFORMS.md: off means the process does not exist, zero idle cost).
- Lifecycle: Settings > Gateway > On makes the app spawn the binary as a child (found next to the app binary, then `PATH`; the Linux packages ship it in the same package). When the app quits it stops the child. Opt-in setting "Keep running when NOCTURNE is closed" (default off) installs and enables a systemd user unit (`~/.config/systemd/user/nocturne-gateway.service`, written by the app, removed on opt-out, `Restart=on-failure`, hardening `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=read-only` plus `ReadWritePaths` for its own data dir, `PrivateTmp=yes`, `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX`). User unit starts at login, which is also when the Secret Service is available; `loginctl enable-linger` (start before login) is not supported because the keyring would be locked. The service manager is a platform seam: macOS (launchd) and Windows (a login task) come in the platform phase; until then "Keep running" is Linux only.
- Single instance and control: a lock file plus a unix socket `$XDG_RUNTIME_DIR/nocturne/gateway.sock` (mode 0600, directory 0700, peer uid checked with `SO_PEERCRED`). The app uses it for status, reload (after settings change), stop, listing and revoking tokens, approving a Quick Connect code, regenerating the gateway password. The protocol types live in `crates/core/src/gateway.rs` with the settings so the app does not depend on the gateway crate (and so not on axum). Windows later: a named pipe, same seam as the player IPC.
- Config: the gateway reads `settings.toml` and `sources.toml` (existing, no secrets) through `nocturne-core`, plus its own files under `$XDG_DATA_HOME/nocturne/gateway/` (`ids.json`, `userdata.json`, `tokens.json`, `users.toml`, all 0600 in a 0700 directory).

**Server tokens.** The gateway reads the same keyring entries as the app (`nocturne_session::load_account`, same OS user, same `NOCTURNE_KEYRING_SERVICE` override for tests). Tokens live only in process memory; they are never written to the gateway's files, never put in a response (V5: stripped from `TranscodingUrl`, playlists, session objects), never logged (the jellyfin client already strips URLs from errors; add a logging filter that redacts `ApiKey=`, `Token="..."`, `api_key=`), and requests to servers carry them in the `Authorization` header only. Core dumps off (`RLIMIT_CORE=0`, `PR_SET_DUMPABLE=0` through `libc`, already a core dependency on unix). Keyring locked or missing: that source shows `SignedOut`/"keyring locked" in status; the others keep working. A server answering 401 sets that source signed out in the gateway, and the app is told over the control socket so its normal re-sign-in flow runs.

**Gateway users and sign-in (never a server password).**
- v1: one gateway user, default name `nocturne`, with a generated 24-character passphrase shown once in Settings (regenerate any time; stored only as an Argon2id hash in `users.toml`). Clients use `AuthenticateByName`, or Quick Connect: the client shows a code, the user approves it in Settings > Gateway (or from any already signed-in gateway client through `/QuickConnect/Authorize`). More users later if the maintainer wants per-person watched state (open question 2).
- Tokens: 256 random bits (`getrandom`, already a dependency), returned once; only SHA-256 stored with device id, device name, created and last-used time, in `tokens.json`; revocable one by one from Settings ("Signed-in devices"); a token is also revoked on password regeneration. Tokens in query strings (`ApiKey`) are accepted because clients and players need them for images and streams; they are scrubbed from logs and a `Referrer-Policy: no-referrer` header is set.
- Policy returned to clients: `IsAdministrator: false`, no admin routes exist at all.

**Binding and exposure.**

| Setting | Bind | Extra rules |
|---|---|---|
| This computer only (default) | `127.0.0.1` and `::1` | none |
| Local network | `0.0.0.0` / `::` | peers outside loopback, RFC 1918, `fc00::/7`, link-local and CGNAT `100.64.0.0/10` (Tailscale) are refused at accept time, whatever the router does; cleartext HTTP, say so in the UI |
| Remote over HTTPS | `0.0.0.0` / `::` | refuses to start without a certificate and key path (`tokio-rustls`, rustls already in the tree; PEM through `rustls-pki-types`); no ACME, no self-signed generation, no UPnP port mapping; TLS 1.2+; HSTS; stricter limits. Documentation recommends a private overlay (Tailscale or WireGuard) or the user's own reverse proxy instead of opening a router port. `X-Forwarded-For` is trusted only from a configured proxy address |

**Limits.** Failed logins: 5 per 10 minutes per (IP, username), then 429 with exponential back-off, plus a global ceiling; request line and headers 16 KiB, body 1 MiB (the gateway has no upload route), header read timeout 10 s, idle timeout 60 s except media; 256 concurrent connections, 64 per IP, 4 sockets per token; Quick Connect: 3 open codes, 5 minutes; command rate per token as above; per-upstream stream concurrency cap; a `Range` request longer than the item is clamped.

**Threat model (short).**

| Threat | Mitigation |
|---|---|
| Someone on the LAN or the internet reaches the port | default loopback; LAN mode accepts private peers only; remote mode needs TLS; auth on everything but the public info routes; login rate limit; no user enumeration |
| A client steals a server token | tokens never leave the process (V5 rewriting, redaction, no redirect streams) |
| A compromised client or token abuses the gateway as a read/write proxy to servers | the allow-list router (no generic proxy), writes only per WRITES.md gating, no admin routes, `IsAdministrator: false`, one-by-one token revoke |
| SSRF: a crafted path or query makes the gateway fetch somewhere else | upstream URLs are built only from the stored server base URL plus a fixed route template; ids are validated (32 hex, mapped) before use; never follow redirects to another host; strip `Host`, `X-Forwarded-*`, `Authorization` from client requests before forwarding |
| Path traversal on local files | local files are addressed only by id through the index; never a client-supplied path; canonical path must remain under a configured root; read-only open |
| Header or JSON injection, oversized bodies, slowloris | strict body and header limits, timeouts, typed JSON parsing with size caps, no reflection of untrusted text into headers |
| Log or crash leaks secrets | redaction filter, no `Debug` of config structs with secrets, core dumps off, panic = unwind keeps the process up |
| Another local user on the machine | sockets 0600 in a 0700 dir with peer-uid check; files 0600; loopback listener is reachable by every local user, hence the gateway password applies on loopback too |
| Malicious media metadata (item names with markup, huge arrays) | the gateway only passes JSON through typed paths with caps (page size clamp 500, nesting depth limit); it renders nothing |

**Security review before release (checklist for the maintainer's review).** Authentication and token handling (hashing, comparison in constant time, entropy, revocation, query-token logging); rate limit bypass (IPv6 prefix rotation, `X-Forwarded-For` spoofing); the allow-list (every route has an explicit auth and gating annotation; a test lists routes and fails on an unannotated one); id mapping (no reflective access to other users' data, unknown id behaviour); request forwarding (header allow-list, redirect policy, SSRF cases, response header filtering and `Set-Cookie` dropped); HLS and JSON rewriting completeness (no server token or server URL or file path in any response, checked by a test that greps recorded responses for the known token); write gating parity with WRITES.md (a table-driven test: with the setting off, every forwarded POST is refused); WebSocket (origin handling, message size caps, per-token limits); TLS config and cert loading errors; process isolation (systemd unit hardening, file modes, socket peer check); dependency audit (`cargo deny`, new crates listed in THIRD_PARTY_NOTICES); fuzzing of the id and playlist rewriters and the `Authorization` header parser; a negative test run from a LAN host against loopback-only mode; secrets scan of logs produced by a full test run.

## 6. Crates, dependencies, settings, tests

**Layout.**

| Where | What |
|---|---|
| `crates/gateway` (`nocturne-gateway`, lib plus bin `nocturne-gateway`) | everything network-facing: `server.rs` (router, allow-list, middleware, limits), `auth.rs`, `idmap.rs`, `rewrite.rs` (JSON and HLS), `lists.rs` (fan-out, merge-sort, de-dup, cache), `items.rs`, `images.rs`, `stream.rs`, `playback.rs`, `local_dto.rs` (local item to `BaseItemDto`), `userdata.rs`, `sessions.rs`, `ws.rs`, `discovery.rs`, `control.rs`, `tls.rs` |
| `crates/core/src/gateway.rs` | pure: gateway settings struct, status, the control-socket protocol, address and QR payload; unit-tested; no axum |
| `crates/core/src/settings.rs` | the Settings > Gateway rows (below) |
| `crates/jellyfin/src/raw.rs` | GET-only forwarding primitive `get(path_and_query, headers) -> Response` using the existing client, headers and request gate (keeps `tests/readonly.rs` true) |
| `crates/jellyfin-user/src/raw.rs` | POST/DELETE forwarding for the gated writes and session commands |
| `crates/app/src/app/gateway.rs` | spawn and stop the child, status polling over the control socket, the Settings page, QR code, devices list |

Reuse instead of rewriting: `nocturne-jellyfin` (clients, headers, request gate, error scrubbing), `nocturne-jellyfin-user` (reporting, played/favourite, session calls), `nocturne-local` (index, ids, state), `nocturne-session` (keyring, paths), `nocturne-core` (`Profile`, `merge`, `versions`, `watched`, `imgcache`, `settings`, `remote` command shapes, `bridge`). The gateway does NOT map Jellyfin DTOs through core's `Media` (it is UI-shaped and lossy: no `MediaStreams`, people, chapters); items pass through as JSON with id rewriting, and only local items are synthesised. `nocturne-core` pulls `zbus`, `wuff` and `ttf-parser` that a headless service does not need; accept it in v1 (dead code is stripped by LTO, the gateway is a separate binary), measure the binary, and extract a small `catalog` crate later only if the size or the review surface becomes a problem.

**HTTP server crate.**

| Option | Verdict |
|---|---|
| `axum` 0.8 (hyper 1, tower) with features `http1`, `tokio`, `ws`, `query`, `json` only | recommended: hyper, hyper-util, tower, tokio-tungstenite 0.30 and tokio-rustls are already in `Cargo.lock` (through reqwest and the SyncPlay client); new crates are about 8 small ones (axum, axum-core, matchit, http-body-util, sync_wrapper, tower-layer, tower-service, serde_path_to_error); streaming bodies, Range, WebSocket upgrade and middleware come ready |
| bare `hyper` 1 plus a hand-written router | smallest, but we would hand-write routing, extractors and the upgrade; more code to review than the 8 crates it saves |
| `tiny_http` / sync servers | no WebSocket, awkward streaming and cancellation; rejected |
| `actix-web`, `warp`, `rocket` | a second runtime or big trees; rejected |

Other new dependencies: `argon2` (RustCrypto, pure Rust, for the gateway password), `qrcode` in the app only (pure Rust, small, drawn as an Iced canvas), `rustls-pki-types` PEM reader (already in the tree). Discovery uses `tokio::net::UdpSocket`. Each must pass `cargo deny check licenses`, then `scripts/gen-notices.py > THIRD_PARTY_NOTICES.md`. Footprint: the app binary only grows by the settings page and the QR code (record in PERF.md); the gateway binary is measured separately; with the gateway off nothing runs.

**Settings > Gateway rows** (additions to `Settings`, serde defaults, `rows()`/`cycle()` index = arm, a unit test; any write path also gated by `allow_server_writes`; settings version bump only if a default changes meaning):

| Row | Values | Default |
|---|---|---|
| Gateway | Off / On | Off |
| Status | Online (address) / Stopped / error text (read only) | |
| Allow connections from | This computer only / Local network / Remote over HTTPS | This computer only |
| Port | number | 8099 (8096 is Jellyfin's own; discovery on UDP 7359 is skipped when that port is taken) |
| HTTPS certificate and key | two file paths, only used for Remote | none |
| Caching | None / size limit (256 MiB, 1 GiB, 4 GiB) | None |
| Address | the URL, Copy, QR code | |
| Sign-in | user name, Show or regenerate password, Approve Quick Connect code, Signed-in devices with Revoke | |
| Keep running when NOCTURNE is closed | Off / On (Linux) | Off |
| Allow remote control | Off / On (`gateway_remote_control`) | Off |
| Per-server library views | Off / On | Off (only the unified Movies, Shows, Music views) |
| Announce on the local network | Off / On (UDP discovery, LAN mode) | On in LAN mode |

Add the gateway rows to WRITES.md (remote control through the gateway, playback reporting through the gateway), SOURCES.md (the gateway is a front for the catalog, not a source; the app refuses to add a source whose `ServerId` equals its own gateway's, to prevent a loop), ARCHITECTURE.md and the safety rules in CLAUDE.md/AGENTS.md (the gateway never mints `Confirmed` or `UserConfirmed`).

**Tests.**

| Layer | How |
|---|---|
| Unit | id stability and dashed/undashed round trip; JSON rewrite on fixtures recorded from the throwaway server (assert no token, no `Path`); HLS and `TranscodingUrl` rewrite from the V5 shapes; merge-sort, de-dup and paging; auth, rate limit, Quick Connect state; Range parsing; write-gating table; allow-list annotation test |
| Contract | validate gateway responses against the 12.4.0 OpenAPI schemas (`components.schemas`) for every route in the table (a small `jsonschema` check in a dev-dependency or a script), so shape drift is caught without a real client |
| Live (ignored by default, throwaway servers only) | extend `scripts/test-jellyfin.sh` with an instance number (`start 2` = port 8098, state `/tmp/nocturne-jf-test-2`, its own `test.env`) and `make-demo-library.sh` with an overlapping second library (same provider ids, different quality) so merging is real; extend `TestServerGuard` to allow exactly ports 8097 and 8098 under `/tmp/`; tests start the gateway in-process on an ephemeral loopback port against both |
| Free conformance oracle | point `nocturne-jellyfin` and `nocturne-jellyfin-user` themselves at the gateway: everything NOCTURNE already calls must work (run the existing read live tests with the gateway URL) |
| Automatable clients | a scripted client using the same crates for the remote-control flow (native session over `/socket`, `Play`/`Playstate` arrive); `jellyfin-mpv-shim` (Python, registers as a cast target over the socket) inside `scripts/headless-session.sh` as a real third-party controllable device; `jellyfin-web` built from source and loaded in headless Chromium or Firefox with Playwright is the best automated full-client check but needs a browser installed (none here), so it is an optional, documented, manual-first task |
| Not automatable | the official Android, Android TV and iOS apps: a manual checklist run by the maintainer on real devices (login, Home rows, play direct, play transcode, resume, cast target list) |
| Recording task | with a logging reverse proxy in front of the throwaway server and jellyfin-web or the Android app on a device, record the real request set per client and diff it against the table in section 1; this closes the UNVERIFIED column |

## 7. Work split and estimate

Master defines first (half a day, no code): the `State` struct and the module boundaries below so workers do not touch each other's files.

| Worker | Owns (files) | Scope | Needs from others |
|---|---|---|---|
| A: server, auth, ids, process | `crates/gateway/{Cargo.toml, src/main.rs, lib.rs, server.rs, state.rs, auth.rs, idmap.rs, config.rs, control.rs, discovery.rs, tls.rs}`, `crates/core/src/gateway.rs`, systemd unit writer, `packaging/` entries | skeleton that compiles first (day 1) with the router and middleware, the public routes, users, tokens, Quick Connect, rate limits, binding modes, TLS, control socket, id map | none; publishes `routes()` registration points for B and C |
| B: library, streams | `crates/gateway/src/{items.rs, lists.rs, rewrite.rs, images.rs, stream.rs, playback.rs, local_dto.rs, userdata.rs}`, `crates/jellyfin/src/raw.rs`, `crates/jellyfin-user/src/raw.rs` | views, items, Home rows, merge and paging, detail pass-through, images, PlaybackInfo, direct streams, HLS rewrite, local items, user data merge and overlay, gated progress forwarding | A's `State` and `idmap` interface (stub at start) |
| C: sessions, UI, tests, docs | `crates/gateway/src/{sessions.rs, ws.rs}`, `crates/gateway/tests/`, `crates/app/src/app/gateway.rs`, `crates/core/src/settings.rs`, `scripts/test-jellyfin.sh`, `scripts/make-demo-library.sh`, `crates/jellyfin-admin/src/guard.rs`, the docs listed in section 6 | session table and merge, command routing, WebSocket, Settings > Gateway page with QR and devices, second throwaway instance, live and contract tests, mpv-shim run, docs and STATUS/CHANGELOG | A's state; B's id helpers; reviews B's rewriting with the "no token in any response" test |

Order: A day 1 skeleton, then B and C in parallel; the master integrates in the first week; hardening and the security review pass are a fourth step (sequential, one worker). At most two workers at the same time per the maintainer's limit: run A first alone (about 3 to 4 days of worker time), then B and C together; the security review items are executed by a third pass after B and C land.

Estimate, honest: A 3 to 4 days, B 7 to 10 days (merged lists, HLS and local DTOs are the bulk), C 5 to 7 days, hardening plus review fixes 3 to 5 days, master review and integration spread across them. About 3 to 4 weeks calendar with two workers and the maintainer testing real apps; the biggest risk is client quirks (each official app has habits the OpenAPI document does not show, V6) so expect an extra round after the first real-device test. A read-only first cut (browse, play, no sessions) is reachable in about 2 weeks if the sessions work is deferred; that is the natural cut if 0.7 has to shrink.

## 8. Open questions for the maintainer

| # | Question | Recommendation |
|---|---|---|
| 1 | Which Jellyfin version string to advertise (clients gate features on it)? | the oldest stable line the maintainer's app and the official apps in use support (10.11.x); record it as one constant; the 12.4.0 spec stays the shape reference |
| 2 | One gateway user or several? | one in v1 (all share the same server accounts, so more users would only split the overlay watched state); revisit after use |
| 3 | Transcoding for local files? | no in v1 (direct play only, documented); ffmpeg-based later if needed |
| 4 | Per-server library views next to the unified ones? | off by default (a row), unified views only |
| 5 | "Keep running when NOCTURNE is closed" (systemd user unit)? | yes, opt-in, Linux only; stop the child with the app otherwise |
| 6 | Remote exposure? | bring-your-own certificate only; no ACME, no UPnP; docs steer to Tailscale or a reverse proxy |
| 7 | Default port? | 8099 |
| 8 | With server writes off, should gateway clients still get their own watched and resume state? | yes through the gateway overlay file (clearly "stored in NOCTURNE, not on the server"); the alternative is clients that forget progress |
| 9 | Should the NOCTURNE app itself ever connect to its own gateway? | no; refuse a source whose `ServerId` equals the gateway's (loop and double sessions) |
| 10 | Is the maintainer's third-party app allowed to use `ApiKey` query tokens for streams? | yes (V1: that is the spelling the server accepts); the gateway still accepts the older spellings for official apps |
| 11 | `X-Emby-Token`/`api_key` rejection on 12.x: should NOCTURNE's own `stream_url_with_key` (VLC path) move to `ApiKey`? | yes, check and fix separately from the gateway (side finding V1) |
| 12 | Keep this in 0.7 as one release or cut sessions to 0.7.1? | ship browse and play first if time is short; sessions follow as 0.7.1 |

## 9. Decisions (maintainer and master, 2026-11-27)

1. Advertise 10.11.x as one constant; the 12.4.0 spec stays the shape reference.
2. One gateway user in v1 (password or Quick Connect), devices revocable one by one.
3. No transcoding of local files in v1 (direct play only, documented).
4. Per-server library views: a row, off by default.
5. "Keep running when NOCTURNE is closed": opt-in, Linux only (systemd user unit); otherwise the gateway stops with the app.
6. Remote exposure: bring-your-own certificate only; no ACME, no UPnP; docs point to Tailscale or a reverse proxy.
7. Default port 8099.
8. With server writes off, gateway clients keep watched and resume state in the gateway overlay ("stored in NOCTURNE, not on the server").
9. The app refuses a source whose `ServerId` is its own gateway's.
10. Query tokens (`ApiKey`, and the older spellings) are accepted from clients; servers are always called with the `Authorization` header.
11. Checked by the master on the throwaway server: `/Videos/{id}/stream?static=true` answers 206 with `api_key`, `ApiKey` and the header alike, so the VLC path is not broken; no change now.
12. Browse, play and remote control ship together in 0.7.0.
13. Workers: A alone first (skeleton, auth, ids, process, control socket), then B and C in parallel (at most three workers at a time).

## 10. Security review, 2026-11-27

The section 5 checklist, item by item. "Test" names live in `crates/gateway/tests/security.rs` (S), `tests/gateway.rs` (G), `tests/live_library.rs` (L, throwaway servers) or are unit tests in `src/`. Findings: none needing a code fix; the gaps were missing tests, now added.

| Item | Status | Test or fix |
|---|---|---|
| Tokens: 256-bit random, only SHA-256 stored, lookup by hash (no comparison of secret bytes), revocation, passphrase through Argon2id | ok | `auth::tokens_hash_revoke_and_replace`, `tokens_persist_hashes_only`, `token_table_is_bounded`, `user_password_lifecycle` |
| Tokens in query strings never logged | ok | `log::removes_every_token_spelling`; fuzz-lite `rewrite::junk_never_panics_or_leaks` feeds junk through `redact` |
| Login rate limit: 5 per 10 min per (IP, user), IPv6 grouped by /64, global ceiling | ok | `limits::ipv6_prefix_rotation_shares_a_key`, `global_ceiling_blocks_everyone`, G `login_is_rate_limited_even_for_the_right_password` |
| No `X-Forwarded-For` / `X-Real-IP` trust unless a proxy address is configured | ok | `server::forwarded_for_only_from_the_trusted_proxy`; end to end S `spoofed_forwarded_for_does_not_dodge_the_login_limit` (new) |
| Every route has an explicit access annotation; nothing else is served | ok | G `routes_are_gated`, `unknown_routes_are_404_even_with_a_token` (a route cannot be registered without an `Access`) |
| Id mapping: unknown ids answer 404, crafted ids never reach a server | ok | `idmap::junk_is_not_an_id`; S `crafted_ids_and_paths_never_leave_the_servers_route_templates` (new: traversal, encoded slashes, absolute URLs and backslashes in ids; the mock server saw zero requests) |
| SSRF: upstream URLs only from stored base plus template, redirects not followed, other host never contacted | ok | `Raw::url` unit tests in `crates/jellyfin`; S `a_redirect_from_a_server_is_never_followed` (new: 302 to a second listener, answer 502, listener hit count 0) |
| Client `Host`, `X-Forwarded-*`, `X-Real-IP`, `Authorization`, `Cookie`, `X-Emby-Token` never forwarded; client token never in the upstream URL; only `Range` and validators pass; upstream `Set-Cookie` and other headers dropped | ok | S `nothing_of_the_client_reaches_the_server_and_no_server_header_comes_back` (new; the mock records every header it receives) |
| No server token, server URL or absolute file path in any response | ok | `rewrite` unit tests; L `two_servers_and_a_local_folder_behind_one_address` records every text answer of the live suite and fails on either server's token, URL, the throwaway data path, the local file name, and (new) any `"Path":"/..."` or `/media/`; S session test checks `/Sessions` |
| Write gating: with `allow_server_writes` off every forwarded POST/DELETE is refused | ok | S `with_server_changes_off_no_route_forwards_a_post_or_delete` fires every writing route (4 report routes, 8 played/favourite calls in both route forms and both verbs, both user-data routes, 5 session command calls) with `sync_progress` on and the mock records zero non-GET requests; its control `the_same_calls_do_reach_the_server_with_the_switches_on` proves the table does reach every path; `jellyfin_user::raw::allowed` unit test; L `writes_reach_only_the_items_own_server_as_the_switches_allow` |
| WebSocket: frame and message size cap (16 KiB), 4 sockets per token, idle drop, revoked token ends its sockets, no ambient credentials so no cross-site socket hijack | ok | S `a_token_gets_four_sockets_and_oversize_frames_close_the_socket` (new); `limits::socket_slots_per_token`. Origin is not checked on purpose: auth is a token the page must already hold, never a cookie |
| Local files: only by index id, canonical path must stay under a configured root, read-only open | ok | S `local_files_are_served_only_from_under_a_configured_root` (new: symlink to a file and to a directory outside, dot-dot, absolute path, unknown source); `stream::serves_a_file_with_range_and_refuses_a_directory` |
| Control socket: 0600 in a 0700 directory, peer uid must equal ours, one line per request with a cap, 30 s per connection | ok, negative peer case untested | G `control_socket_drives_the_gateway` checks both modes and the same-uid path; the foreign-uid refusal (`peer_cred`) is code-reviewed only, a second uid is not available to tests |
| Private files 0600 in 0700 dirs | ok | `config::private_files_are_0600_in_0700_dirs` |
| Loopback-only default and LAN-mode peer filter | ok | S `local_mode_does_not_listen_on_the_lan_address` (binds the machine's real LAN address, connection refused; skipped without a non-loopback address), `lan_mode_serves_private_peers_and_refuses_the_rest_at_accept_time` (served from the LAN address; public, documentation, CGNAT-outside-range and IPv6 global addresses refused by the accept predicate); `server::admit_follows_the_binding_mode` |
| TLS: no start without certificate and key, bad files refused with a reason, HSTS | ok | G `remote_mode_will_not_start_without_a_certificate`, `remote_mode_serves_https`, `tls::bad_files_are_refused_with_a_reason` |
| Systemd unit hardening and validity | ok | `user_unit::unit_has_the_hardening_and_the_quoted_binary`; new `systemd_accepts_the_unit` runs `systemd-analyze verify --user` on a temp file (offline, nothing enabled or started; skipped without the tool) |
| Fuzzing of the id and playlist rewriters and the `Authorization` parser | partial | `rewrite::junk_never_panics_or_leaks` (new): 3000 fixed-seed junk strings through `rewrite_url`, `rewrite_playlist`, `redact`, `parse_authorization` and `Gid::parse`, asserting no panic, no server token left in any query parameter and no server host left in a well-formed absolute URL. A coverage-guided fuzzer (cargo-fuzz) is not set up |
| Secrets in logs of a full test run | ok | the live suites were run with `--nocapture` and the output grepped for both servers' admin tokens and the test users' passwords: no hit |
| Dependency audit | see `scripts/ci-checks.sh` | `cargo deny` runs in the gate; no dependency was added in this pass |
| NowPlayingItem and sessions carry no paths | ok | S `now_playing_shows_the_real_item_without_paths_and_user_data_news_is_pushed` (new) |

Footprint (`fast-release` profile, Linux x86_64, no clients, no sources): `nocturne` 56.7 MB, `nocturne-gateway` 14.0 MB; the gateway idles at 9.4 MB RSS with 3 threads and used 0 CPU ticks (under 0.01 % of a core) over 60 s.

Behaviour added in the same pass: `NowPlayingItem` of a native session is the item's real DTO (name, type, series fields, runtime, image tags; never `Path` or media sources), fetched through the library code and kept 5 minutes; `UserDataChanged` is pushed on played, favourite and user-data changes and on playback stop; `LibraryChanged` is pushed when the app reloads the gateway (sources or settings changed). The gateway keeps no socket to the upstream servers (it only polls, with short caches), so a change made directly on a server shows up on the next cache expiry, not as a push; adding upstream sockets would cost an idle connection per server and was left out.
