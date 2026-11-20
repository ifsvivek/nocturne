# Theme conformance

The measuring stick for the 0.5.2 theme engine (step T in [V052_PLAN.md](V052_PLAN.md)): the same Jellyfin web theme, the same server, the same demo data, rendered by the original jellyfin-web and by NOCTURNE with Server UI on, compared screen by screen. No theme CSS and no screenshots live in the repo; captures stay under `/tmp/theme-conformance/<theme>/` and only the `@import` line of each theme is kept (in `/tmp/theme-conformance/themes.json`, with its source URL).

## How the harness works

`scripts/theme-conformance.sh [--bin PATH] [--hidden] [theme ...]` (helper: `scripts/theme-conformance.py`, stdlib + Pillow).

1. **Server.** A throwaway Jellyfin built by `scripts/make-demo-library.sh` under `/tmp/nocturne-conf/demo` on `127.0.0.1:8171` (`NOCTURNE_CONF_PORT`), relaunched WITH the installed `jellyfin-web` (`/usr/share/jellyfin/web`). Admin and `demo` (administrator) passwords are generated and kept mode 600 under that directory; nothing is typed by hand. Before every branding write the script checks that the server's own data path is under the throwaway directory in `/tmp` (the same idea as `TestServerGuard`), and it refuses port 8097. The script never reads the maintainer's `.env` or contacts any configured server.
2. **Theme list.** `THEMES.md` of awesome-jellyfin is fetched at run time (raw, into `/tmp`). Active themes are run; the "hidden pending v12" ones only with `--hidden`. Each theme's repository README is fetched and its first documented `@import url(....css)` line is used (on Jellyfin 12 a file name containing `12` is preferred, `legacy`/`compat` files are skipped; a theme always runs with the import ITS OWN project documents: forks made for Jellyfin 12 are only noted in the report, never substituted. ElegantFin documents one import for 10.11 and later and says Jellyfin 12 needs Display Mode "Desktop (Legacy)"; the `elegantfin-jf12` fork used by the baseline looks almost stock). Only that line is written to the server's Branding Custom CSS (`POST /System/Configuration/branding`), so both clients resolve the theme from the network exactly as users do. Themes with several variants (colour schemes, flavours) run their first documented import.
3. **Original.** Headless `google-chrome-stable` (own profile in `/tmp`, remote debugging on a local port, driven over DevTools by a small stdlib websocket client) at 1920x1080, signed in as `demo` through the login form; waits for fonts, images and 2.5 s of settle time. Screens: Home, Movies library grid, a movie detail (Ember Tide), a series detail (The Ashen Coast).
4. **NOCTURNE.** The given debug binary through `scripts/run-isolated.sh` inside `HS_NAME=conf scripts/headless-session.sh` (headless sway, 1920x1080). The isolated config is seeded with the `demo` login (token in the keyring under the test service `nocturne-test`, removed again on exit) and `ui_from` of "All" and of the server set to that server (`sources.toml` version 2), so "All" uses the server's UI and its branding CSS is read; each launch also writes the engine report (`NOCTURNE_KEYS=theme-report@11000`, `NOCTURNE_THEME_REPORT=<file>`). One launch per screen with `NOCTURNE_KEYS` (`open-movies`, `open-shows`, then `enter` on the first card) and `NOCTURNE_SNAP`.
5. **Cleanup.** The trap stops the Jellyfin and Chrome started by the script (by pid file), the headless session, and removes the keyring entries; the throwaway server's branding CSS is reset. Captures stay in `/tmp`.

Outputs per theme: `web-<screen>.png`, `app-<screen>.png`, `pair-<screen>.png` (web left, NOCTURNE right), `scores.json`.

## Score

Both images are shrunk to 256x144 (box filter). Per screen, 0 to 100:

`score = round(100 * (0.6 * SSIM + 0.4 * HIST))`

- `SSIM`: mean structural similarity of the luminance over non-overlapping 8x8 blocks (`C1 = (0.01 * 255)^2`, `C2 = (0.03 * 255)^2`), negative values clamped to 0. It measures layout and contrast structure.
- `HIST`: per-channel (R, G, B) 16-bin histogram intersection, averaged. It measures palette and overall brightness.

