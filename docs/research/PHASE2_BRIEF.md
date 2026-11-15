# Phase 2 brief: post-release research and improvement (QUEUED, not started)

Start only after: everything is implemented, tested, and the first public release is published.
Owner's brief (2026-11-15), condensed. Rules from CLAUDE.md still apply (max two subagents, one at a time by default, main agent reviews).

## Scope
1. Research the Jellyfin ecosystem from primary sources (official GitHub repos): Server, Web, docs, API/SDK, other official clients, and upstream deps of NOCTURNE (GPUI, libmpv). Not only Jellyfin Web.
2. Read open and closed issues (not titles only): playback, auth, API inconsistencies, metadata/artwork, search, browsing, subtitles/audio tracks, hardware acceleration, performance, large libraries, Linux/Wayland/X11, UX complaints, frequent feature requests. Classify each: Server limitation / Web limitation / API limitation / client problem / Linux-specific / solvable independently by NOCTURNE. Record repo + URL for every important finding.
3. Read relevant PRs (code, not just descriptions): new capabilities, API changes/deprecations, playback, performance, auth, metadata, subtitles/audio, Linux-relevant changes. Note upstream changes that may require updating NOCTURNE.
4. Study Jellyfin Web as a reference (home, Continue Watching, Next Up, search, details, seasons/episodes, playback, track selection, prefs, auth/server selection, image caching, keyboard). Adapt ideas, do not clone web patterns: prefer native desktop conventions (shortcuts, window management, system media controls, notifications where appropriate, background work).
5. Compare against the released NOCTURNE: missing/incomplete features, weaknesses, bugs, architecture limits, performance, security, UX, compatibility. Every recommendation needs a reason it belongs in a native client.
6. Backlog entries: feature/problem, description, why it matters, evidence URLs, related issues/PRs, effect on NOCTURNE, approach, complexity, dependencies, risks, priority. P0 critical (security, data loss, serious playback failure, crashes), P1 high, P2 medium, P3 exploratory. Do not inflate priorities.
7. NOCTURNE-specific opportunities (evaluate, do not assume): native playback and libmpv features, hardware decoding, subtitles, library navigation, artwork caching/preloading, search, desktop media controls, keyboard-driven nav, multi-server, network resilience, large-library performance, accessibility, customizable playback.
8. Verify technical feasibility against current Jellyfin API behaviour and supported server versions, SDKs, libmpv, GPUI, Linux graphics, Wayland/X11, existing Rust deps, upstream maintenance. Separate "upstream already provides" from "needs NOCTURNE work". If an issue is fixed upstream, check it is in a released version before acting.

## Deliverables
- `docs/research/POST_RELEASE_RESEARCH.md`: scope, repos, important issues, important PRs, Jellyfin Web observations, community feedback, problems affecting NOCTURNE, proposed improvements with priorities, feasibility, source URLs, research date. Factual and traceable; never invent issue numbers, PRs, opinions or capabilities.
- Update `docs/ROADMAP.md` with the most valuable findings.
- Curated list of important issues/PRs; Jellyfin Web vs NOCTURNE comparison; prioritized backlog; implementation roadmap; list of upstream contribution opportunities.
- Then implement the highest-value safe improvements incrementally (small plan, tests, docs, no breaking changes, no history changes), reporting files changed and tests run for each. Separate researched from implemented clearly.

## Hard rules
- Do NOT file issues or PRs upstream under the owner's GitHub account without explicit approval; prepare drafts for review.
- Do not claim endorsement or upstream support.
- Keep the read-only/write-gating policy for the owner's server.
