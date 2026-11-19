# Server UI mode: research and plan (v0.5, section 1 of V05_BRIEF)

Status: research only, nothing implemented. Method: jellyfin-web sources on GitHub (`src/constants/homeSectionType.ts`, `components/homesections/homesections.js`, `scripts/settings/userSettings.js`), the existing client code in this repo, and GET-only calls against the throwaway server (127.0.0.1:8097, Jellyfin 12.4.0, stopped again). Nothing touched a real server, no token was printed.

## 1. What "the server's UI" is, as data

jellyfin-web is a JS app, but almost everything that makes "my server" look like itself is stored as plain JSON on the server and is readable with a user token. Four sources:

### 1.1 Display preferences: `GET /DisplayPreferences/usersettings?userId=<id>&client=emby`

jellyfin-web uses the fixed id `usersettings` and client `emby` (`CLIENT_ID = 'emby'` in userSettings.js). Real shape from the throwaway server (alice, never customised, `Id` is a derived guid):

```json
{"Id":"3ce5b65d-...","SortBy":"SortName","SortOrder":"Ascending","RememberIndexing":false,"RememberSorting":false,
 "PrimaryImageHeight":250,"PrimaryImageWidth":250,"ScrollDirection":"Horizontal","ShowBackdrop":true,"ShowSidebar":false,"Client":"emby",
 "CustomPrefs":{"chromecastVersion":"stable","skipForwardLength":"30000","skipBackLength":"10000",
                "enableNextVideoInfoOverlay":"False","tvhome":null,"dashboardTheme":null}}
```

Everything web-specific lives in `CustomPrefs` (string to string map, values are always strings, `null` allowed). Keys relevant to a native client, per jellyfin-web source:

- `homesection0` .. `homesection9` (MAX_SECTIONS = 10): value is one of `smalllibrarytiles`, `librarybuttons`, `resume`, `resumeaudio`, `resumebook`, `activerecordings`, `livetv`, `nextup`, `latestmedia`, `none`. A missing key falls back to the server default order: `smalllibrarytiles, resume, resumeaudio, resumebook, livetv, nextup, latestmedia, none, none, none` (also what an untouched user gets, as above). The legacy value `folders` means `smalllibrarytiles`. This is the whole "home sections order and types" feature.
- `enableBackdrops`, `enableThemeSongs`, `enableThemeVideos` (backdrop artwork on detail pages), `libraryPageSize` (default 100; 0 = no paging), `skipForwardLength` / `skipBackLength`, `enableNextVideoInfoOverlay`, `tvhome`, `dashboardTheme`, `displayLanguage`/`language` (already read by `jellyfin-user`).
- Per-library and per-page view state: `<key>` (JSON `{SortBy, SortOrder, ...}`), `<key>-filter` (not synced to server), `<key>-_view` (`Poster`, `PosterCard`, `Thumb`, `ThumbCard`, `Banner`, `List`...), plus image type keys. `<key>` is built by the page (library id plus item type); the exact key strings must be confirmed by diffing `CustomPrefs` of a user after using the web UI on the throwaway server (open item for phase 2; there is no way to create them without a write, and the web client is not allowed against any real server).
- Per-library `GET /DisplayPreferences/<libraryId>?client=emby` returned the same default object (no `CustomPrefs` of interest) on the throwaway server: the web UI does not use per-library display-preference objects for these knobs, it stores them all in the single `usersettings` `CustomPrefs`. So one GET covers everything.
- Top-level `SortBy`/`SortOrder`/`PrimaryImageHeight`/`ScrollDirection`/`ShowBackdrop` are legacy fields, mostly unused by the current web client; read, do not rely on.

### 1.2 User configuration: `GET /Users/<id>` -> `Configuration`

```json
{"PlayDefaultAudioTrack":true,"SubtitleLanguagePreference":"","SubtitleMode":"Default","DisplayMissingEpisodes":false,
 "DisplayCollectionsView":false,"GroupedFolders":[],"OrderedViews":[],"LatestItemsExcludes":[],"MyMediaExcludes":[],
 "HidePlayedInLatest":true,"RememberAudioSelections":true,"RememberSubtitleSelections":true,"EnableNextEpisodeAutoPlay":true,"EnableLocalPassword":false}
```

