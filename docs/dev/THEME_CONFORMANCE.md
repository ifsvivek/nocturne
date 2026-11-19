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
Run: 2026-11-19 18:54, NOCTURNE commit `f0d6543`, jellyfin server 12.4.0, 1920x1080.

| Theme | Home | Movies | Movie | Series | Mean | Applied / not applied | Import used | Main differences |
|---|---|---|---|---|---|---|---|---|
| Abyss | 42 | 43 | 44 | 47 | 44 | n/a (not re-run) | `cdn.jsdelivr.net/gh/AumGupta/abyss-jellyfin@main/abyss.css` | Frosted-glass surfaces, rounded centred header, blur; palette, header, cards (gaps 1,2,4,8) |
| Better Jellyfin UI | 49 | 44 | 45 | 46 | 46 | n/a (not re-run) | `cdn.jsdelivr.net/gh/tromoSM/better-jellyfin-ui@main/theme.css` | Floating header, iOS-style rounded cards, animations (gaps 1,2,4) |
| Catppuccin | 15 | 13 | 49 | 52 | 32 | 0 / 174 (168 selectors not supported, 5 unknown variables, 1 properties not supported); 0 fonts | `jellyfin.catppuccin.com/theme.css` | Latte flavour is a LIGHT theme: white canvas vs NOCTURNE dark, lowest Home/Movies scores (gap 1) |
| ElegantFin | 42 | 41 | 42 | 44 | 42 | n/a (not re-run) | not re-run: the baseline used the jf12 fork `mihaif7/elegantfin-jf12`; the next run uses the official import `cdn.jsdelivr.net/gh/lscambo13/ElegantFin@main/Theme/ElegantFin-jellyfin-theme-build-latest-minified.css` | Rounded cards and buttons, translucent header pill, wide title block on detail (gaps 1,2,4,6) |
| Evergarden | 34 | 28 | 36 | 39 | 34 | n/a (not re-run) | `evergardentheme.github.io/jellyfin/evergarden-winter.css` | Muted green-grey palette on a dark teal canvas (gap 1) |
| ijelly | 54 | 54 | 58 | 61 | 57 | n/a (not re-run) | `cdn.jsdelivr.net/gh/safiyu/ijelly@latest/ijelly.css` | Apple TV style: large rounded cards, blurred header (gaps 1,2,4,8) |
| infinitv | 40 | 41 | 50 | 52 | 46 | n/a (not re-run) | `buesche87.github.io/infinitv/infinitv-dev-v12.css` | Dark card grid, Netflix-like header, hover scale (gaps 1,2,4) |
| Jellyfish | 43 | 38 | 40 | 38 | 40 | n/a (not re-run) | `cdn.jsdelivr.net/gh/n00bcodr/jellyfish@main/theme.css` | Teal glass floating header, jellyfish artwork background, Quicksand font (gaps 1,2,7,8) |
| JellyThemes | 49 | 45 | 52 | 54 | 50 | n/a (not re-run) | `cdn.jsdelivr.net/gh/kingchenc/JellyThemes@master/Themes/Solaris/Solaris.css` | Solaris variant: near-black with gold accent, floating glass top bar (gaps 1,2,8) |
| Nebula | 43 | 41 | 45 | 49 | 44 | n/a (not re-run) | `cdn.jsdelivr.net/gh/nemesbak/jellyfin-nebula@main/nebula.css` | Violet to cyan gradient accent, glass menus, Inter font, logo on movie pages (gaps 1,2,6,7,8) |
| Netflin | 59 | 59 | 49 | 57 | 56 | 5 / 1226 (468 selectors not supported, 28 at-rules not supported, 2 fonts only if installed); 6 fonts | `cdn.jsdelivr.net/gh/ahza-code/Netflin@latest/Theme/Netflin.min.css` | Netflix red on black, title-art hero, wordmark logo in the header (gaps 1,2,3,6) |
| NeutralFin | 42 | 39 | 51 | 48 | 45 | n/a (not re-run) | `cdn.jsdelivr.net/gh/KartoffelChipss/NeutralFin@latest/theme/neutralfin-minified.css` | ElegantFin derivative in black and grey (gaps 1,2,4,6) |
| Scyfin | 42 | 39 | 36 | 38 | 39 | n/a (not re-run) | `cdn.jsdelivr.net/gh/loof2736/scyfin@latest/CSS/scyfin-theme.css` | Backdrop-led pages, vertical drawer instead of tabs (gaps 1,2,6,8) |
| SleekFin | - | - | - | - | - | n/a | - | no documented @import line (JS builder or other install method) |
| Ultrachromic | 44 | 39 | 46 | 42 | 43 | 4 / 333 (292 selectors not supported, 33 at-rules not supported, 3 properties not supported); 5 fonts | `cdn.jsdelivr.net/gh/CTalvio/Ultrachromic/presets/monochromic_preset.css` | Monochromic preset: sectioned rows with bullet-marked titles, big continue-watching cards (gaps 1,3,4,7) |
| Zombie | 35 | 36 | 31 | 31 | 33 | n/a (not re-run) | `cdn.jsdelivr.net/gh/MakD/zombie-release@latest/zombie_revived.css` | AMOLED black, small header nav, square-ish cards, wide detail hero with round action buttons (gaps 1,2,4,6) |
| Finimalism | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| Finity | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| Flow | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| Jamfin | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| NetFin | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| Zesty | - | - | - | - | - | n/a | - | not run (hidden pending v12) |
| **mean of run themes** | 42 | 40 | 45 | 47 | | | | |
<!-- END GENERATED -->

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
