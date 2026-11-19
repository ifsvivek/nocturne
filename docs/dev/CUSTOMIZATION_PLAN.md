# Customization plan: native CSS themes

Status: phase 1 (research, plan, UI-free engine in `crates/core/src/customize/`) done. Phase 2 (storage, Themes page, Iced wiring, URL import) done, see section 13 for what shipped and what did not. Layout themes (named parts, stage 1) are specified in section 14: the core engine and the app wiring are done (14.7). Requirements: `docs/dev/V05_BRIEF.md` section 2. No WebView, no browser engine, JavaScript is never executed.

## 1. Research: what Jellyfin community themes use

Sampled by reading the CSS itself (counts are occurrences in the file):

| Theme | Source | Size | Style |
|---|---|---|---|
| ElegantFin | https://github.com/lscambo13/ElegantFin (minified build) | 79 KB, ~740 rules | variable driven: 234 custom properties, 494 `var()`, 61 `border-radius`, 23 `backdrop-filter`, 29 gradients, 43 `calc()`, 28 `@media`, 4 `@keyframes`, 8 `url()`, one Google Fonts `@import` |
| Finimalism 7 | https://github.com/tedhinklater/finimalism | 90 KB, ~755 rules | selector driven: 3 custom properties, 84 `var()`, 31 `box-shadow`, 54 `transform`, 61 `@media`, 18 `url()`, 13 gradients (one with 17 stops), Google Fonts `@import` |
| Ultrachromic | https://github.com/CTalvio/Ultrachromic | many small files pulled in by `@import` | presets are almost only `@import` lines of other files plus a few `:root{--rounding:0px}` and `.backdropImage{filter:blur()...}` rules |
| JellySkin / Zesty | https://github.com/prayag17/JellySkin and https://github.com/prayag17/Zesty (checked, not measured) | build output via a toolchain | same families of constructs |

What they use, by category:

- Custom properties: the main palette mechanism in the variable-driven themes (`--accentColor`, `--textColor`, `--dimTextColor`, `--borderColor`, `--headerColor`, `--smallRadius`, ...). The names are theme-specific, not a standard. Maps well.
- Colours: hex 3/4/6/8, `rgba()`, `hsla()`, and generated `rgb(8.6% 2.3% 18.4%)` percentages. Maps well.
- Backgrounds: gradients (often long, many stops), `url()` images, fixed attachments, pseudo-element overlays. Plain colours and short linear gradients map; images and overlays do not.
- Fonts: `@import` of Google Fonts and `font-family` lists. Family names map only if installed.
- Radii: widely used. Maps well.
- Shadows, blur (`backdrop-filter`), transitions, animations, transforms: used everywhere for polish. Only a backdrop blur is plausible natively; the rest is out of scope for phase 1.
- Selectors: overwhelmingly tied to the jellyfin-web DOM (`.cardBox`, `.skinHeader`, `.itemsContainer`, `.detailPagePrimaryContainer`, descendant chains, `:hover`/`:focus`/`::before`, attribute selectors). Only the handful that name a whole kind of element (card, header, button, dialog, backdrop, page) have a NOCTURNE equivalent.
- Media queries: 28 and 61 blocks, mostly responsive layout for the web page. Not applicable.

Measured on the real files with the engine (one run, warnings list capped at 500): ElegantFin sets 14 tokens (accent and hover, text and secondary, edge, header, surface, control, danger, three radii, a gradient background, font family); Finimalism sets 7 (background colour, text, two radii, font family and size, a blur); the Ultrachromic base file sets none, the Novachromic preset sets 2 (blur, card radius) and warns about 14 `@import`s. Parsing takes about 12 ms for 90 KB in a debug build.

Realistic share that maps to native tokens:

- By rule or declaration count: roughly 5 to 10 percent (the rest is DOM layout, effects and responsive rules).
- By visual intent (palette, corner shape, font, page background): about 50 to 70 percent for variable-driven themes such as ElegantFin and Ultrachromic, and about 10 to 20 percent for selector-driven themes such as Finimalism, which is mostly layout and effects.
- Hence the honest promise: "import the colours, radii, font and background of a Jellyfin theme", never "your Jellyfin theme looks the same". Everything else is listed in the warnings so the user sees exactly what was skipped.

## 2. Parser choice

Hand-written reader (about 300 lines for the rule reader, 350 for values and gradients, no dependency) instead of `cssparser`.

- `cssparser` is MPL-2.0, which our `deny.toml` allows, and it is correct and fast, but it pulls in `phf`, `dtoa-short`, `itoa`, `smallvec` and `cssparser-macros` and returns a token stream: we would still write the rule splitter, the value grammar and the selector classifier ourselves, and still need our own position tracking and limits.
- The subset we translate is small and bounded: top-level rules, one level of declarations, balanced skipping of at-rule blocks, strings and comments. This needs no recursion and no tokenizer state beyond quotes and parentheses, so a hand-written reader is smaller, has no binary-size or licence cost, and is easy to bound (limits below).
- Revisit if we ever need real selector matching (phase 3+): then adopt `cssparser` plus `selectors` rather than extending ours.

Size impact: `Cargo.toml`, `Cargo.lock` and `THIRD_PARTY_NOTICES.md` are unchanged; the engine adds about 2k lines including about 800 of tests to `nocturne-core`.

## 3. Token model

