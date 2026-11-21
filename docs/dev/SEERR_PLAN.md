# Seerr requests: research and plan (0.6.0, feature 3)

Status: research done, no product code. Review by the master before any code. Every fact below was checked against the Seerr v3.5.2 sources and OpenAPI file, and against a throwaway Seerr (Docker, bound to 127.0.0.1, data under /tmp) connected to the throwaway Jellyfin from `scripts/test-jellyfin.sh`. Both were stopped and their data deleted afterwards. JSON examples are sanitised (ids and hosts are throwaway values, tokens removed).

## 1. Which Seerr

- Overseerr (Plex only) is stale. Jellyseerr (fallenbagel) merged with Overseerr into **Seerr**, repository `github.com/seerr-team/seerr` (the old `fallenbagel/jellyseerr` redirects there). Active: last push the day of this research, latest release v3.5.2 (2026-11-12), `develop` is the default branch, not archived. It supports Jellyfin, Plex and Emby, GPL-3.0.
- Images: `ghcr.io/seerr-team/seerr` and `docker.io/seerr/seerr`, tags `latest`, `vX.Y.Z`, `vX`, `vX.Y`, `develop`. Runs as uid 1000, config volume `/app/config`, default port 5055, env `PORT`, `HOST` (bind address; needed to bind a throwaway to loopback), `LOG_LEVEL`.
- API base `{server}/api/v1`; spec `seerr-api.yml` at the repository root (8325 lines). `GET /status` is unauthenticated and returns `{"version":"3.5.2","commitTag":"...","updateAvailable":false,...}`. `GET /settings/public` is unauthenticated and tells a client what to offer: `initialized`, `localLogin`, `mediaServerLogin`, `mediaServerType` (1 Plex, 2 Jellyfin, 3 Emby, 4 not configured), `movie4kEnabled`, `series4kEnabled`, `partialRequestsEnabled`, `applicationTitle`.
- Support policy for NOCTURNE: Seerr 3.x. A Jellyseerr or Overseerr server uses the same `/api/v1` paths for everything below; only Seerr is tested. Show the `/status` version in Settings and warn when it is below 3.0 (untested).

## 2. Sign-in options and permissions

Two mechanisms exist (the spec says so too): the session cookie from a sign-in, or the `X-Api-Key` header.

**API key (`X-Api-Key`).** One global key, read by an admin from Settings > General (`GET /settings/main`, field `apiKey`; `POST /settings/main/regenerate` rotates it). With the header alone the request runs **as user 1, the original administrator** (`auth/me` returned `{"id":1,"permissions":2}`, ADMIN). An optional `X-Api-User: <id>` header makes it act as any user. Verified in `server/middleware/auth.ts`. So the key is admin-wide, cannot be limited, and cannot be minted per user. Not suitable for a normal user.

**Jellyfin credentials: `POST /auth/jellyfin`** with `{"username":"alice","password":"..."}`. On a configured server do not send `hostname` (it fails with "Jellyfin hostname already configured"). Seerr forwards the credentials to its own Jellyfin (`AuthenticateByName`), so the account must exist on the Seerr's Jellyfin. The first sign-in of a user creates a Seerr user with the server's default permissions (here `32` = REQUEST) if "new Jellyfin login" is allowed; otherwise 403. Responses:

```
200 {"id":2,"permissions":32,"userType":3,"jellyfinUsername":"alice","movieQuotaLimit":null,"movieQuotaDays":null,"tvQuotaLimit":null,"tvQuotaDays":null,...}
    Set-Cookie: connect.sid=...; HttpOnly   (Seerr also echoes jellyfinAuthToken in this body: never store or log it)
401 {"message":"INVALID_CREDENTIALS"}
```

The cookie is `connect.sid`, HttpOnly, `Max-Age` 30 days (`server/index.ts`, `maxAge: 1000*60*60*24*30`), `SameSite=lax` (strict if the admin enabled CSRF protection; irrelevant for a non-browser client), `Secure` only on HTTPS. No refresh endpoint; the lifetime is fixed from sign-in. Without a valid cookie every call returns `401 {"message":"cookie 'connect.sid' required",...}`; an expired or revoked session gives the same. Policy: on 401 mark Seerr signed out and offer sign-in again; never loop.

