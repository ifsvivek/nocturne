# Online metadata for local media: sources and terms

Status: research (2026-11-16); implemented in 0.4 for TMDB, TVmaze, MusicBrainz + Cover Art Archive and OpenSubtitles (see
"Implementation" at the end). Feeds milestone 5 of [SOURCES.md](SOURCES.md). The local
library works fully from file names, `.nfo` and sidecar art; online lookup is an optional extra. Terms change, so
re-read the linked pages before implementing. Items marked (unverified) came from secondary sources only.

Privacy common to all: a lookup sends the parsed title, year and sometimes provider ids from the user's private
library to a third party, plus the user's IP address. Nothing is sent unless the user enables a provider.

| Source | Covers | Key (who supplies) | Data / image terms | Attribution | Rate limit |
|---|---|---|---|---|---|
| [TMDB](https://www.themoviedb.org/api-terms-of-use) | Movies, TV, art, ids (IMDb/TVDB cross-refs) | Free personal key from a TMDB account; user supplies | Non-commercial use only; commercial needs a written agreement. Do not cache longer than 6 months; delete on termination. Images are TMDB Content under the same licence. | TMDB logo (less prominent than own branding) and the notice "not endorsed, certified, or otherwise approved by TMDB" | No number in the terms; "excessive" use forbidden. Older docs cite 40 req / 10 s (unverified) |
| [TVmaze](https://www.tvmaze.com/api) | TV only: shows, episodes, cast, art, ids | None for the public API | Data CC BY-SA 4.0; images hot-linkable from CDN, cacheable indefinitely | Credit TVmaze (link back) | At least 20 calls / 10 s per IP, HTTP 429 beyond |
| [TheTVDB v4](https://thetvdb.com/api-information) | TV and movies, art, ids | Project key via contract, or user's own subscription PIN (about $12/year; [subscribe](https://thetvdb.com/subscribe)) | Contractual; discounts for free/open-source projects. Terms may change without notice | Visible link to TheTVDB.com where its metadata is shown (about/readme acceptable for CLIs) | Not published |
| [OMDb](https://www.omdbapi.com/) | Movies/series basics, IMDb rating and ids | Free key by email; user supplies. Poster API only for Patreon supporters | Site content CC BY-NC 4.0; posters not separately licensed | Not spelled out; credit OMDb | Free tier 1,000 req/day (unverified, not on the page) |
| [Wikidata](https://www.wikidata.org/) | Ids and cross-links (IMDb, TMDB, TVDB, MusicBrainz), labels, dates; no art | None | CC0 | None required (courtesy credit) | Polite use; send a descriptive [User-Agent](https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy) or risk HTTP 403; query service limits parallelism and run time |
| [MusicBrainz](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting) + [Cover Art Archive](https://wiki.musicbrainz.org/Cover_Art_Archive/API) | Artists, albums, tracks, ids; album covers | None; MusicBrainz requires a User-Agent with app name, version and contact | Core data CC0 (see [data licences](https://musicbrainz.org/doc/MusicBrainz_Database/Data_licenses), not re-checked); cover images carry the uploader's licence and are not uniformly free | Not required for core data; credit MusicBrainz/CAA as courtesy | About 1 req/s per IP; excess gets HTTP 503. CAA: no limits stated |
| [fanart.tv](https://fanart.tv/get-an-api-key/) | Extra art (logos, clearart, backgrounds) for movies, TV, music | Project key (developer) plus optional personal key (user); personal key shortens the image delay (unverified: 7 days vs 2) | Community-contributed artwork; read the site terms before use (not retrieved: page returned 403 for the terms) | Artwork requires attribution (unverified) | Not published |

## Fit for a GPL desktop app

- GPL is not the obstacle; data terms are. NOCTURNE ships no keys and no data, so CC0 / CC BY-SA data fetched at runtime
  does not affect the code licence. Caching CC BY-SA TVmaze data stays under that licence (note it in the cache dir).
- **TMDB** has the best coverage but is non-commercial and demands logo plus disclaimer. Fine for a free GPL app where
  each user holds their own key; ship no shared key (a shared key is the "who is the commercial user" problem).
- **TVDB** needs a user subscription or a contract; skip initially.
- **OMDb** adds little beyond TMDB and its poster API is paywalled; skip.
- **TVmaze** (TV) and **MusicBrainz + CAA** (music) need no key and have permissive data terms: easiest to enable.
- **Wikidata** is a cheap id cross-reference to improve duplicate matching with server items, without art.
- **fanart.tv** is decoration only; later, if ever.

## Recommendation

1. Off by default, one switch in Settings, with a plain statement of what is sent (title, year, ids, IP) and to whom.
   Per-provider toggles; local `.nfo`, sidecar art and user overrides always win over online data.
2. First providers: TMDB (movies and TV; user pastes their own free key, stored in the keyring like other secrets,
   never in settings TOML) and MusicBrainz + CAA (music). TVmaze as a keyless TV alternative. Defer TVDB, OMDb, fanart.tv.
3. Be a polite client: descriptive `NOCTURNE/<version> (<project URL>)` User-Agent, a per-provider request gate
   (MusicBrainz 1 req/s, others well under their limits), 429/503 backoff, look up each item once and store the result
   (overrides-style, never silently overwriting user edits), refresh TMDB data within 6 months or drop it.
4. Never send file paths, server addresses or anything but the parsed title/year/ids.

## Attribution UI needed

- Settings > Metadata: for each enabled provider its logo (TMDB, TVmaze, MusicBrainz) and link, the TMDB sentence
  "This product uses the TMDB API but is not endorsed or certified by TMDB.", and TVmaze's CC BY-SA credit.
- Same text in About / Credits, and in `THIRD_PARTY_NOTICES.md` style docs for the website.
- Items whose fields came from a provider show a small source label on the detail page ("Metadata: TMDB"), which also
  satisfies per-view credit if a provider ever requires it.

## Implementation (0.4)

Code: `crates/core/src/online/` (clients, matching, gates, run loop), `crates/local/src/online.rs` (stored layer, ageing),
`crates/app/src/app/online.rs` (switches, keys, finder dialog). Per local source the switches live in its profile in
`sources.toml` (`[profile.online]`: `details`, `tmdb`, `tvmaze`, `musicbrainz`, `subtitles`; all off by default); API keys
and the OpenSubtitles login live only in the system keyring (`secret|tmdb`, `secret|opensubtitles`,
`secret|opensubtitles-login`).

- **What is sent**: the parsed title, year, provider ids (IMDb / TMDB / TVDB from names or `.nfo`), season and episode
  numbers, and for music the album and artist name, plus the IP address. Never a file name, folder or path, a file hash or a
  server address. A unit test checks that no recorded request contains the media path.
- **User-Agent**: `NOCTURNE/<version> (+https://nocturne.ifsvivek.in)` on every request.
- **Gates** (minimum spacing per provider): TMDB 250 ms, TVmaze 600 ms (limit: 20 per 10 s), MusicBrainz 1100 ms (limit about
  1 per second), Cover Art Archive 500 ms, OpenSubtitles 300 ms. HTTP 429 / 503 back off (Retry-After if given, else 2, 4,
  8 s, at most 30 s, 4 tries). Three failed lookups in a row end the run; the next scan tries again.
- **Matching**: ids first (TMDB id, IMDb id via TMDB `find` / TVmaze `lookup`), else a search by title and year. A result is
  taken automatically only when its score is at least 0.85 and the runner-up trails by 0.15 (an exact title with a matching
  year scores 1.0, exact title and unknown year 0.85, wrong year 0.5). Otherwise up to five candidates wait in "Choose the
  right match" (Local folders page). Nothing found is retried after 30 days.
- **Layering**: user override > `.nfo` > sidecar art > online > file name, per field. Online data is stored in
  `online.json` and `online-art/` next to the index (never next to media). A title is looked up once.
- **TMDB retention**: entries older than 150 days are refreshed on the next run (with TMDB on); entries older than 180 days
  are deleted with their images on every run, even with TMDB off. "Remove downloaded details" deletes everything fetched.
- **Credits shown** (Local folders page, per enabled provider): TMDB link plus "This product uses the TMDB API but is not
  endorsed or certified by TMDB."; TVmaze link plus "TV data from TVmaze.com, licensed CC BY-SA 4.0."; MusicBrainz and Cover
  Art Archive; OpenSubtitles. Detail pages show a "Details" row naming the provider for items filled in online. The TMDB
  logo is not shipped (the notice is shown as text); add it before a public release if TMDB requires the logo.
- **Credentials**: TMDB accepts the v3 API key (sent as `api_key`) or the v4 read access token (Bearer, starts with `eyJ`);
  the key is checked with one `/configuration` request before it is stored.

### OpenSubtitles.com (subtitles)

Could not be re-read for this implementation: the official docs (Stoplight) are script-rendered and the site refused
automated fetches, so the points below come from the API as known at implementation time and are **unverified**; re-check
https://opensubtitles.stoplight.io/docs/opensubtitles-api and https://www.opensubtitles.com/consumers before release.

- Base URL `https://api.opensubtitles.com/api/v1`. Every request needs the user's own `Api-Key` (a free "consumer" key from
  their account) and a descriptive `User-Agent` (ours above).
- Search `GET /subtitles` with `query`, `year`, `imdb_id`, `season_number`, `episode_number`, `type`, `languages` (we send
  title or show name, year, IMDb digits, numbers and a two-letter language from the user's subtitle preference, default
  `en`; we do not send a movie hash).
- Download `POST /download {file_id}` returns a temporary `link` (and `remaining` quota); downloads normally require the
  user's login (`POST /login` for a bearer token). Free accounts have a small daily download quota (HTTP 406 when used up);
  the dialog says so. Search is cheap, downloads are the limited resource.
- Terms (unverified): personal use with the user's own key; the data may not be redistributed. Credit shown as text
  "Subtitles from OpenSubtitles." on the folders page. Downloaded files are kept in
  `<data>/local/<source>/subtitles/<item id>/`, keyed to the file's NOCTURNE id, and offered to the player by
  `LocalBackend::subtitles_of`.