Rules of thumb: two different dark UIs of the same data score 40 to 55 (the demo posters and the black canvas match anyway); a pixel-near copy scores above 85. The number only ranks and tracks progress between runs, it is not a verdict: read the pairs. Known noise: the web screens show jellyfin-web chrome (cast icon, user avatar) NOCTURNE does not draw, the web home has no hero, and Jellyfin's detail layout differs by design.

## Applied / not applied counts

Debug builds write the active theme's report as JSON to `NOCTURNE_THEME_REPORT=<file>` when the key hook `theme-report@<ms>` fires (`crates/app/src/snap.rs`, handled in `app/themes.rs`): `applied` (tokens the theme set), `not_applied` (all warnings), `not_applied_by_kind` (the engine's warning kinds), `imports` (files downloaded, from cache, notes) and `fonts` (faces loaded). The harness reads it from the Home launch and fills the column as `applied / not applied (top three kinds); N fonts`. The "Import used" column shows the `@import` URL actually written to the server. An `open-item-<id>` hook does not exist yet: `Media` cannot be built from an id alone, so the detail screens still open the first grid card.

## Results

<!-- BEGIN GENERATED -->
Run: 2026-11-20 13:05, NOCTURNE commit `c22509c`, jellyfin server 12.4.0, 1920x1080.

| Theme | Home | Movies | Movie | Series | Mean | Applied / not applied | Import used | Main differences |
|---|---|---|---|---|---|---|---|---|
| Abyss | 82 | 79 | 85 | 83 | 82 | 5 / 662 (465 selectors not supported, 17 at-rules not supported, 14 properties not supported); 5 fonts | `cdn.jsdelivr.net/gh/AumGupta/abyss-jellyfin@main/abyss.css` | - |
| Better Jellyfin UI | 93 | 93 | 80 | 79 | 86 | 4 / 534 (455 selectors not supported, 36 at-rules not supported, 8 properties not supported); 1 fonts | `cdn.jsdelivr.net/gh/tromoSM/better-jellyfin-ui@main/theme.css` | - |
| Catppuccin | 89 | 95 | 91 | 88 | 91 | 4 / 169 (168 selectors not supported, 1 properties not supported); 0 fonts | `jellyfin.catppuccin.com/theme.css` | - |
| ElegantFin | 76 | 65 | 75 | 78 | 74 | 14 / 845 (437 selectors not supported, 38 at-rules not supported, 18 properties not supported); 12 fonts | `cdn.jsdelivr.net/gh/lscambo13/ElegantFin@main/Theme/ElegantFin-jellyfin-theme-build-latest-minified.css` (official import; a jf12 fork (mihaif7/elegantfin-jf12) exists, not used) | - |
| Evergarden | 89 | 95 | 91 | 88 | 91 | 4 / 169 (168 selectors not supported, 1 properties not supported); 0 fonts | `evergardentheme.github.io/jellyfin/evergarden-winter.css` | - |
| ijelly | 89 | 76 | 85 | 85 | 84 | 10 / 819 (434 selectors not supported, 42 properties not supported, 18 at-rules not supported); 0 fonts | `cdn.jsdelivr.net/gh/safiyu/ijelly@latest/ijelly.css` | - |
| infinitv | 76 | 70 | 69 | 70 | 71 | 6 / 1158 (484 selectors not supported, 16 at-rules not supported); 5 fonts | `buesche87.github.io/infinitv/infinitv-dev-v12.css` | - |
| Jellyfish | 81 | 77 | 74 | 77 | 77 | 6 / 1203 (447 selectors not supported, 49 at-rules not supported, 3 properties not supported); 2 fonts | `cdn.jsdelivr.net/gh/n00bcodr/jellyfish@main/theme.css` | - |
| JellyThemes | 80 | 76 | 84 | 83 | 81 | 6 / 728 (474 selectors not supported, 15 at-rules not supported, 10 properties not supported); 0 fonts | `cdn.jsdelivr.net/gh/kingchenc/JellyThemes@master/Themes/Solaris/Solaris.css` | - |
| Nebula | 86 | 95 | 72 | 80 | 83 | 9 / 878 (463 selectors not supported, 19 at-rules not supported, 16 properties not supported); 4 fonts | `cdn.jsdelivr.net/gh/nemesbak/jellyfin-nebula@main/nebula.css` | - |
| Netflin | 90 | 83 | 79 | 80 | 83 | 5 / 912 (475 selectors not supported, 20 at-rules not supported, 2 fonts only if installed); 6 fonts | `cdn.jsdelivr.net/gh/ahza-code/Netflin@latest/Theme/Netflin.min.css` | - |
| NeutralFin | 76 | 64 | 71 | 75 | 72 | 14 / 858 (443 selectors not supported, 32 at-rules not supported, 18 properties not supported); 12 fonts | `cdn.jsdelivr.net/gh/KartoffelChipss/NeutralFin@latest/theme/neutralfin-minified.css` | - |
| Scyfin | 86 | 94 | 81 | 86 | 87 | 4 / 286 (274 selectors not supported, 12 at-rules not supported); 0 fonts | `cdn.jsdelivr.net/gh/loof2736/scyfin@latest/CSS/scyfin-theme.css` | - |
| SleekFin | - | - | - | - | - | n/a | - | no documented @import line (JS builder or other install method) |
| Ultrachromic | 92 | 77 | 89 | 82 | 85 | 4 / 333 (292 selectors not supported, 33 at-rules not supported, 3 properties not supported); 5 fonts | `cdn.jsdelivr.net/gh/CTalvio/Ultrachromic/presets/monochromic_preset.css` | - |
| Zombie | 58 | 87 | 46 | 48 | 60 | 5 / 1231 (448 selectors not supported, 45 at-rules not supported, 3 properties not supported); 8 fonts | `cdn.jsdelivr.net/gh/MakD/zombie-release@latest/zombie_revived.css` | - |
| Finimalism | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| Finity | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| Flow | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| Jamfin | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| NetFin | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| Zesty | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| **mean of run themes** | 83 | 82 | 78 | 79 | | | | |
<!-- END GENERATED -->