**Quick Connect: `POST /auth/jellyfin/quickconnect/initiate`** (no auth) returns `{"code":"937215","secret":"..."}`; the user authorises the code in any signed-in Jellyfin client; poll `GET /auth/jellyfin/quickconnect/check?secret=...` (`{"authenticated":false}`, 404 when expired); then `POST /auth/jellyfin/quickconnect/authenticate` with `{"secret":"..."}` returns the User and sets the cookie. This is the only password-free path.

**Reusing the Jellyfin account NOCTURNE already has: no token reuse.** `/auth/jellyfin` needs the password, and there is no endpoint that accepts a Jellyfin access token. Seerr's `jellyfinAuthToken` is its own device token. Least-privilege flows, in order of preference:
1. Quick Connect (no password typed; NOCTURNE already offers Quick Connect sign-in for Jellyfin and has an account menu for it). Needs Quick Connect enabled on the Jellyfin behind Seerr.
2. Username and password, prefilled with the username of the Jellyfin account for that same server when the Seerr is attached to one of the user's servers. The password is used once for the POST and then dropped; only the cookie is stored. NOCTURNE already holds the password only transiently for its own login; do the same.
3. Developer API-key mode (env `NOCTURNE_SEERR_KEY`, never persisted, never in the UI) for testing only, read-only, like `NOCTURNE_JF_TOKEN`.
Local Seerr accounts (`POST /auth/local`, email and password, only if `localLogin` is true) are accepted as a fourth option for people whose Seerr users are not Jellyfin users; same cookie.

**Permissions** (bit flags in `permissions`; ADMIN `2` implies all): `MANAGE_REQUESTS 16`, `REQUEST 32` (basic; also needed for either type unless `REQUEST_MOVIE 262144` / `REQUEST_TV 524288` are set), `AUTO_APPROVE 128` (+ `_MOVIE 256`, `_TV 512`), `REQUEST_4K 1024` (+ `_MOVIE 2048`, `_TV 4096`), `AUTO_APPROVE_4K 32768`, `REQUEST_ADVANCED 8192`, `REQUEST_VIEW 16384`. A user's value is in `GET /auth/me` (`permissions`); `GET /user/{id}/settings/permissions` is admin only (403 for a normal user), so the client reads the bitmask from `auth/me` and evaluates the flags itself (`hasPermission`: ADMIN passes everything).

| Case | Result observed |
|---|---|
| Normal user (32) creates a movie request | 201, request `status 1` (pending), media `status 2` |
| Admin (2) creates a request | 201, request `status 2` (approved, auto-approve), media pending or processing |
| User without REQUEST | `403 {"message":"You do not have permission to make movie requests."}` |
| 4K when no 4K service is set up | `403 {"message":"You do not have permission to make 4K movie requests."}` |
| Same title twice | `409 {"message":"Request for this media already exists."}` |
| Quota used up | `403 {"message":"Movie Quota exceeded."}` (TV: "Series Quota exceeded.") |
| TV request without `seasons` | `500 {"message":"Cannot read properties of undefined (reading 'filter')"}`: the client must always send `seasons` |
| Admin-only action by a normal user (approve, settings) | `403 {"status":403,"error":"You do not have permission to access this endpoint"}` |
| Normal user deletes own pending request | `DELETE /request/{id}` 204 |
| TMDB hiccup | `500 {"message":"[TMDB] Failed to fetch movie details: read ECONNRESET"}`; transient, retry once on user action |

Search and browse need no REQUEST permission (a user with permissions 0 still searched, got 200, and an empty requests list).

## 3. Reads and writes: endpoints and shapes

**Search** `GET /search?query=alien&page=1` (page size 20; `language` optional) returns `{"page":1,"totalPages":81,"totalResults":1616,"results":[...]}`. A result is movie, tv or person (`mediaType`; skip persons):
```
{"id":348,"mediaType":"movie","title":"Alien","releaseDate":"1979-05-25","posterPath":"/vfrQ...jpg","voteAverage":8.1,"overview":"...","mediaInfo":{"id":2,"tmdbId":348,"status":2,"status4k":1,"jellyfinMediaId":null,...}}
{"id":1396,"mediaType":"tv","name":"Breaking Bad","firstAirDate":"2008-01-20","posterPath":"...","mediaInfo":{...}}
```
`id` is the TMDB id. `mediaInfo` is absent when Seerr has never seen the title. In search results `mediaInfo.requests` is **not** populated (length 0 even with a pending request) but `mediaInfo.status` is; detail calls carry the requests. Poster URL: `https://image.tmdb.org/t/p/w342{posterPath}` (TMDB direct) or Seerr's `/imageproxy` if image caching is on; use the TMDB CDN, no Seerr round trip, and cache like other posters. The very first search call after start returned 0 results once, a repeat returned 1616: treat an empty first page as normal and let the user retry.