- `OrderedViews`: library ids in the user's chosen order (empty = server default order). `MyMediaExcludes`: libraries hidden from "My media" tiles. `LatestItemsExcludes`: libraries with no "Latest" row. `GroupedFolders`: libraries merged into one Next Up/Latest group. `HidePlayedInLatest`, `DisplayCollectionsView` (show the Collections view), `DisplayMissingEpisodes`, subtitle/audio defaults (`SubtitleMode`, `SubtitleLanguagePreference`, `PlayDefaultAudioTrack`, `Remember*Selections`, `EnableNextEpisodeAutoPlay`) feed the player.
- Per-user, so they must be fetched with the user token of that source; the API-key developer source has no user, so Server UI is simply unavailable there (see 3).

### 1.3 Library list: `GET /UserViews?userId=<id>`

Already used by `Client::libraries`. Throwaway shape: `{"Name":"Test Movies","Id":"8b0f...","CollectionType":"movies","Type":"CollectionFolder"}`. Server returns the server default order; the web client re-sorts it with `OrderedViews` (and drops `MyMediaExcludes` for the tiles). Images come from the existing `Items/<id>/Images/Primary` path.

### 1.4 Branding and server identity (public, no auth)

- `GET /Branding/Configuration`: throwaway returned `{"SplashscreenEnabled":false}` (`LoginDisclaimer` and `CustomCss` are omitted when empty). When the admin sets them: `{"LoginDisclaimer":"...text with links...","CustomCss":"...","SplashscreenEnabled":true}`.
- `GET /Branding/Css` (and `/Branding/Css.css`): the same CSS as a stylesheet, with `@import` lines left for the browser to resolve. Use `Configuration.CustomCss`, it needs one request less and no content-type games.
- `GET /System/Info/Public`: `ServerName` ("omen15"), `Version`, `Id`. `/Branding/Splashscreen?format=jpg` is the splash image (only when `SplashscreenEnabled`).
- Notes: `CustomCss` is also the vector for server-side injected JS-free theming; it is plain CSS, so the existing engine applies. `LoginDisclaimer` is HTML-ish text (anchors, line breaks): strip to text, keep link targets as plain "(url)" or clickable text opening the system browser (links go out of app, never rendered).

### 1.5 What is NOT data

Plugins that inject JS (custom tabs, banner plugins, Jellyfin Enhanced), the web client's CSS (DOM-bound selectors), per-device web `localStorage` settings (theme, layout "desktop/mobile/tv", language of the web UI itself), dashboard pages. None of this is readable or reproducible; document and warn.

## 2. Server UI mode: feature list

One setting per Jellyfin source (`sources.toml` profile, or a `server_ui` table in settings keyed by `SourceId`, same pattern as `[server_theme]` in `themes.toml`): `ui_mode = nocturne | server`, default `nocturne`. Local sources always NOCTURNE.

When a source is in Server mode, its own pages follow the server's data. Pages are the same widgets; only the data that drives layout changes.

Prototype scope (phase 1):
- Home sections in the server's order: `homesection0..9`, `none` skipped, rendered top to bottom as rows with the server's names: `smalllibrarytiles` (My media tiles), `resume` (Continue Watching), `resumeaudio` (Continue Listening), `resumebook` (Continue Reading), `nextup`, `latestmedia` (one "Latest <library>" row per library not in `LatestItemsExcludes`, in `OrderedViews` order, respecting `HidePlayedInLatest`), `livetv` and `activerecordings` (shown only if a Live TV library exists and the server answers; otherwise skipped with no error), `librarybuttons` (same data as tiles, button style).
- Library tiles: `UserViews` re-ordered by `OrderedViews` (unlisted ones appended in server order), `MyMediaExcludes` hidden from the tiles (still reachable via search and the header), `GroupedFolders` honoured for Next Up/Latest.
- Branding CustomCss: `Configuration.CustomCss` goes through `nocturne_core::customize` (parse, tokens, warnings) as that source's override theme; stored in memory or a cache file, never merged into the user's saved themes. Precedence for a page of that server: user-assigned override from `[server_theme]` (explicit choice) > server branding CSS > global theme. A "Server branding" entry shows in Settings > Themes with the warnings list. Re-fetch at connect and at most every launch; cached copy used offline.
- Login disclaimer shown on that server's sign-in page.

