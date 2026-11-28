# Native requests vs Seerr, Radarr and Sonarr (research, 2026-11-28)

Status: research only, no product code. Question from the maintainer: for Seerr, Radarr and Sonarr (with the gateway as the front door), keep connecting to the existing programs, or build native Rust replacements into NOCTURNE? Inputs: [SEERR_PLAN.md](SEERR_PLAN.md), [GATEWAY_PLAN.md](GATEWAY_PLAN.md), [WRITES.md](../WRITES.md), [ROADMAP.md](../ROADMAP.md) (0.8 add-ons, Dropped), and the code in `crates/seerr`, `crates/arr`, `crates/core/src/{wishlist,requests,discover,discover_fetch}.rs`, `crates/app/src/app/{requests,discover}*.rs`, `crates/gateway`.

## 0. What exists today (0.7.0)

| Piece | Where | Size (lines) | What it does |
|---|---|---|---|
| Seerr client | `crates/seerr` | ~1 100 | sign-in (Quick Connect, password), search, discover, details, own requests, quota; writes `create_request`, `delete_request` behind `RequestConfirmed` |
| Radarr / Sonarr client | `crates/arr` | ~800 | status, quality profiles, root folders, lookup, progress (`/queue`); writes `add_movie`, `add_series` behind `ArrConfirmed` |
| Native request list | `core/wishlist.rs` | 380 | one local `wishlist.json`, add / merge seasons / cancel, Requested → Queued → Downloading → Available, Available computed from the catalog by TMDB id and seasons |
| Request logic | `core/requests.rs` | 680 | permission bits, badges, season picker, quota and error wording |
| Discover | `core/discover.rs`, `discover_fetch.rs` | 1 290 | trending / popular / upcoming / search / genre / year from Seerr, the user's own TMDB key, Trakt, or *arr lookup |
| UI | `app/requests*.rs`, `app/discover*.rs` | ~4 000 | Requests page, Discover view, dialogs |
| Gateway | `crates/gateway` | ~11 000 | Jellyfin API subset, **one gateway user** (decision 2), no request routes |

So NOCTURNE already is a single-user, single-machine Seerr: discover, request, track, turn Available. The *arr side is already the "thin manager" shape: NOCTURNE adds a title and reads progress; the user's Radarr / Sonarr does the rest.

## 1. Seerr

### What is missing to replace a Seerr server for one household

| Seerr feature | NOCTURNE today | Needed for a household | Cost (new lines, rough) |
|---|---|---|---|
| Discover, search, details | yes, but needs Seerr, a TMDB key or Trakt | works with no key at all | small if a project TMDB key ships (open question 3); a proxy service is ops, not code |
| Request a movie / seasons | yes (local list) | yes | 0 |
| Availability sync | yes, from the catalog, per season | yes, plus the gateway's view of the same catalog | small (the gateway already has the merged catalog) |
| Hand-off to Radarr / Sonarr | yes, at request time | after approval, not at request time | small |
| Several users | no: list is per desktop, gateway has one user | each person requests from their own device (phone, TV, laptop) | **large**: multi-user gateway (users, passwords, per-user tokens and user data overlay), requests stored in the gateway, not the app |
| Request from a phone / TV | no | the requesting device is usually not the desktop | depends on a client: official Jellyfin apps have no request UI; only a Seerr-API client or the gateway's web page could do it |
| Approvals (pending / approve / decline) | no (every native request is "approved") | admin approves, others request | medium: status + one admin dialog; the approve is the `ArrConfirmed` step moved |
| Per-user permissions, auto-approve | no | a few flags per user | small once multi-user exists |
| Quotas / request limits | no | N per D days per user | small (pure logic + test) |
| Notifications | none (no desktop notification code yet; `zbus` is already a dependency for MPRIS) | "your request is available" to the requester | medium: desktop notification is small; email / Discord / push / webhooks are each an integration with secrets and failure handling |
| Issues ("audio out of sync" reports) | no | rarely used | skip |
| 4K requests, advanced options | no | rare | skip |
| Watchlist import (Plex / Trakt) | Trakt exists separately | nice | skip |

Total to match Seerr for a household: roughly 4 000–6 000 lines plus a multi-user gateway, i.e. one release cycle of two workers, and permanent upkeep of a second user/permission system. The decisive cost is not the request logic (done) but **multi-user and a request UI on other devices**, which is exactly what Seerr already ships as a web app.

### Seerr-compatible API from the gateway?