**Details with status** `GET /movie/{tmdbId}` and `GET /tv/{tmdbId}` return the TMDB data plus `externalIds` (`imdbId`, `tvdbId`) and `mediaInfo`:
```
"mediaInfo":{"id":2,"status":3,"status4k":1,"jellyfinMediaId":null,
  "requests":[{"id":2,"status":2,"is4k":false,"requestedBy":{"id":2,...},"seasons":[{"seasonNumber":1,"status":2}]}],
  "seasons":[{"seasonNumber":1,"status":3}]}
```
TV: `seasons` of the TMDB data lists `{"seasonNumber":0,"episodeCount":9},{"seasonNumber":1,"episodeCount":7},...` (season 0 is Specials; the server's `partialRequestsEnabled` and `enableSpecialEpisodes` settings decide whether partial requests and specials are allowed). Per-season request state lives in `mediaInfo.seasons` and in each request's `seasons`.

**Status values.** `MediaStatus`: 1 unknown (Seerr knows the title, nothing requested: show "Request"), 2 pending, 3 processing, 4 partially available, 5 available, 6 blocklisted, 7 deleted. `MediaRequestStatus`: 1 pending approval, 2 approved, 3 declined, 4 failed, 5 completed. Badge mapping for NOCTURNE: no `mediaInfo` or status 1 or 7: nothing; request status 1: "Requested" (waiting for approval); media 2 or 3 or request 2: "Approved" ("Requested" on cards); request 3: "Declined"; request 4: "Failed"; media 4: "Partly available" (offer the missing seasons); media 5: "Available"; 6: hide the button.

**Create** `POST /request`:
```
{"mediaType":"movie","mediaId":348}
{"mediaType":"movie","mediaId":603,"is4k":true}
{"mediaType":"tv","mediaId":1396,"seasons":[1,2]}      // or "seasons":"all"
```
`mediaId` is the TMDB id. The `tvdbId`, `serverId`, `profileId`, `rootFolder`, `languageProfileId`, `userId`, `ignoreQuota` fields are advanced options (need REQUEST_ADVANCED or MANAGE_REQUESTS); NOCTURNE never sends them. Reply 201 with the request:
```
{"id":2,"type":"movie","status":1,"is4k":false,"seasons":[],"media":{"id":2,"tmdbId":348,"status":2,"status4k":1,"mediaType":"movie","jellyfinMediaId":null,...},"requestedBy":{"id":2,...},"modifiedBy":null}
```
`seasons":"all"` expanded to `[1..8]` for Game of Thrones (no season 0).

**List own requests** `GET /user/{myId}/requests?take=20&skip=0` returns `{"pageInfo":{"pages":1,"pageSize":20,"results":3,"page":1},"results":[request,...]}`; `GET /request?filter=pending|approved|available|processing|failed|completed|declined|all&sort=added&take=&skip=` returns only the caller's own for a normal user (everyone's for admin or MANAGE_REQUESTS). Requests carry `media.tmdbId`, `media.mediaType`, `status`, `seasons`, `createdAt` but **no title or poster**: the Requests page needs `GET /movie/{id}` or `/tv/{id}` per entry (cache, fetch lazily, at most a few in parallel). `GET /request/count` gives `{"total":4,"movie":2,"tv":2,"pending":3,"approved":1,"declined":0,"processing":1,"available":0,"completed":0}`. `GET /user/{id}/quota` gives `{"movie":{"days":7,"limit":1,"used":1,"remaining":0,"restricted":true},"tv":{"used":0,"restricted":false}}`; show the remaining count next to the Request button when `restricted`. Cancelling: `DELETE /request/{id}` (own pending requests; returns 204). Approve and decline (`POST /request/{id}/approve`) are admin actions and are not in scope.

