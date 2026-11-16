# Online metadata for local media: sources and terms

Status: research only (2026-11-16), no integration code. Feeds milestone 5 of [SOURCES.md](SOURCES.md). The local
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
