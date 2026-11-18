# NOCTURNE v0.6 brief (maintainer's requirement, recorded 2026-11-17)

Status: not started. After 0.5 (and only with the maintainer's go-ahead).

## Seerr requests inside the app
- Integrate a Seerr server (Jellyseerr / Overseerr lineage) so users can request movies and shows from inside NOCTURNE, without opening Seerr's web UI. Seerr hands requests to the user's download stack (e.g. Radarr / Sonarr); once the file lands in a Jellyfin library, it simply appears in NOCTURNE's catalog.
- Only requesting (and request status) lives in NOCTURNE; downloading stays in the user's existing stack.
- To investigate first: the Seerr API (auth via Jellyfin sign-in vs API key, request / search / status endpoints, per-user permissions and quotas, 4K requests, season selection for shows), how to show "Requested / Pending / Available" on search results and detail pages, and how it fits the read-only-by-default rule (a request is a write to the Seerr server: gate it behind an explicit setting and the user's Seerr permissions).