**Requests without Radarr or Sonarr.** No download service is needed for the request itself. On the throwaway, a request went `pending` (user without auto-approve) or `approved` (admin) with no Radarr configured; after an admin approved it, media status became 3 processing and it stayed there. Seerr does not refuse; the stack just never fetches the file. So the live test needs **no** Radarr, Sonarr or extra containers. 4K requests need a 4K service, so they cannot be tested live (`movie4kEnabled` false): cover them with a fixture and the 403 above.

## 4. How Seerr maps to library items (Available)

Seerr knows items by TMDB id (`mediaInfo.tmdbId`, plus `tvdbId` for TV). Its Jellyfin scan jobs (`jellyfin-recently-added-scan`, `jellyfin-full-scan`, also `availability-sync`) match items of the enabled Jellyfin libraries by the item's `ProviderIds` (Tmdb, and the Tvdb/Imdb fallbacks), then set `mediaInfo.status` 5 and `jellyfinMediaId` to the Jellyfin item id. Verified on the throwaway: a Jellyfin movie given `ProviderIds.Tmdb = "348"` and a pending-approved request for TMDB 348 turned into `mediaInfo.status 5`, `jellyfinMediaId = <jellyfin item id>`, request `status 5` (completed) after a full scan (the library had to be enabled first: `PUT /settings/jellyfin/library/{id}` `{"enabled":true}`; sync with `POST /settings/jellyfin/library/sync`). The recently-added scan runs every few minutes by default and the full scan nightly; an item is therefore "Available" in Seerr minutes after Jellyfin has it.

For NOCTURNE: do not rely on `jellyfinMediaId` (it belongs to the Seerr's Jellyfin, which may not be one of NOCTURNE's servers, and local library items never have it). NOCTURNE's catalog already carries TMDB ids from its own sources; the "Available" state shown to the user is computed from the catalog (an item with that TMDB id, or for TV that season, exists in any source) first, with Seerr's `mediaInfo.status` as the fallback and for the Requests page. When the catalog gains the item, the Request button disappears and the Requests entry reads "Available"; no polling is needed beyond refreshing the Requests page and the visible detail page when they are opened (and a 60 s throttle on re-fetch).

## 5. Recommendation

Sign-in: Quick Connect first, username and password second, both producing the 30-day `connect.sid` cookie for the user's own Seerr account (REQUEST rights as Seerr's admin configured them). Never the global API key in normal use. The user signs in with the same Jellyfin account they already have; the least-privilege rule is "your own Seerr user, nothing admin". Details of the rest follow.

### Keyring and settings
- Secret: the cookie value (and nothing else) in the same keyring through `nocturne_session::save_secret` / `load_secret` / `delete_secret`, name `seerr:<normalised URL>` (lowercase scheme and host, no trailing slash), so the service stays `nocturne` or `NOCTURNE_KEYRING_SERVICE` (test isolation works unchanged). The password and the echoed `jellyfinAuthToken` are never stored or logged. Keyring unavailable: ask for sign-in each run, like Jellyfin sessions (no file fallback).
- Non-secret state (Seerr URL, the Seerr user id and display name, last permission bitmask) in the existing TOML in settings, versioned, atomic; add a `seerr` struct with serde defaults.
- Settings (new section "Requests" under Settings, after Sources; rows added at the end of the list, `cycle()` arms to match): **Seerr server** (URL, Sign in / Sign out, status line "Signed in as alice, 3 requests left this week"), **Allow requests** (`seerr_requests`, default off). Allow requests is its own switch and does **not** need `allow_server_writes`, because nothing is sent to a Jellyfin server; the UI says so in the row's help text: "A request is sent to your Seerr server, not to your media servers." Browsing and the Requests page (reads) work with the switch off once signed in; with it off the Request buttons are absent (not disabled). Developer API-key mode is read-only (no Request buttons).
- Privacy: NOCTURNE talks to Seerr only for sign-in, search, details, quota and requests; it sends the search text typed in the Request flow and the TMDB id to Seerr, nothing else. Titles and ids are never sent to a third party. The Search box keeps searching the user's own catalog; Seerr results appear only in a separate "Not on your servers" group below, and only when Seerr is signed in (off by default through the setup itself).