| For | Against |
|---|---|
| Existing Seerr clients would work against NOCTURNE (e.g. Streamyfin's Seerr integration, Seerr mobile wrappers) | Seerr's API is large (spec 8 325 lines) and moves between minor versions; we would chase it like the Jellyfin API but without a contract owner on our side |
| | Needs Seerr's auth (`/auth/jellyfin` with a password, cookie sessions, permission bitmask) on top of the gateway's token auth: a second auth surface on an internet-facing process |
| | Few clients use it; the official Jellyfin apps do not |
| | Only useful after multi-user exists |

**Recommendation for Seerr: keep both, do not replace.** Native requests stay the "no server needed" path for one person (already done). Households that want approvals, quotas, notifications and requesting from phones keep running Seerr, which NOCTURNE already talks to. Small, cheap improvements worth doing:

1. Desktop notification when a native request turns Available (small).
2. Optional quota-free "approval" is not needed for one user; skip.
3. Discover without any key (open question 3).
4. No Seerr API in the gateway now; revisit only if the gateway becomes multi-user and a concrete client asks for it.

## 2. Radarr and Sonarr

### What a native replacement really requires

| Part | What it involves | Existing Rust | Size / risk |
|---|---|---|---|
| Indexer search | Torznab (torrent) and Newznab (Usenet) over HTTP + RSS/XML, caps, categories, per-indexer limits; in practice through Prowlarr or Jackett | [`torznab`](https://docs.rs/torznab) 0.7.2, last release 2021-12, ~30 recent downloads (unmaintained); no Newznab crate found | medium code, **high policy risk** (section 2.2) |
| Release-name parsing | title, year, season / episode / absolute / daily, quality source (WEB-DL, BluRay, Remux), resolution, codec, edition, proper / repack, language, release group, anime | [`torrent-name-parser`](https://github.com/IGI-111/torrent-name-parser) 0.12.1 (MIT, last release 2023-04, 20 stars); Radarr / Sonarr's own C# parser is thousands of lines of regex tuned over a decade | large; quality of matching is the product |
| Quality profiles, custom formats, upgrades | ranking, cutoffs, size limits, preferred words, upgrade until cutoff | none | large |
| Download-client control | qBittorrent, Transmission, Deluge (torrent); SABnzbd, NZBGet (Usenet): add, categories, poll, remove, seeding ratio rules | [`qbit-rs`](https://github.com/George-Miao/qbit) 0.6.0 (MIT, active), [`transmission-rpc`](https://github.com/j0rsa/transmission-rpc) 0.5.0 (2025-02); nothing for SABnzbd / NZBGet (simple JSON APIs) | medium |
| Import / rename | hardlink or move from the download folder into library folders, naming templates, permissions, extras, sample removal, cross-filesystem moves | none | medium code, **conflicts with rule 3** |
| Monitoring and calendar | RSS sync every 15 min, missing / cutoff-unmet searches, air dates from TVDB / TMDB, series metadata refresh | Calendar already exists in NOCTURNE (reads) | medium, and an always-on process |
| Failed-download handling | detect stalled / failed, blocklist the release, search again | none | medium |
| Database and history | per-file history, grabs, blocklist, queue state survive restarts | SQLite would be a new dependency (DECISIONS: JSON until it fails) | medium |

Effort: a usable movie-only subset is ~15 000–25 000 lines; parity with Radarr + Sonarr (anime, daily shows, custom formats, upgrades) is far more, and the work never ends because indexers, release naming and download clients keep changing.

### Against the project's standing decisions

| Decision / constraint | Effect on a native *arr |
|---|---|
| No torrent features, no libtorrent in NOCTURNE (maintainer, 2026-11-27; ROADMAP 0.8) | A native *arr's core job is finding and grabbing torrents (and NZBs). Even without libtorrent, driving qBittorrent from NOCTURNE is a torrent feature in substance. Direct conflict. |
| Local media is never modified (CLAUDE.md rule 3, SOURCES.md) | Import means moving / renaming / hardlinking into library folders. Would need a new, separate "managed folder" concept and an exception to rule 3. Direct conflict. |
| Flathub and distribution review | An app that searches torrent indexers is a likely review problem; Radarr / Sonarr are not on Flathub for that reason's neighbourhood. Today NOCTURNE only calls the user's own services, which is defensible. |
| Footprint rules (PLATFORMS.md) | An always-on RSS / monitoring loop and a database inside a desktop app; it would have to live in a companion process like the gateway. |
| Legal exposure | Shipping indexer search makes NOCTURNE itself the tool; integrating with a tool the user already runs keeps NOCTURNE a client. |
| Mature tools exist | Radarr, Sonarr, Lidarr, Readarr, Prowlarr (GPL-3.0, active). Rust efforts exist but are early: [Scryer](https://github.com/scryer-media/scryer) (GPL-3.0, Rust, ~480 stars, one binary replacing Sonarr + Radarr + Bazarr + Seerr, beta); [Managarr](https://github.com/Dark-Alex-17/managarr) (Rust TUI that manages existing *arrs, ~790 stars); [MediaManager](https://github.com/maxdorninger/MediaManager) (Python, AGPL, ~3 300 stars). |

### Options

| Option | What | Effort | Risks |
|---|---|---|---|
| **(a) Keep integrating** (today) and improve | see the list below | small, per item 300–1 500 lines | low; same pattern as now (keyring key, guard, confirmation token, throwaway live tests) |
| **(b) Thin manager** over the user's tools | (a) plus a read-mostly "Activity" view: *arr queue, calendar of upcoming episodes / releases (already have Calendar), download-client status (through the *arr's `/queue`, not by talking to qBittorrent directly), health warnings; writes limited to "search again", "remove from queue", "change quality profile", each confirmed | medium, ~3 000–5 000 lines | low to medium; still no indexer or torrent code in NOCTURNE; scope creep toward a full *arr UI (Managarr already is one) |
| **(c) Full native replacement** | indexers, parser, profiles, download clients, importer, monitoring, DB | very large, open-ended (above) | conflicts with three standing decisions; legal and Flathub risk; permanent maintenance; Scryer already attempts it |

Improvements under (a), in order of value:

1. Read Radarr / Sonarr `/queue` and `/calendar` more fully: show download progress, ETA and "stalled / failed" on the Requests page (today: Queued / Downloading only).
2. Lidarr (music) and Readarr (books) for requests of those types, only when NOCTURNE has music and book request flows (Readarr is retired upstream; check before any work).
3. Prowlarr: nothing to do; it feeds the *arrs, NOCTURNE never needs indexer access. Do not add it.
4. Sonarr season-monitoring body still needs confirming against a real Sonarr 4 (STATUS.md).
5. Run the untested live tests (`scripts/test-arr.sh`, Docker) before any new *arr work.

**Recommendation for Radarr / Sonarr: (a) now, the read-only half of (b) later (queue detail, health), never (c).** Users who want a single Rust binary can run Scryer; if it exposes a Radarr / Sonarr-compatible API (unverified), NOCTURNE gets it for free through `crates/arr`.

## 3. Gateway

| Question | With the recommendations above |
|---|---|
| Serve a Seerr API? | No. Only useful with multi-user, a second auth surface on the internet-facing process, few clients. |
| Serve Radarr / Sonarr status to phone apps? | No route exists in the Jellyfin API for it, so official apps cannot show it; would need a custom client or the gateway's web page. Not worth it. |
| Serve the native request list to other devices? | Only meaningful if the household wants requests from phones without Seerr. That is the multi-user gateway decision (open question 1), not a requests decision. |
| Anything that must change now | Nothing. Keep the allow-list router as is; the gateway never holds *arr or Seerr keys and never mints `ArrConfirmed` / `RequestConfirmed` (same rule as `Confirmed`). |
| If (c) were ever chosen | The *arr engine would have to be a companion process like the gateway (always-on loop, DB), and the importer would sit outside rule 3. Another reason not to choose it. |

## 4. Recommendation

| Component | Recommendation | Next step |
|---|---|---|
| Seerr | Keep integrating; native list stays the single-user path; no replacement | desktop notification on Available; Discover without a key (Q3) |
| Seerr API in gateway | No | revisit only with multi-user gateway and a named client |
| Radarr / Sonarr | Keep integrating (a); later the read-only part of (b) | richer queue status on the Requests page; run the Docker live tests |
| Prowlarr | Do not integrate | none |
| Lidarr / Readarr | Later, only with music / book requests | check Readarr's upstream status first |
| Native *arr (c) | Never (conflicts with torrent decision, rule 3, Flathub) | point users to Radarr / Sonarr or Scryer |
| Gateway | No change | none |

## 5. Open questions for the maintainer

| # | Question | Recommendation |
|---|---|---|
| 1 | Should the gateway become multi-user (per-person requests, approvals, quotas from phones)? | Not now. It is the real cost of replacing Seerr; Seerr already does it. Keep decision 2 (one gateway user). |
| 2 | Is a household expected to run Seerr, or should NOCTURNE alone be enough? | NOCTURNE alone for one person; Seerr for a household. Say so in FEATURES.md. |
| 3 | Discover without any key: ship a project TMDB key in the binary, run a small proxy on the website, or keep "bring your own key"? | Ship a project key (TMDB permits it for non-commercial apps with attribution; Seerr does the same), with the user's own key overriding it and an attribution line in About. No proxy (ops cost). Needs the maintainer to register the key. |
| 4 | Desktop notifications when a request turns Available? | Yes, `org.freedesktop.Notifications` over the `zbus` already used for MPRIS (no new dependency); opt-in; a platform seam (Linux first). |
| 5 | Richer *arr status (progress, ETA, stalled / failed, "search again")? | Yes for reads; "search again" and "remove from queue" only as confirmed writes behind `arr_requests`. |
| 6 | Is a Seerr-compatible API wanted for a specific client (e.g. Streamyfin)? | No unless a named client and multi-user exist. |
| 7 | Any path toward a native indexer / download engine? | No; keep it in Dropped next to libtorrent, with the reasons from section 2. |