## Round 4 notes (Jellyfin layout mode)

- **Done.** Radial and repeating (linear and radial) gradients are rasterised once per box size into a cached texture (`paint::rasterize`, drawn as an image layer by `widget/styled.rs::draw_raster`; smooth ones snap to 32 px steps while resizing, long edges are capped at 1600 px). `@keyframes` + `animation`: opacity, transform and colour, per-interval easing, delay, iteration count, direction, fill mode; driven by the existing frame request, endless ones at 30 per second, none under reduced motion, none on essential controls or hidden elements. `filter: brightness()` (a black layer: Zombie's and Jellyfish's backdrop dimming), `initial`/`unset` (ElegantFin's `margin-top: unset` was left at jellyfin-web's -7.2em: the item ribbon sat about 100 px too high), a page-sized `.backgroundContainer` on Home and library pages (Jellyfish's translucent teal), `data-monitor` / `data-positionticks` / the Next Up `href` (Zombie's Continue Watching grid), a hidden previous page with `.backdropPage` on the item page (Zombie dims the backdrop with `body:has(.backdropPage:not(.selfBackdropPage)) .backgroundContainer::after`).
- **Not done.** Conic gradients. Jellyfish's jellyfish artwork is not a gradient of the theme: it comes from the web client's own background picture, which the server UI mode does not draw. `blur`, `saturate` and `contrast` filters. Zombie Home: the small Continue Watching cards sit far right because `grid-auto-columns: var(--zf-s)` is `clamp(...)`, which the grid track parser does not read; Zombie's item page puts the title in the ribbon column and the clear logo is drawn behind the poster. Keyframes on virtualised library cards are not started. ElegantFin's backdrop is not zoomed like the web's.

## Round 3 notes (Jellyfin layout mode)