### Crate shape: `crates/seerr` (`nocturne-seerr`)
- Same pattern as `jellyfin-user` and `jellyfin-admin`: async `reqwest` client with `Client::new(base_url)`, `sign_in_password`, `quick_connect_{initiate,check,complete}`, the cookie held as a `SessionCookie` value (cookie jar disabled; add the `Cookie: connect.sid=...` header ourselves so nothing persists implicitly), typed errors (`Unauthorized`, `Forbidden{message}`, `Conflict`, `QuotaExceeded`, `Status{status,path}`, `Http`). Reads: `status`, `public_settings`, `me`, `search`, `movie`, `tv`, `my_requests`, `quota`, `request_count`. Writes: `create_request`, `delete_request`.
- Writes take a `RequestConfirmed` token (zero-size, private field, `i_have_asked_the_user()`), minted only in the confirmation handler of the Request dialog, exactly like `UserConfirmed`, with a `compile_fail` doctest. Sign-in is not a write in this sense (it is explicit user action) and takes no token. All futures go through `nocturne_core::bridge::spawn`; responses carry the page/epoch and are dropped when stale.
- Pure logic in `crates/core` (`requests.rs`): permission bit evaluation, status to badge mapping, the "available" computation against the catalog, quota text, season selection state, error to message mapping. Unit-tested without a window.
- Model types parse only the fields above with `#[serde(default)]` and `Option` everywhere; unknown fields ignored, because Seerr's JSON is large and changes between minor versions.
- A `crates/jellyfin/tests/readonly.rs`-style test is not needed (new crate), but add one that fails if `crates/seerr/src` calls a write verb outside `write.rs` (keep writes in one file).

