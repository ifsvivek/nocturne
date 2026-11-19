# NOCTURNE v0.6 brief (maintainer's requirement, recorded 2026-11-17)

Status: not started. After 0.5 (and only with the maintainer's go-ahead).

## Seerr requests inside the app
- Integrate a Seerr server (Jellyseerr / Overseerr lineage) so users can request movies and shows from inside NOCTURNE, without opening Seerr's web UI. Seerr hands requests to the user's download stack (e.g. Radarr / Sonarr); once the file lands in a Jellyfin library, it simply appears in NOCTURNE's catalog.
- Only requesting (and request status) lives in NOCTURNE; downloading stays in the user's existing stack.
- To investigate first: the Seerr API (auth via Jellyfin sign-in vs API key, request / search / status endpoints, per-user permissions and quotas, 4K requests, season selection for shows), how to show "Requested / Pending / Available" on search results and detail pages, and how it fits the read-only-by-default rule (a request is a write to the Seerr server: gate it behind an explicit setting and the user's Seerr permissions).

## Play and save links with yt-dlp (decided 2026-11-18)

- **Play a link** (on by default): paste a video URL (YouTube, Vimeo, Twitch VODs and the other sites yt-dlp supports) and it plays in NOCTURNE's own player through mpv's built-in yt-dlp hook (`ytdl=yes`), with the usual controls, subtitles and hardware decoding. Check that the embedded libmpv context loads the hook; the external player does.
- **Save to library** (off by default, Settings): save a link's video into one of the user's local folders with yt-dlp, through the existing downloads queue; it then appears in the local library like any other file. Neutral wording ("Save to library"), never branded as a YouTube downloader, with a short note to save only videos you have the right to keep.
- `yt-dlp` is an optional dependency, never bundled; when it is missing the app explains how to install it with the system package manager.
- Not in scope: browsing or searching YouTube inside NOCTURNE (needs a Google API key and quota or fragile scraping; a different product).

## Native layout engine for themes (decided 2026-11-19)

0.5 lets CSS restyle NOCTURNE's named app parts (cards, rows, hero, header, typography, hiding and reordering sections). The next stage gives themes real layout power without a browser: each screen becomes a tree of NOCTURNE components laid out by a native flexbox/grid engine (Taffy, MIT, used by other Rust UI toolkits), so `display: flex` / `grid`, sizes, gaps, alignment and ordering in a stylesheet can rearrange whole screens. Still no WebView, browser engine or HTML: the tree contains only NOCTURNE's own components, and CSS never adds arbitrary elements or runs code. Scope, performance budget (layout once per change, never per frame) and the migration path from the 0.5 named parts are to be planned when 0.6 starts; it may move to 0.7 if it is too large.