A `Theme` is a set of optional overrides over the built-in tokens in `crates/app/src/theme.rs`; absent means "keep built-in". Dark-only stays; a theme may still choose a light palette by overriding all colours (contrast is the user's responsibility, see section 8).

| Token (`--nocturne-<name>`) | Type | Built-in (theme.rs) | Notes |
|---|---|---|---|
| `background` | colour or `linear-gradient` (max 8 stops) | `FRAME` | the window background; Iced `Background::Gradient` |
| `surface` | colour | `SURFACE` | panels, dialogs |
| `raised` | colour | `RAISED` | cards, hover surfaces |
| `header` | colour | `HEADER_BG` | header and breadcrumb fill |
| `text`, `text-secondary`, `text-tertiary` | colour | `TEXT*` | |
| `accent`, `accent-hover`, `accent-text`, `on-accent` | colour | `ACCENT*`, `ON_ACCENT` | focus ring uses `accent`; glow and tint are derived from it in phase 2 |
| `edge` | colour | `EDGE` | control outlines |
| `hairline` | colour | `HAIRLINE` | decorative dividers |
| `control`, `control-hover` | colour | `CONTROL*` | filled value controls |
| `danger`, `warn` | colour | `DANGER`, `WARN` | |
| `scrim` | colour | `SCRIM` | modal scrim |
| `radius-panel`, `radius-small`, `radius-card` | px, 0 to 48 | `R_PANEL` 10, `R_XS` 3, cards | `%` is of a 40 px box, so `50%` is a pill |
| `density` | number, 0.5 to 2 | spacing scale S1..S8 | multiplies the 4 px spacing scale |
| `font-size` | px, 10 to 32 | `BODY` 15 | the whole type scale scales by `font-size / 15` |
| `font-family` | name | Noto Sans | first non-generic name; used only if installed |
| `backdrop-opacity` | 0 to 1 | 1 | opacity of backdrop artwork on detail pages |
| `backdrop-blur` | px, 0 to 40 | 0 | blur of backdrop artwork where Iced can do it (an image filter pass on the backdrop only, never full-window) |

Not tokens in phase 1: shadows, transitions and animations (motion durations stay `HOVER/BASE/SLOW`), the type scale ratios, layout (`PAGE_PAD`, `HEADER_H`), card aspect and shape. The `card style` knob (poster aspect, labels below or overlaid) belongs to display settings, not CSS.

## 4. Selector vocabulary and mapping

Custom properties are read only from global scope (`:root`, `html`, `body`, `*`). Custom properties not in the tables below are kept as storage for `var()` and have no effect on their own.

NOCTURNE selectors (a "role" is a kind of element; one selector, no combinators):

| Selector | Role | Properties accepted |
|---|---|---|
| `:root`, `html`, `body`, `*` | page | `background`/`background-color` (colour or gradient) -> `background`; `color` -> `text`; `font-family`; `font-size` |
| `.card` | card | `border-radius` -> `radius-card`; `background(-color)` -> `raised` |
| `.header` | header | `background(-color)` -> `header` |
| `.button`, `button` | button | `background(-color)` -> `control`; `border-color` -> `edge`; `border-radius` -> `radius-small` |
| `.panel` | panel | `background(-color)` -> `surface`; `border-color` -> `hairline`; `border-radius` -> `radius-panel` |
| `.backdrop` | backdrop | `opacity` -> `backdrop-opacity`; `filter: blur(Npx)` -> `backdrop-blur` |

A detail-page role (`.detail`) is not defined yet; add it in phase 2 once the detail view exposes tokens beyond the backdrop.

Common jellyfin-web names accepted as aliases (same role, same properties):

| jellyfin-web | Role |
|---|---|
| `.backgroundContainer` | page |
| `.cardBox`, `.cardImageContainer`, `.cardScalable` | card |
| `.skinHeader` | header |
| `.emby-button` | button |
| `.dialog`, `.formDialog` | panel |
| `.backdropImage`, `.itemBackdrop` | backdrop |

Common jellyfin-web theme variables mapped to tokens (read in global scope):

| Variable | Token |
|---|---|
| `--accentColor`, `--accent-color` | `accent` |
| `--accentHoverColor` | `accent-hover` |
| `--textColor` | `text` |
| `--dimTextColor` | `text-secondary` |
| `--borderColor` | `edge` |
| `--headerColor` | `header` |
| `--drawerColor` | `surface` |
| `--selectorBackgroundColor` | `control` |
| `--btnDeleteColor` | `danger` |
| `--smallRadius` | `radius-small` |
| `--largeRadius` | `radius-panel` |
| `--rounding` (Ultrachromic) | `radius-card` |

Ordering rule: declarations apply in source order, last wins; `!important` is accepted and ignored (no specificity model). A `var(--x, fallback)` resolves `--x` from the global scope, then the fallback; a cycle or undefined variable without fallback skips that declaration with a warning. Values are limited to 4096 characters after substitution and `var()` nesting to 16, so fan-out bombs are bounded.

Supported value forms: colours (hex 3/4/6/8, `rgb()`/`rgba()` in comma or space syntax with `%` channels and `/ alpha`, `hsl()`/`hsla()` with `deg` or `turn`, `transparent`, about 35 named colours), lengths (`px`, `rem`, `em` of 16 px, `pt`, `%`), numbers and percentages, `font-family` lists, `linear-gradient()` with `deg`/`turn`/`to side` direction (stops with `%` positions; more than 8 stops are resampled to 8 with a warning).

## 5. Unsupported, and the warnings

The engine never drops anything silently: every skipped at-rule, selector, property, value or limit produces `Warning { line, col, kind, message }` (1-based, sorted by position, capped at 500 with a count of the rest). Kinds and typical text:

| Kind | Example message |
|---|---|
| `UnsupportedAtRule` | "`@media` blocks are skipped with the 412 characters inside"; "`@keyframes`: animations are not supported" |
| `UnsupportedSelector` | "selector `.a .b`: descendant and sibling selectors are not supported; 3 declaration(s) skipped" (also pseudo-classes/elements, attribute, id, compound, "not a NOCTURNE or known jellyfin-web class") |
| `UnsupportedProperty` | "animations and transitions are not supported"; "`box-shadow`: shadows and blur effects are not supported here"; "`margin`: layout properties are not supported; NOCTURNE keeps its native layout" |
| `UnsupportedValue` | "`currentcolor` is not a supported colour"; "functions such as calc() are not supported"; "gradients are only supported for the page background" |
| `RemoteResource` | "`@import` is not fetched; styles from other files or sites are ignored"; "url() resources are not fetched; the declaration was ignored" |
| `FontNotDownloaded` | "font `Inter` is used only if installed; fonts are never downloaded"; "`@font-face` is not supported: fonts are never downloaded or loaded from a stylesheet" |
| `UnresolvedVar`, `VarCycle` | "`--x` is not defined and has no fallback"; "`--a` refers to itself through var()" |
| `Approximated`, `Clamped` | "only the first of 4 values is used"; "radius 100 is outside 0..48; clamped" |
| `SyntaxError`, `LimitReached` | "unmatched `}`"; "stylesheet too large: the rest was not read" |

The UI shows the list grouped by kind with counts ("434 selectors not supported"), each entry with its line, and a summary line "N settings applied from M rules". Informational kinds (`FontNotDownloaded`, `Clamped`, `Approximated`) are shown dimmed.

## 6. Security

- Parsing is pure and offline: no I/O, no network, no file access, no `unsafe`. `url()` and `@import` are reported and never fetched in phase 1 (and image URLs stay out in phase 2; only a stylesheet URL typed by the user is ever fetched, see section 9).
- Nothing in a stylesheet is executed: no `expression()`, no `javascript:` (any `url()` is dropped), no `@import`, no HTML. Unknown functions are warnings.
- Limits: 256 KB of input (rejected before parsing), 5000 rules, 50000 declarations, 500 warnings, 4096 characters per resolved value, `var()` depth 16, gradient stops 8, font name 64 characters. The reader is iterative (no recursion on input nesting) and the fuzz-style test feeds random text and bytes. `parse_bytes` rejects non-UTF-8.
- Output is data only (`Theme`: colours, numbers, one short font name), validated and clamped, so a hostile stylesheet cannot produce an unusable UI by size values (radii, font size, blur and density are clamped). Colours are not forced to be legible: a "reset to default" action is always reachable by keyboard and the editor itself is not themed until the user confirms (live preview applies to a preview region first, see section 8).
- The stored theme contains no secrets, no server addresses and no media titles; the original CSS text is stored only if the user keeps "source" (it may contain whatever they pasted, so it lives in the user config dir, never in logs).

## 7. Storage

- Directory: `$XDG_CONFIG_HOME/nocturne/themes/` (`paths` helper in `nocturne-session`).
- `themes.toml`: versioned (`version = 1`), atomic write (temp file plus rename, like `settings.rs`), holds the list of saved themes `[[theme]] id, name, css (optional source text), tokens (the translated `Theme`)` and the assignment: `global = "<id>"` (optional) and `[server_theme]` mapping a source id (from `sources.toml`) to a theme id.
- Resolution order for a source: its own assignment, else the global theme, else built-in. Local library sources use the global theme; the per-source override applies to the screens of that source (library, detail, player chrome) while the shell (header, settings) follows the global theme, so a theme never makes the app unusable for another source.
- Why a separate file and not `settings.toml`: themes are large user content, edited and exported independently; the settings file stays small and diff-friendly. The `Theme` tokens (not only CSS) are stored so loading never has to re-parse, and a future engine version can re-translate from the stored CSS when it improves.
- Export is the original CSS (or generated `:root { --nocturne-... }` for themes made in the editor), so themes are shareable as plain `.css` files.

## 8. Live preview flow

1. The user opens Settings, Appearance: choose a saved theme, or "New theme" (paste, open file, or later URL).
2. Input goes to a multi-line text field (copy, cut and paste are required there, see brief section 3). Each change (debounced 300 ms, off the UI thread via the bridge as it can be tens of ms for a big sheet) runs `customize::parse` and shows the warning list.
3. Preview applies the resulting `Theme` to a sample panel in the editor (a card, a button, a header strip, a backdrop, text on each surface) with no change to the real app. A "Try it on the whole app" button applies it transiently with a 15 second auto-revert unless confirmed, so a bad palette is never a trap.
4. Save, rename, assign (global or this server), duplicate, delete, reset to built-in. A contrast report using the same WCAG checks as `theme.rs` tests flags text below 4.5:1 and edges below 3:1 on the resulting surfaces (advisory, not blocking).
5. Applying a theme switches the active `Theme` value in the app state; widgets read it on their next `view`.

## 9. Import paths

- Paste: the editor text field.
- File: a native file dialog for `.css` (the app already uses `rfd` with the xdg-portal backend for folder picking; reuse it). Read with a 256 KB cap, UTF-8 only.
- URL (phase 2): the user types or pastes one `https://` URL of a stylesheet (for example a raw GitHub file). Fetched through `nocturne_core::bridge::spawn` with the shared client: https only (http only for loopback), no credentials or cookies sent, redirects limited to 3 and to https, 10 s timeout, a hard 256 KB body cap enforced while streaming, `Content-Type` must be `text/css` or `text/plain` (raw GitHub serves `text/plain`), result goes into the editor like pasted text and is never applied unseen. The `@import` chain is still not followed; the warning tells the user to paste the imported file's URL too. Only the URL the user typed is contacted, once, on their action; no background fetching and no theme auto-update in v0.5.

## 10. Iced integration plan (phase 2, `crates/app` owner)

- Today `theme.rs` exposes `pub const` colours and sizes used directly by view code (about every file in `crates/app/src`). Keep the const names as the built-in defaults and add a `Tokens` struct (plain `f32`/`iced::Color` fields, `Copy`-friendly parts only) built once by `Tokens::resolve(&nocturne_core::customize::Theme)` which overlays the optional values onto the defaults, derives the dependents (accent tint 12% and glow 35% from `accent`, hover from `edge`, text-on-accent from luminance if `on-accent` is unset), scales spacing by `density` and the type scale by `font_size / 15`.
- The active `Tokens` lives in the root state and is passed to views by reference (as the couch scale already is); constant call sites change from `theme::ACCENT` to `t.accent`. This is a mechanical migration, done screen by screen; until a screen is migrated it keeps the built-in look, which is a safe partial state.
- Cost per frame: zero parsing and zero allocation at draw time. CSS is parsed on import or edit and the translated `Theme` is stored; at startup `resolve` runs once, and again only when the active theme or the source in focus changes (a source switch re-resolves, a few dozen field copies). Gradients become `iced::Background::Gradient(Linear)` built in `resolve`. Backdrop blur is the only effect that costs GPU: it is applied to the detail backdrop image once at load time into the image cache (key includes the blur radius), not per frame, and `backdrop-opacity` is a plain alpha.
- Font: `font-family` is mapped to `Font::with_name`. Iced needs a `&'static str`, so the name is interned once per distinct name (leaked, bounded by the 64-character cap and the number of themes tried in a session; documented ponytail).
- The `theme::tests` WCAG audit keeps running on the built-in defaults; add a test that every `Tokens::resolve(default)` equals the constants.

## 11. Phase 2 work list

1. `customize::store` in core: `themes.toml` load/save/assign with tests (atomic write, version, unknown fields ignored).
2. `Tokens::resolve` and the screen-by-screen migration in `crates/app` (separate worker, owns `crates/app`).
3. Appearance settings screen: theme list, editor with live preview and warning list, import from file; couch-mode and keyboard reachability.
4. URL import through the bridge (section 9) with tests against a local test server.
5. Display preferences from the requests log (#35: density, UI scale, home sections, accent presets) share `Tokens`: the accent presets are just built-in `Theme` values.

## 12. Open questions for the maintainer

1. Where does the theme editor live: Settings, Appearance (proposed), or its own top-level screen? It needs a large multi-line field, so a full page rather than a Settings row.
2. Per-server vs global: proposed default is global theme with optional per-source override; should local library sources be themeable separately from the shell, or always follow the global theme?
3. URL import policy: allow only https URLs typed by the user (proposed), or also support a curated list of known themes (needs a maintained list and network trust decisions)? Should an imported URL be remembered for a manual "refresh"?
4. Should a theme be allowed to change the font only from installed fonts (proposed), or may NOCTURNE later bundle one or two optional fonts?
5. Is advisory-only contrast checking acceptable, or should themes below 4.5:1 text contrast be refused for the shell?

## 13. Phase 2 as shipped

Maintainer decisions (2026-11-18): the editor is its own page (Settings > Themes), the theme is global with an optional per-server override, a typed https URL is fetched once and remembered for a manual Refresh, installed fonts only, contrast warnings are advisory.

- Storage: `customize::Store` in `store.rs`, `themes.toml` next to `settings.toml`, `version = 1`, atomic write, unknown or corrupt file gives an empty store. Only the CSS text and the optional import URL are stored (not the translated tokens): tokens are re-read from the CSS on load, which costs milliseconds and lets a better engine improve old themes. `global` and `[server_theme]` (source id as text to theme id) are the assignments; deleting a theme clears them.
- Which theme applies (`Store::active_id`): the override of the server on screen, else the global theme. "The server on screen" is the server of the top detail page, or the single server picked in the source filter; the shell (header, Settings, Themes), Home with all sources, and local folders use the global theme. The app re-resolves after a message only when this id changed (`App::sync_theme`), never while drawing.
- Runtime tokens: `theme::Tokens::resolve` overlays a `Theme` on the built-in constants (a test asserts the empty theme resolves to exactly the defaults), derives dependents when the parent changes (accent hover, text and tint, label colour on the accent, secondary text, control hover, header fill from the background) and is stored in one `RwLock`. The old `theme::ACCENT`-style constants for themable colours, radii and fonts became functions (`t::accent()`); spacing, type scale and layout constants are unchanged.
- Applied today: all colours, the three radii (the card radius defaults to the small radius), the window background as a colour or a gradient, backdrop opacity on the detail hero, and the font family. Not applied yet: `density`, `font-size` and `backdrop-blur` are parsed and counted but have no effect, because spacing and type sizes are plain constants used in layout geometry (migrating them is the next step). The font family is read once at launch (Iced's default font is fixed then), so a changed font needs a restart; colours and radii change at once.
- Themes page: saved-theme list, name, `text_editor` for the CSS (select, copy and paste work), file path field plus a native chooser, URL field with Download or Refresh, a summary ("N settings applied", counts per kind, advisory contrast lines) and the list of warnings with line and column, and Preview, Save, Apply, Reset to built-in, Delete (second click confirms). Preview applies the editor's theme to the whole app for 15 seconds, then reverts unless Keep; Esc reverts. Parsing runs through `bridge::spawn` after every edit and stale results are dropped by epoch (not debounced). The per-server picker cycles global then each saved theme.
- URL import: `customize::validate_url` and `customize::fetch` (https only, http only for loopback, no credentials in the URL, 10 s, at most 3 redirects to https, 256 KB counted while streaming, `text/css` or `text/plain` only). The result goes into the editor and is never applied unseen. Tested against a loopback server.
- Not done: a live sample panel inside the editor (the whole-app preview replaced it), keyboard shortcut for Reset, per-source themes for local folders (they follow global), export to a `.css` file, accent presets (item 5 of section 11), a real Jellyfin theme from the web has not been tried against the app (only the engine tests in phase 1).


## 14. Layout themes (stage 1, 0.5): named parts and their properties

Maintainer decision (2026-11-19): themes may change layout, not only colour ("write CSS to make it look like a streaming service or a video site"). A browser engine stays ruled out. Stage 1 is a fixed vocabulary of named app parts with a few typed properties each, translated by `customize/layout.rs` into a `Layout` next to the colour `Theme` (`Parsed.layout`). Stage 2 (0.6/0.7) puts a native flex/grid engine (Taffy) behind the same selectors and properties, see the end of this section. Everything here is read from the same stylesheet; colour-only stylesheets produce an empty `Layout` and parse exactly as before.

### 14.1 Selectors

One selector per rule item (lists with commas are fine), always one of:

- a part: `.card`, `.row`, ... (table below); `button` and `header` also work as tags;
- a scoped part: one scope, a space, one part: `.page-home .card`, `.section-resume .card`;
- a part under `:hover`: `.card:hover`, `.button:hover` only (the one pseudo-class, and only `transform: scale()` is read under it);
- a section on its own: `.section-latest { order: 2 }` (same as `.section-latest .row`).

Scopes: pages `.page-home`, `.page-library`, `.page-detail`, `.page-search`; home sections `.section-resume` (continue watching), `.section-next-up`, `.section-latest` (every "latest in library" row). A bare `.page-home` is a warning (write `.page-home .card`). Resolution for a view: built-in, then the unscoped rules, then the page's, then the section's; within a layer the last declaration wins. Any other pseudo-class (`:focus`, `:active`), pseudo-elements, descendant chains, compound selectors, ids and attributes still warn and are skipped. Custom properties named below are read on the part they belong to (`.card { --title-position: overlay }`), not on `:root`.

### 14.2 Parts and properties

Lengths are `px` (`rem`/`em` of 16 px accepted). Out-of-range values are clamped with a "clamped" note; wrong keywords give a warning and are skipped.

| Part | Selector | Properties (range) and what the app does |
|---|---|---|
| Card (whole cell) | `.card` | `aspect-ratio`: `2/3` poster image, `16/9` thumb (backdrop or episode still), `1/1` square; any other ratio snaps to the nearest with a note. `width` or `--card-size` (80 to 640 px) is the target card width; whole cards still fill the row exactly, as today. `--title-position: below\|overlay\|none` (caption under the image; title on a gradient inside the image; no caption at all). `transition-duration` or `transition` (0 to 1000 ms, only the duration is read). `display: none`. `border-radius` is the global card radius when unscoped (the colour token `radius-card`) and the image radius when scoped. `box-shadow` applies to the image. |
| Card hover | `.card:hover` | `transform: scale(1.0 to 1.3)`. Nothing else. |
| Card image | `.card-image` | `border-radius` (0 to 48), `box-shadow: <x> <y> [blur] [spread, ignored] [colour]` or `none` (offsets -32 to 32, blur 0 to 64; no `inset`, no multiple shadows) |
| Card caption | `.card-title`, `.card-meta` | typography (below), `display: none` |
| Row (a strip of cards) | `.row` | `display: carousel\|grid` (or `--row-layout`; `flex` is read as carousel): a horizontally scrolling strip or a wrapping grid. `gap` (0 to 64), `--row-columns` (1 to 12; grid only, absent means as many as fit), `padding` (first value, the horizontal inset of the row, 0 to 96), `display: none`. |
| Row title | `.row-title` | typography, `display: none` |
| Home section | `.section-resume`, `.section-next-up`, `.section-latest` | same properties as `.row` plus `order: <n>` (-100 to 100, lower first, ties keep the built-in order Resume, Next up, Latest) and `display: none`. `order` anywhere else warns. Also usable as scopes: `.section-resume .card`. |
| Hero | `.hero` | `--hero-style: billboard\|banner\|none` (full-bleed artwork behind a see-through header; a shorter band below the header; removed), `height` (120 to 1200), `background` (colour or `linear-gradient` laid over the artwork, for the fade into the page), `display: none` (same as style `none`) |
| Hero text | `.hero-title`, `.hero-meta` | typography, `display: none` |
| Header | `.header` | `background` (colour, `transparent` allowed; the existing `header` token), `position: fixed\|absolute` (overlays the content, which starts at the window top) or `static\|relative\|sticky` (own height, pushes content down), `height` (32 to 120), `--header-items` (space or comma list of `nav`, `search`, `history`, `account`; unlisted items are hidden, `none` hides all). The header itself cannot be hidden. |
| Header items | `.header-nav`, `.header-search`, `.header-history`, `.header-account` | `display: none` (or any other value to show again) |
| Header links | `.header-link` | typography |
| Button | `.button`, `button` | `border-radius`, `background(-color)`, `border-color` (existing colour tokens), `transition-duration`; `.button:hover` takes `transform: scale()` |
| Detail page | `.detail-title`, `.detail-text`, `.detail-backdrop` | typography for the first two; `.detail-backdrop` is the existing backdrop role (`opacity`, `filter: blur(Npx)`) |
| Player controls | `.player-controls` | `background` (colour or gradient behind the controls). The controls cannot be hidden. |

Typography (every text part above): `font-size` (8 to 96 px), `font-weight` (100 to 900, `normal`, `bold`, `lighter`), `text-transform` (`none`, `uppercase`, `lowercase`, `capitalize`), `letter-spacing` (-2 to 10 px or `normal`; `em` is taken of 16 px), `line-height` (a unitless number or percentage, 0.8 to 2.5), `color`. The font family stays global (`font-family` on `:root`, installed fonts only).

Global tokens that parse today but are not applied yet, and that the wiring work below turns on: `--nocturne-density` (spacing scale), `font-size` on `:root` (type scale), `backdrop-blur` (see section 13, "Not applied yet").

`display: none` hides a part. Any other `display` keyword on a part that takes `display` means "show" (so a later layer can undo a hide); `carousel`, `grid` and `flex` also set the row layout.

### 14.3 What stage 1 cannot do

Said plainly, so nobody expects a browser:

- Move a part somewhere else (the hero below the rows, the search box in the middle, a sidebar). The structure of each page is fixed; only the listed properties and the home section order change.
- Create elements, text or images: no `::before`/`::after`, no `content`, no `url()` backgrounds, no custom icons or logos.
- Arbitrary flex or grid: `display: flex|grid` as a general container, `flex-*`, `grid-template-*`, `margin`, free `padding`, `width`/`height` on anything except card width, hero height and header height, `min-*`/`max-*`, `position` other than the header's, `z-index`, `overflow`, `calc()`, `vh`/`vw`.
- Pseudo-classes other than `:hover` on `.card` and `.button` (no `:focus`, `:active`, `:nth-child`), `@media`, `@container`, `@keyframes`, `transition` of anything but the card and button hover, `transform` other than `scale`, multiple or inset shadows, `filter` other than the backdrop blur, `backdrop-filter`.
- Per-element targeting by state or content (the third card, a watched item, a particular library by name).

### 14.4 What stage 2 adds (native flex/grid engine)

The same selectors and the same properties keep their meaning; a Taffy-backed layout tree is added underneath. Stage 2 can then accept the layout properties stage 1 refuses on the existing parts: `display: flex|grid`, `flex-direction`, `gap`, `justify-content`/`align-items`, `grid-template-columns`, `margin`/`padding`, `width`/`height`/`min`/`max`, and `order` on any part, so parts can be rearranged inside their page (for example the header items in a different order, a hero beside a list). New parts (sidebar, rail) would be added as named slots, not as arbitrary elements. Because stage 1 `Layout` fields are plain optional values keyed by (scope, part), stage 2 only widens the structs and the property table. It still will not run a browser engine, `::before`/`::after` content, scripts, or remote resources.

### 14.5 Engine as shipped (core, phase A)

- `customize::Layout` holds `base: Parts`, `pages: BTreeMap<Page, Parts>`, `sections: BTreeMap<Section, Parts>`; `Parts` has one small struct of `Option` fields per part (`Card`, `CardImage`, `Row`, `Hero`, `Header`, `Button`, `Controls`, `Text` for each typography part, `Visibility` for the four header items). `Layout::parts(page, section)` returns the merged `Parts` for a view, `Layout::section_order()` the home sections in order, `Layout::overlay` merges layers like `Theme::overlay`, `Layout::count()` feeds the "N settings applied" line. A field is `None` when unset (keep the built-in), and `hidden: Some(false)` when a stylesheet un-hides.
- `customize::PRESETS` lists the built-in example themes (`Marquee`: dark, billboard hero, 16:9 carousels with hover zoom, no captions, red accent; `Feed`: dark grey, no hero, slim static header, wrapping grid of 16:9 thumbnails with title and details below, rounded corners). Their CSS lives in `crates/core/src/customize/examples/` and parses with zero warnings (tested), so the Themes page can offer them as ordinary themes.
- Card radius quirk: `.card { border-radius }` unscoped stays the global `radius-card` token (so existing themes are unchanged); scoped (`.page-home .card`) or on `.card-image` it sets `CardImage.radius`, which wins for that view.

### 14.6 Wiring work (app crate, later task)

Per file and part. Nothing here changes core:

1. `crates/app/src/theme.rs` and the root state: keep the active `Layout` next to the active tokens (`RwLock`, resolved in `sync_theme` by overlaying the global theme's layout, then the per-server theme's, the same way colours resolve); add `fn parts(page, section) -> Parts`. `HEADER_H`, `PAGE_PAD` and the `S*` spacing become functions of the layout and `--density` (this is also the pending `density`, `font-size`, `backdrop-blur` wiring from section 13).
2. `crates/app/src/app/themes.rs`: carry `Parsed.layout` through the reparse, add `layout.count()` to the "settings applied" summary, offer `customize::PRESETS` as built-in themes (a list above the saved themes).
3. `crates/app/src/widget/cell.rs` (`poster_cell`, `cell_h`, `CAPTION_H`, `tile`): image box from `card.aspect` and `card.width`; `title_position` (below keeps today's caption, overlay draws the title and meta on a bottom gradient inside the image `Stack`, none drops the caption and shrinks `cell_h`); `card_image.radius` and `shadow` (replace `t::r_card()` and `glow_shadow` for non-focused cells); `card_title`/`card_meta` typography in the caption texts; `card.hidden`; hover scale and `transition_ms` (Iced has no transform: draw the hovered cell larger by growing the image box inside the fixed cell and animate with `motion.rs`; the cell size stays fixed so virtualised grids keep working). Because `cell_h` feeds every virtualised grid, make it take the resolved card part instead of constants.
4. `crates/app/src/app/home.rs` (`dims`, `row_dims`, `band_h`, `strip_band`, `hero_band`, `hero_h`, `Home::layout`/`HRow`): per-strip card parts via `parts(Home, Some(section))` (Resume and Next up use `Section::Resume`/`NextUp`, each Latest row `Section::Latest`); `aspect` selects `Art::Poster` or `Art::Landscape` and the ratio in `row_dims`; `layout: grid` renders the strip as a wrapping grid (`--row-columns` or fit by width; reuse the library grid in `virt.rs`) and changes `band_h` accordingly; `gap` and `padding` replace `S3` and `PAGE_PAD` in the strip; section order and `hidden` change `Home::layout` (the keyboard navigation model follows it) using `Layout::section_order`; hero style, height, overlay and `hidden` replace `hero_h`, `HRow::Hero` exists only when shown, and a fixed header must overlay the billboard (hero content starts at y 0); `row_title`, `hero_title`, `hero_meta` typography.
5. `crates/app/src/app/header.rs` (`header`, `nav_link`, `nav_strip`, `nav_geom`): hide items from `header_nav/search/history/account` (keyboard focus and `nav_widths` must skip them); `header.height` and `position` (fixed: overlay in a `Stack` over the page and drop `HEADER_H` from the content offset; static: as today); `header_link` typography in `nav_link`.
6. `crates/app/src/detail/view.rs`, `detail/cells.rs`, `detail/geom.rs` (`TOP = HEADER_H + 29.0`, `hero_h`, `PORTRAIT`/`LANDSCAPE`, `card_w`/`card_h`): header position offsets, `detail_title`/`detail_text` typography, page-scoped card parts for the strips (`Page::Detail`), backdrop blur; geometry stays the single source of row heights.
7. Library, search and other grid pages (`crates/app/src/app/browse.rs`, `search.rs`, `virt.rs`): the card part under `Page::Library` / `Page::Search` for cell size, aspect and caption; header offset.
8. `crates/app/src/player/` (control bar view): `player_controls.background` replaces the fixed scrim.
9. Button styling (`crates/app/src/style.rs`, `widget/controls.rs`): `button.hover_scale` and `transition_ms` where buttons are drawn; the radius stays a colour-token concern.
10. Tests in `crates/app`: the empty `Layout` reproduces today's geometry exactly (`cell_h`, `band_h`, `hero_h`, `HEADER_H`), plus one test per part with a non-default value.

### 14.7 What shipped (app wiring, stage 1)

- Resolved once per theme change: `theme::set(theme, layout)` holds the active `Layout` (same single-layer resolution as the colours: server override, else server branding, else global) with `--nocturne-density` and the `font-size` scale; `theme::parts(page, section)`, `section_order()`, `header_h()`, `header_pushes()`, `header_item()`, `page_pad()`, `button()` read it. `Store::resolve_all` returns tokens and layout together. With no theme everything equals today's constants (tests in `app/home.rs` and `widget/cell.rs`; a before/after capture of Home, a library grid and a detail page was pixel-identical on Home and geometry-identical elsewhere).
- Cards (`widget/cell.rs` `Look`): `aspect-ratio` and `width` (with the matching artwork), `--title-position` below, overlay (title and details on a bottom gradient) or none (cell shrinks), `.card-image` radius and shadow, `.card-title` and `.card-meta` typography and hiding, `.row gap` as the cell padding (two paddings make the gap), `transition-duration`. Hover scale is `widget/zoom.rs`: the cell is drawn larger around its centre through a renderer transformation in its own layer, eased with the Iced animation helper and zero when reduced motion is on; the layout size never changes, so virtualised grids stay exact. The zoomed card may overflow its cell and the strip's clip.
- Home: per-section parts (`.section-resume`, `-next-up`, `-latest`), carousel or wrapping grid (`--row-columns` or fit), `.row padding` (horizontal inset), `order` and `display: none` for sections (the keyboard model follows the arranged rows), `.row-title` typography, hero billboard or banner, `height`, `background` overlay replacing the bottom fade, `display: none` removes the hero row, `.hero-title` and `.hero-meta` typography. Strip heights, scroll targets and image requests use the same `Geo` so they stay in step.
- Header: `height`, `--header-items` and `display: none` per item (hidden links are not in `nav_links`, so keyboard focus and `nav_widths` skip them), `.header-link` typography, `position: static` pushes the Home hero down by the header height (fixed keeps the billboard under it); every page already reserves `header_h()` at the top.
- Library grid: `.page-library` card part for aspect, width, caption and gap (the same `Look` feeds `metrics`, so scrolling stays correct). Detail: `.detail-title` and `.detail-text` typography, backdrop blur (see below). Player: `.player-controls background`. Buttons: `.button:hover` scale and `transition-duration` in `widget/press.rs`, which every button uses. Themes page: built-in examples (Marquee, Feed) offered as "use as starting point" copies; the summary counts layout settings.
- Degrades: `letter-spacing` is parsed but Iced 0.14 text has no tracking (ignored). `font-size` on `:root` zooms the whole UI through the application scale factor, like couch mode (Iced has no per-text global scale). `--nocturne-density` scales the page padding, header height and Home band padding, not every spacing token. `backdrop-blur` cannot blur an image: on the detail page it shows the item's blurhash stand-in instead of the sharp backdrop. Grid rows on Home are not navigated vertically inside a row (Up and Down move between sections). Not wired: card parts on the detail page's own strips (fixed sizes), `.page-search` (the search overlay has no cards), `display: none` on cards outside Home, the admin screens (they keep the built-in header height).

## 15. Remote parts of a theme (0.5.2): `@import` and `@font-face`

Maintainer decision #104: themes that link to other stylesheets and fonts must work, because most Jellyfin web themes are a thin file that imports the real one and a font sheet. `customize::import` (core, no UI) does the fetching; the parser itself stays offline and still never fetches anything (`url()` inside a declaration is still only a warning).

- **What is followed.** `@import url("https://...")` and `@import "https://..."` at the top level of a stylesheet, with an optional media query list that is kept as an `@media` wrapper for the engine. `layer(...)` and `supports(...)` after the address are not supported and are reported. Relative addresses are resolved against the importing stylesheet (the theme's own address when it was imported by URL; typed or pasted text has no base, so a relative import there is reported). Fetched sheets are inlined in place, so CSS order is kept, comments are blanked and the line count of the original text is preserved, which keeps every warning on the right line. `@font-face` blocks (in the text or in an imported sheet, with `url()` resolved against that sheet) are read, their fonts fetched, and the blocks removed from the expanded text.
- **Rules.** https only (plain http only for loopback in the tests, switch `Options::allow_loopback`, never set by the app); no credentials, cookies or `Referer`; the fixed User-Agent `NOCTURNE`; 10 s per download and 30 s per theme; at most 3 redirects, each of them https again (a redirect that breaks a rule is refused, not followed); addresses with a user name or password, `localhost` and literal loopback, private, link-local and unique-local addresses are refused so a stylesheet cannot make the app probe the local network; stylesheets `text/css` or `text/plain`, 256 KB each and 1 MB together, depth 3 (a file at depth 3 may not import), at most 16 imports, cycles refused by normalised address (fragment dropped); a fetched stylesheet with unbalanced braces is dropped as a whole so it cannot close or swallow the rules around it. Every failure is a `Note` (address, reason, line of the `@import`) shown as a warning; nothing is fatal and a failed import leaves the rest of the theme applied.
- **Fonts.** Same rules (https, no credentials, timeout, redirects) with 2 MB per file, 8 MB per theme, at most 12 files, content type `font/*`, `application/font-*`, `application/x-font-*` or `application/octet-stream`. The first bytes must be a font signature (TTF, OTF, TTC, WOFF, WOFF2), the declared decoded size is checked before decoding, and the decoded font is parsed by `ttf-parser` before it is returned; decoding runs inside `catch_unwind`. `src` entries are tried TTF/OTF first, then WOFF2, then WOFF (EOT and SVG fonts are never downloaded); `local()` is ignored; base64 `data:` addresses work without a download. Faces served as several `unicode-range` parts (Google Fonts) keep the Latin part. Output is `LoadedFont { family, weight, weight_max, style, internal_family, url, bytes }` with TTF/OTF bytes: the engine registers the font under `internal_family` (the name inside the file, which a text engine looks fonts up by) and maps the CSS `family` to it. WOFF and WOFF2 are decoded with the pure-Rust `wuff` crate (MIT, plus `brotli-decompressor`, BSD-3-Clause or MIT; both pass `cargo deny`, about 120 KB of source), so web fonts from Google Fonts and most themes work.
- **Cache and refresh.** Each download is kept in `$XDG_CACHE_HOME/nocturne/css/` (at most 256 files and 64 MB, oldest removed first), keyed by the normalised address with the fetch time (`Options::fetched_at`). A cached file is used as it is until a manual Refresh; a Refresh that fails falls back to the cached copy and says so. There is no background polling and no expiry: a theme is fetched when it is applied for the first time in a session, when the user presses Refresh, or when its text changes (`Store::needs_resolve` keys the expanded copy by a hash of the original text, so edited text or changed server branding is a new text). The expanded copy is session state (`Store::set_resolved`, never written to `themes.toml`); the stored theme and the saved branding stay the original text.
- **Setting.** `remote_css` in Settings (default on) turns all of it off: nothing is fetched, the text is parsed as it is, and the existing warnings ("`@import` is not fetched", "`@font-face` is not supported") say what was ignored. Turning it off also drops the fetched copies of the session.
- **Layering.** The look of a source is, from the top: the user's own per-source theme (an explicit choice), the branding CSS of the UI source in use (own server, or another server's, section 8 of `SERVER_UI_PLAN.md`), the global theme, the built-in look (`Store::active_with(source, ui)`).
- **Built-in themes.** `import::BUILTIN`: NOCTURNE (default, no CSS) and Glass (placeholder until its stylesheet lands). Third-party themes are never bundled or copied into the repository; the user pastes any Jellyfin CSS, an `@import` line or an address and the fetcher does the rest.
- **Tests.** `customize/import/tests.rs` runs a loopback HTTP server: nested and relative imports, media wrappers, line preservation, cycles, depth, count, per-file and total size, content type, https and private host refusal, redirects (to http, too many), timeout, cache hit, refresh and stale fallback, setting off, unbalanced sheets; fonts in TTF, WOFF and WOFF2 (the fixtures are hand-built fonts), content type, magic bytes, per-file, total and count limits, `unicode-range` parts, data addresses, cache, setting off; a large minified theme pasted as an `@import` line.

