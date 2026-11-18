# Customization plan: native CSS themes

Status: phase 1 (research, plan, UI-free engine in `crates/core/src/customize/`) done. Phase 2 (storage, editor UI, Iced wiring, URL import) not started. Requirements: `docs/dev/V05_BRIEF.md` section 2. No WebView, no browser engine, JavaScript is never executed.

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