Later phases:
- Per-library default view/sort: `Poster/Thumb/Banner/List` choice and sort by/order as the initial state of the browse page for that server, still changeable in the session (not written back).
- Display pref extras: `enableBackdrops` toggles detail backdrops, `libraryPageSize` as page size, skip lengths into the player for that source, `DisplayCollectionsView` shows a Collections tile, `HidePlayedInLatest`.
- Server name as the title for that source's section in the header and breadcrumbs; splashscreen as the onboarding/sign-in backdrop when `SplashscreenEnabled`.
- Refresh: re-read the three JSON documents on pull-to-refresh, when the source reconnects, and on a 10 minute timer while the app is focused. "Changes to preferences made on the server are reflected in NOCTURNE" (brief) is satisfied by polling, there is no push for display preferences. Compare an ETag-like hash and only rebuild on change.

Out of scope (documented in the UI as "not shown natively"): plugin JS and custom tabs, web-only pages (Dashboard, Quick Connect admin, plugin pages), the web client's layout skeletons (sidebar drawer, tv/mobile layout), exact card sizes and typography, CSS layout rules, animations. No WebView, no JS ever.

Composition with the unified catalog:
- The unified Home keeps the NOCTURNE layout for all sources by design; the Server layout applies only when the user is viewing that server alone (source filter = that server, or its own Home entry). Concretely: Home with filter "All" merges items of every source into NOCTURNE rows (server-mode sources contribute as usual, Server UI never reorders another source's content); Home with filter "<server>" in Server mode renders the server's sections in the server's order.
- Detail pages of an item follow its origin: theme override, backdrop pref, player skip lengths of that origin source.
- Search and duplicates merge are unaffected (they are data, not layout).
- Theme: reuse `Store::active_id(source)` resolution, add the branding layer between override and global.

## 3. Writes (superseded: editing the home sections shipped in 0.5, see section 7)

Read-only in 0.5. Every call above is a GET (`crates/jellyfin` `get` plus new typed reads). Editing display preferences from NOCTURNE (`POST /DisplayPreferences/usersettings`, `POST /Users/<id>/Configuration`) is a user-scope write and would go behind `allow_server_writes` and `jellyfin-user`; `jellyfin-user` already has a read-modify-write of `usersettings` for the display language (`client.rs`, ~line 479), so a future "reorder home sections here" would reuse it, but the risk of clobbering other CustomPrefs keys concurrently with the web UI is real. Recommendation: no in-app editing in 0.5; link to "change on the server" only. Developer API-key source: no user, no Server UI.

## 4. Iced feasibility and reuse

- Rows: `app/home.rs` (992 lines) already renders `HRow::{Resume, NextUp, Latest(i)}` with `strip()` and `layout()`. The Server layout is a different `layout()` producing the same row kinds in a different order, plus tiles for My media. Reuse, small refactor: make `layout()` take an ordered list of `SectionKind` built from `HomePrefs`.
- Cards, grids, detail: reused as is. View style (Poster/Thumb/Banner/List) needs a card variant per image type: poster is existing, thumb (16:9 landscape) already exists for Continue Watching, banner and list are new but small. Library grid with `libview.rs` already has sort state.
- New: My media tile row (library artwork, 16:9 with name, or `librarybuttons` as text buttons) cost S; branding layer in theme resolution cost S (engine exists on v0.5-local); login disclaimer (text with plain links) cost S; Live TV/active recordings rows: no client support today, cost M, defer; per-library default view: M.
- Overall: backend/data about 60%, rest is glue. Network-facing code is in `crates/jellyfin` (GET), pure parsing/ordering in `crates/core`, view code stays thin.
- Cost: no new dependencies; extra requests per server at connect (`Branding/Configuration`, `DisplayPreferences/usersettings`, `Users/<id>`), small JSON, cacheable.

## 5. Phases

Phase 0 (half a day): confirm `CustomPrefs` keys for library view/sort/image type by reading a user whose settings were changed via the web UI on the throwaway server (needs a write on the throwaway server only, with `TestServerGuard`, in an `#[ignore]` live test). Record in this doc.

Phase 1 (prototype, one server, read-only):
- `crates/jellyfin/src/` new `server_ui.rs` (or `client.rs`): `display_preferences()`, `user_configuration()`, `branding()`, typed structs `DisplayPrefs { custom: HashMap<String, Option<String>>, .. }`, `UserConfig`, `Branding { login_disclaimer, custom_css, splashscreen_enabled }` (all fields optional, `#[serde(default)]`). `tests/readonly.rs` keeps passing (GET only). Unit tests with fixture JSON copied from section 1; live ignored test against the throwaway server.
- `crates/core/src/server_ui.rs` (pure): `ServerUi { sections: Vec<Section>, views_order: Vec<String>, my_media_excludes, latest_excludes, branding_css: Option<String> }`; `fn home_sections(&DisplayPrefs) -> Vec<Section>` (defaults, `folders` alias, unknown values dropped, `none` dropped), `fn order_views(views, &UserConfig)`, `fn visible_tiles`. Plus settings: `ui_mode` per source (versioned TOML, serde default, unit test).
- `crates/core/src/customize/store.rs`: `resolve(source)` gains `branding: Option<Theme>` layer; `Store` never persists it. Tests: precedence override > branding > global; warnings exposed.
- `crates/app/src/app/home.rs`: Server-mode `layout()`; My media tiles row; hook in `fan_answer` for the extra reads. Screenshot using the demo library.
- Tests: pure tests for ordering and defaults; fixture tests for branding with a realistic CSS; live test: set `homesection0=latestmedia` via TestServerGuard (throwaway only) and check order.
- Risk: web UI semantics drift between server versions (Jellyfin 10.8 to 12.x); mitigate by parsing defensively and keeping unknown sections hidden.

Phase 2: per-library default view/sort/image type, `enableBackdrops`, `libraryPageSize`, skip lengths, Collections tile, server name in header, login disclaimer on sign-in, Settings > Sources "Interface: NOCTURNE / Server" cycle row. Files: `app/browse.rs`, `core/libview.rs`, `app/sources.rs`, `core/settings.rs`. Risks: card variants for Banner/List; keep the existing grid virtualization (`virt.rs`).

Phase 3: refresh/poll, offline cache of the three documents, Live TV and active recordings rows if Live TV client support is added, splashscreen backdrop. Risk: Live TV endpoints are untested here.

Documentation: add `docs/SOURCES.md` paragraph (Server UI per source), `docs/WRITES.md` (read-only row), `docs/FEATURES.md`, `CHANGELOG.md`, and `docs/dev/STATUS.md`.

## 6. Open questions for the maintainer

1. Default mode for a newly added server: NOCTURNE (recommended; Server UI opt-in per source), or ask at the end of sign-in?
2. Branding CSS: apply automatically when the source is in Server mode (proposed), or always show it as a suggestion in Themes that the user accepts?
3. With the source filter on "All", should Server-mode sources still show their own sections (stacked per server), or only the unified NOCTURNE rows? Proposed: unified rows only, Server layout when one server is selected.
4. In-app editing of display preferences later (write behind `allow_server_writes`), or never?
5. Live TV rows: out for 0.5 or pulled in?
6. Language: show the server's `LoginDisclaimer` verbatim (it is admin text), with links opening the system browser?

## 7. What shipped in 0.5 (phase 0 and 1, with the maintainer's decisions)

Decisions: new servers start in the NOCTURNE layout and Server UI is a per-server switch (`server_ui` in the source profile, Settings > Sources > Layout); the branding CSS applies automatically in Server mode; filter All always shows the unified NOCTURNE rows and a server's own layout only when that server alone is selected; editing the home sections is in 0.5 behind `allow_server_writes`; Live TV rows are skipped silently; the login disclaimer is shown verbatim as plain text.

Phase 0 (throwaway server, Jellyfin 12.4.0): the `CustomPrefs` keys `homesection0..9` round-trip through `POST /DisplayPreferences/usersettings?client=emby` exactly as written, and every other key present before (`chromecastVersion`, `skipForwardLength`, `skipBackLength`, `enableNextVideoInfoOverlay`, `tvhome`, `dashboardTheme`) survives a read-modify-write. The per-library view/sort keys are still not confirmed (no web client run against the throwaway server); that stays in phase 2.

Built:
- `crates/jellyfin/src/server_ui.rs`: GET-only typed reads `display_prefs`, `user_config` (the `Configuration` block of `GET /Users/{id}`), `branding`; `public_info` already existed. `readonly.rs` still passes.
- `crates/core/src/server_ui.rs`: pure rules and tests: jellyfin-web default order, `none`, legacy `folders`, unknown kinds dropped, repeats ignored, `OrderedViews`, `MyMediaExcludes`, `LatestItemsExcludes`, and the editor model (ten slots, `none` padding). `Source::server_ui()` does the three reads.
- `crates/core/src/customize/store.rs`: branding layer (session state, never serialized, never a saved theme). Order: user per-server override, server branding, global, defaults (`Store::active`, `active_key`, `resolve`). Themes shows the branding of each server in Server mode with its counts and first warnings.
- `crates/app/src/app/home.rs`: `Home::server` holds the cached layout; `layout()` builds the server's rows from the same row kinds plus `HRow::Tiles` (My media, landscape tiles; `librarybuttons` uses the same row). Resume audio, resume book, Live TV and active recordings are listed in the editor as "not shown yet" and skipped. Per-source cache in `App::server_ui`, refreshed at connect, when the mode is switched on, when Home loads (cached copy first) and after a save; an answer for a source whose mode was switched off meanwhile is dropped.
- `crates/jellyfin-user`: `set_home_sections(&[&str; 10])`, a read-modify-write that replaces only `homesection0..9` and sends every other key back as read; refuses anything but ten known values. Live test `tests/home_sections_live.rs` (throwaway server only, guarded, restores the default).
- Editor (Settings > Sources > Sections): up / down, on / off, Save. With server changes off, or with a developer token, it is view-only with the usual hint; the handler re-checks the gate, uses the session of that source only, and never mints a confirmation token (same pattern as the display language).
- Login disclaimer: `Probe` reads the public branding; the sign-in page shows it as plain text (1500 characters at most).
- Debug QA hooks (`NOCTURNE_KEYS`): `cycle-filter`, `layout-<id>`, `sections-<id>`, `sec-toggle-<n>`, `sec-up-<n>`, `sec-down-<n>`, `sec-save`.

Checked in the app against the throwaway server (alice): NOCTURNE layout unchanged; Server mode shows My media then Latest in the server's order; the editor is view-only with writes off; with writes on a reorder is saved and is still there after a restart. Not checked: a server with real branding CSS (parsing and precedence are unit-tested only), several servers at once, a server with Live TV, the sign-in page with a non-empty disclaimer (state test only).

Still open (phase 2/3): per-library default view/sort, `enableBackdrops`, `libraryPageSize`, skip lengths, Collections tile, server name in the header, polling the three documents, offline cache on disk, splashscreen.

## 8. One server's UI over another (0.5.2)

Maintainer request: apply one server's UI over another, just for us in the app. Local and read-only: nothing is written to either server, and a borrowed UI is never editable (the home-sections editor, which writes, is only offered for the server being browsed with its own UI and only with server changes allowed).

- **Choice.** Per context: each source (servers and the local library) and "All": `ui_from` = `nocturne` (default), `self` (the server's own UI, the 0.5 Server UI switch) or the id of another server. Stored in `sources.toml` as the `[ui_from]` table (key `all` or the source id), file `version = 2`. A version 1 file's `server_ui = true` on a server becomes `ui_from = "self"` when it is loaded; the old key is no longer written (`Profile::server_ui` stays as a read-only mirror until the app uses `Sources::ui_of`). A choice that points at a removed source or a non-server, or a folder choosing `self`, is dropped on load and when a source is removed. `self` for "All" means the server of the item at hand.
- **Resolution** (`server_ui::pick`, pure): the browsing context and the item's own server give the UI source. NOCTURNE layout when nothing is chosen; the chosen server's layout when it is ready; otherwise the NOCTURNE layout with a note: signed out, unreachable, switched off, or gone (no note while its UI is still loading, so nothing flashes). A source's own id is normalised to `self`.
- **What is applied** (`server_ui::effective`): the UI source's home section list (kinds and order, from its display preferences) filled with the browsed context's own data: Continue watching, Next up and Latest come from the libraries of the browsed source or sources. The UI source's library order, My media and Latest exclusions and "hide played in Latest" belong to that server's libraries and users, so they apply only when the UI source is the server being browsed (`Pick::own`); otherwise they are dropped (`ServerUi::foreign`). Its branding CSS is the theme layer below the user's per-source theme (`customize::Store::active_with`), fetched through the `@import` rules of section 15 of `CUSTOMIZATION_PLAN.md`.
- **Loading** (`server_ui::wanted`): the app loads the `ServerUi` of every enabled server some choice points at, once per connect, as before; the user's account on that server is the one already signed in. A server that is signed out or unreachable simply is not available.
- **Tests.** `server_ui.rs` and `profile.rs` unit tests cover every case above (own, over another server, "All", a local folder, signed out, unreachable, loading, disabled, removed, dangling file, the layout and data split, `wanted`); `crates/core/tests/ui_from_readonly.rs` fails if the picking code or the stylesheet fetcher can reach a write crate, a confirmation token or a write verb.