### UI
- **Request button** on search results (the "Not on your servers" group), detail pages of a TMDB-known title that is not in the catalog (reached from search or the Calendar's "not on your servers" entries), and Calendar entries marked "not on your servers". Shown only when: Allow requests on, signed in, user has REQUEST (or the type-specific flag), `mediaInfo.status` is not 5/6, and for 4K only when `movie4kEnabled`/`series4kEnabled` is true and the user has the 4K flags (a "4K" toggle in the dialog).
- **Dialog** (confirmation; the only place a `RequestConfirmed` is minted): title and year, "Request this on {Seerr name}?", for a show a **season picker** (checkboxes, "All seasons" preselected; when `partialRequestsEnabled` is false the picker is replaced by one "All seasons" line; specials only if enabled; seasons already available or requested are shown ticked and disabled), 4K toggle when allowed, quota line ("2 requests left this week") and the sentence "This sends a request to your Seerr server. Nothing is changed on your media servers." Buttons Request and Cancel; errors from the table above shown inline in plain words ("You have used up your request quota", "Already requested", "Your Seerr account may not request 4K").
- **Badges** on cards and detail pages: Requested (request pending), Approved, Declined, Failed, Partly available, Available (catalog hit). Token colours only from `theme.rs`.
- **Requests page** (header link, shown when Seerr is signed in): a list with poster, title, type, status badge, requested date, seasons, Cancel (own pending only, confirmation), filter tabs All / Pending / Approved / Available / Declined; Available entries have "Open" when the item is in the catalog. Read-only without Allow requests (Cancel hidden).
- Copy says "request" and "Seerr"; no hard-coded Jellyfin.

### Test plan
- Unit tests with recorded JSON fixtures in `crates/seerr/tests/fixtures/` (sanitised copies of: search with and without `mediaInfo`, movie and tv details, create reply, requests page, quota restricted and not, `auth/me`, each error body, quick connect initiate/check) covering parsing, permission bits, status/badge mapping, season state, URL normalisation and keyring naming (with the in-memory store).
- Mock HTTP tests (no network) for sign-in cookie capture, 401 handling, 409/403 mapping and the header being added.
- Live test (`#[ignore]`, `crates/seerr/tests/live.rs`) guarded like `TestServerGuard`: a `SeerrTestGuard::connect()` that refuses unless the URL comes from the file named by `NOCTURNE_SEERR_TEST_ENV` (or the same `test.env`), is loopback on a non-default port (5055 is fine for a throwaway but must differ from the configured real Seerr), the Jellyfin it is attached to is the throwaway one on 8097 (checked through the Seerr's `/settings/public` and the Jellyfin test guard), and the container's data directory is under /tmp (the script sets `SEERR_TEST_DATA=/tmp/...` and writes it into the env file). A new `scripts/test-seerr.sh {start|stop|reset|status}` mirrors `test-jellyfin.sh`: `docker run -d --network host -e HOST=127.0.0.1 -e PORT=5055 -v /tmp/nocturne-seerr-test:/app/config ghcr.io/seerr-team/seerr:v3.5.2` (host network is needed so the container reaches the Jellyfin that listens on 127.0.0.1:8097; the `HOST` variable keeps Seerr itself on loopback), then seeds it: log in as the Jellyfin admin with `serverType:2` and `hostname`/`port` once, `POST /settings/initialize`, sync and enable the library, log in once as alice and bob. Credentials come from `test.env` and are never printed; the data directory is owned by uid 1000, so `reset` removes it with a throwaway container (`docker run --rm -v ...:/d alpine rm -rf`), because plain `rm` fails. Live checks print booleans and counts only. Order: sign-in password, wrong password 401, search, details, create request (alice, pending), duplicate 409, quota 403 after the test sets a quota through the admin cookie, list, cancel, then set `ProviderIds.Tmdb` on a throwaway Jellyfin item (admin API key on the throwaway), run the full scan job and assert the status moves to 5.
- App: headless screenshots against the throwaway Seerr for Settings, search group, detail with season picker, dialog, Requests page; demo data only.

### Risks
- Seerr changes between minor versions; the fixtures pin 3.5.2 behaviour, parsing is lenient, and the status line shows the server version.
- The cookie is a bearer credential for the user's Seerr account: keyring only, HTTP warning (plain `http://` for a non-loopback Seerr gets a one-line warning like Jellyfin), sign-out deletes it and calls `POST /auth/logout`.
- Seerr's `/auth/jellyfin` receives the Jellyfin password; if the Seerr is not the user's own server that is a trust decision. The sign-in dialog names the server it is sending to; Quick Connect avoids the issue.
- Seerr echoes `jellyfinAuthToken` in login responses: parse with a struct that does not have the field so it never reaches logs or `Debug`.
- A user can lose rights or exceed quota at any time: always read the error body and show it; refresh quota and `auth/me` when the Requests page opens.
- 500 on TV requests without `seasons` and flaky TMDB calls are Seerr bugs to be defended against, not user errors.
- Catalog-based "Available" may disagree with Seerr (an item on a server Seerr does not scan): the catalog wins for the button, Seerr's own state is shown on the Requests page.
- Binary size: no new dependency (reqwest, serde, tokio exist; no cookie-store feature).

### Open questions for the maintainer
1. Is a Seerr search group inside the Search page acceptable ("Not on your servers", only when signed in), or should requesting live only on detail pages and the Calendar?
2. Sign-in: Quick Connect plus password as above; is a local Seerr account (email and password) worth including in 0.6.0, or Jellyfin accounts only?
3. Several Seerr servers or exactly one? The plan assumes one (a Settings row), with the keyring name already per-URL so more can come later.
4. Should "Available" prefer the catalog (plan) or Seerr's status?
5. Should the user be able to cancel their own pending requests from NOCTURNE in 0.6.0 (a second write), or is that left to Seerr's web UI?
6. Is the docker-based `scripts/test-seerr.sh` acceptable in the repository (needs Docker; skipped like the Jellyfin live tests when absent), and is `ghcr.io/seerr-team/seerr` pinned to a version tag the right image?
7. Should 4K and the advanced options (server, profile, root folder) stay out of 0.6.0 entirely (4K cannot be live-tested without a 4K Radarr/Sonarr)?

## Decisions (master, 2026-11-21, maintainer away; revisit on request)

1. Search shows a "Request" group for titles found on Seerr but not in the catalog.
2. Jellyfin accounts only in 0.6.0 (Quick Connect first, then username and password); local Seerr accounts later.
3. One Seerr server.
4. "Available" comes from the catalog by TMDB id first, Seerr's `mediaInfo.status` as the fallback.
5. Cancelling your own pending request is in 0.6.0 (a write: confirmation dialog, `RequestConfirmed`).
6. `scripts/test-seerr.sh` (Docker, loopback, data under /tmp) is fine in the repository.
7. 4K and the advanced options (server, profile, root folder) stay out of 0.6.0.