- **Harness fairness.** Run the harness on `cargo build --profile conformance -p nocturne-app --features theme-engine` (optimised code plus the debug-only snapshot hooks): `scripts/theme-conformance.sh --bin target/conformance/nocturne`. `NOCTURNE_ENGINE_TIMING=1` logs every styling pass; the slowest pass of every theme was 23 to 29 ms (Jellyfish included), the guard is now 300 ms (it was 50 ms and switched Jellyfish off in debug builds, which left the page unstyled and made it look like a stale backdrop: the series' picture at its natural size).
- **Fixed in the engine.** Gradient colour hints and `vw`/`vh` stop positions (ElegantFin's wrapper and backdrop dim); the item page's body is no longer `withSectionTabs` (a theme rule `.withSectionTabs .backgroundContainer.withBackdrop { background: solid }` hid the backdrop); the clear logo is an in-flow image filling `.detailLogo` (Abyss's `position: static` logo had filled the whole page); text beside `::before`/`::after` bullets gets its own box (Zombie "Nex"/"Rec", Nebula "Next Up" wrapping); `[class*="defaultCardBackground"]` (Nebula's dark tiles); `content: attr(title)` (Netflin's Resume label).
- **Not done.** Radial and repeating-linear gradients (Jellyfish's teal glow and jellyfish artwork, JellyThemes tiles): Iced's gradient is linear only, a faithful version needs a rasterised texture per box. `@keyframes` (`animScaleUp` on the backdrop, so the backdrop is not scaled up as in the browser), `backdrop-filter`, masks. The detail ribbon sits about 100 px higher than the web's under ElegantFin (min-height and negative margins of `.detailPageWrapperContainer`).
- A stale previous-item backdrop could not be reproduced with the optimised build: a `Detail` is built per item with its own picture table keyed by item, and the tree only has a backdrop layer when that item's image (or blurhash) is there.

## Engine gaps seen in the baseline

At commit `42dd6dc` the engine is not wired: NOCTURNE renders its own look whatever the server's CSS says, so the four app captures of every theme are identical and the scores only measure how close the stock look happens to be. This is the baseline for the engine steps D to F. What the pairs show, ordered by how many themes and screens it affects:

1. **Palette and surfaces (all themes).** Backgrounds, text colours, accent colours and the card/pill fills come from variables and plain rules (`:root` custom properties, `body`, `.skinHeader`, `.emby-button`). Largest single win; light themes (Catppuccin Latte) score lowest because NOCTURNE stays dark.
2. **Header.** jellyfin-web's `.skinHeader` / `.headerTabs` / `.headerButton` (left brand, icon links, floating or glass bars) versus NOCTURNE's centred text nav. Needs the compatibility class map and absolute/fixed positioning plus `backdrop-filter` fallback.
3. **Home composition.** Web shows "My media" tiles, Continue Watching, Next Up as rows of `.card`/`.itemsContainer`; NOCTURNE opens on the hero carousel. Needs the Server UI section order driven into the tree and row layout (`.sectionTitle`, `.padded-left`, scrolling `.itemsContainer`).
4. **Cards.** Card shape, radius, captions below the poster, unplayed badges, progress bars, hover scale (`.cardBox`, `.cardScalable`, `.cardPadder`, `.cardFooter`, `.cardText`, `transform`, `transition`).
5. **Library grid chrome.** Web has a slim toolbar (sort, filter, play all) and an alphabet rail; NOCTURNE shows its tab row, filters and a large title. Mapping `.itemsViewSettingsContainer`, `.alphaPicker` or accepting the difference as a NOCTURNE feature.
6. **Detail page.** Web: backdrop, poster, title block, button row, overview, cast (`.detailPagePrimaryContainer`, `.detailImageContainer`, `.itemName`, `.detailButton`); NOCTURNE: full-bleed hero with metadata. Needs the detail page as an element tree (step F2).
7. **Fonts.** Themes load web fonts through `@import` of font CSS or `@font-face` (Inter, Quicksand, Poppins, Noto): needs the theme-font loader (step C is merged; the app must use the faces) and `font-weight`/`font-family` lists.
8. **Effects with no native equivalent.** `backdrop-filter` blur, `mask-image`, animated gradients and blend modes fall back to the translucent fill or the nearest gradient and must be reported as such.
9. **Not reproducible without JavaScript.** SleekFin is a plugin (no stylesheet to import), Jellyfish and Netflin ship scripts or companion plugins (media bar, KefinTweaks, Jellyfin Enhanced); only their CSS part is measured.

## Blockers and caveats of this harness

- SleekFin: installed as a server plugin, no `@import`; skipped (listed in the table).
- Themes with colour-scheme variants are measured in their first documented variant only.
- Item screens use the first card of the grid (ordering matches jellyfin-web's default name sort for the demo library); a flaky navigation shows up as a pair whose NOCTURNE side is the Home hero. A debug hook to open an item by id would remove this.
- Chrome needs network access for the theme CDNs; a theme whose CDN is down renders unthemed and scores like the others.
- Full run over the 15 resolvable active themes takes about 25 minutes (about 90 s per theme, dominated by four app launches).
