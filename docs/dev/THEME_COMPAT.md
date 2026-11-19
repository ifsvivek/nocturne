# Jellyfin web theme compatibility (selector translation)

Step F of [V052_PLAN.md](V052_PLAN.md). A Jellyfin web theme is a stylesheet written against jellyfin-web's DOM. NOCTURNE draws its own element trees (`home_tree.rs`, `header_tree.rs`, `detail_tree.rs`, `bar_tree.rs`), so a theme only applies where a Jellyfin selector can be rewritten into the selector of the element that plays the same role. That rewrite lives in `crates/style/src/compat.rs` (the translator) and `crates/style/src/compat_map.rs` (the tables). No theme code is stored in the repo; the measurement below reads themes fetched into `/tmp` at test time.

## How it works

- **Where.** `Engine::set_user_css` (the one place a user or server-branding sheet becomes engine rules) calls `compat::translate` first. NOCTURNE's own default sheet is added after that step and never goes through it. `Engine::set_compat(false)` switches it off. `Engine::compat_report()` returns what the translation did, and the lost selectors are also in the sheet's warning report under `Kind::Compat` ("jellyfin selectors with no counterpart"; informational, never a failure).
- **Recognition.** A sheet in which no Jellyfin-only name was met (no mapped class, id, variable or attribute) comes back unchanged with `applied == false`, so a stylesheet written for NOCTURNE (the starters, the stage-1 presets, `themes/*.css`) is never altered; the tests assert it. Names both vocabularies share (`.card`) do not count as a Jellyfin hint.
- **Text in, text out.** The walker copies at-rules it does not understand, descends into `@media`, `@supports`, `@layer` and `@container`, translates each style rule's selector list, appends mirrored custom properties to the same block and leaves declarations alone. The engine's own reader then parses the result, so every property limit and warning stays in one place.
- **Selector rules.** Each selector is split into compounds and combinators and every simple selector is translated:
  - A Jellyfin class or id maps to a NOCTURNE selector (a compound, a list written as `:is(a, b)`, or a short chain such as `.header-brand > .slot:first-child`). Mapping values are in `compat_map.rs`: `Sel` (maps), `Same` (the name is also NOCTURNE's), `Skip` (a wrapper, padding or behaviour class with no element), `Never` (a role NOCTURNE does not have; the reason is the report group).
  - Classes already in NOCTURNE's vocabulary (`card-title`, `hero`, `player-btn`, ...) pass through, so a sheet may mix both vocabularies.
  - Several Jellyfin compounds that are one NOCTURNE element collapse: `.cardBox > .cardScalable` is `:is(.card, .poster)`, `.card .cardBox` too. A spacer with no element (`.cardPadder`) drops out of `.cardPadder + .cardImageContainer`. A wrapper with no element (`.padded-left`, `.focuscontainer-x`, `.itemAction`) drops out of a chain and the descendant relation stays. A rule whose subject is a wrapper is reported.
  - `>` becomes a descendant combinator (counted in the report) because NOCTURNE nests several roles one level apart from Jellyfin (`.skinHeader > .headerTabs` is `.header .header-nav`, two wrappers deeper).
  - `:is`, `:where`, `:not` and `:has` lists are translated member by member. A member that cannot match is dropped from `:is`, `:where` and `:has` (an empty list loses the selector) and from `:not` (the negation of something impossible is true, so the clause disappears). `:enabled` and `:any-link` are always true here and disappear; `:disabled`, `:checked`, `:read-only`, `::placeholder`, scrollbar and thumb pseudo-elements have no counterpart and are reported.
  - Attributes: `[data-type=Movie]` becomes `[data-kind=movie]` (Series, Season, Episode, Photo, Book, Person, and BoxSet or Folder as `folder`), `[data-mediatype=Video|Photo|Book]`, `[data-isfolder=true|false]` become kind lists, `[href]` becomes the tag `a`, `[data-role=button]` becomes `.button`. NOCTURNE's own `data-*` pass through; Jellyfin's `data-action` values (resume, link, menu) do not.
  - Tags: `html` and `body` are `:root`; `button` is `a`; `h1`, `h2`, `h3` are the title, section title and page title leaves; `div`, `span`, `a`, `section`, `nav`, `header`, `i`, `img` pass through; `input`, `select`, `label`, `p`, `svg` and friends have no element.
  - `layout-desktop` is `:root`; `layout-tv` is `:root[data-layout=tv]` (see builder changes); `layout-mobile` can never match and is reported.
- **Specificity.** Dropping or merging a class lowers a selector's weight, which would let an earlier, lighter rule of the same theme win. After translating, the weight of the result is compared with the original's (the engine's own `specificity()`), and `:not(.jf-pad)` (a class that matches everything) is appended until the result is at least as heavy.
- **Variables.** A custom property a theme sets is mirrored onto the NOCTURNE token in the same block (`--jf-palette-primary-main`, `--accentColor`, `--theme-primary-color`... to `--nocturne-accent`; `--jf-palette-background-default` to `--nocturne-frame`; `AppBar-defaultBg` to `--nocturne-header-bg`; text, divider, paper, contrast, `--jf-card-borderRadius` to `--nocturne-r-card`, and a few community names; 20 names). A variable a theme only reads (`var(--jf-palette-text-primary)` without defining it, as the dark theme does) gets a default pointing at the token, written first so any later definition wins.
- **Limits raised.** `select::MAX_PARTS` 8 to 16 and `MAX_LEN` 300 to 500: a translated chain carries `:is(a, b)` lists and a four-level Jellyfin chain would otherwise hit the limit. The step budget that bounds matching is unchanged.

## The mapping (size)

388 class and id entries (`compat_map.rs`): 196 map to NOCTURNE selectors, 143 are roles NOCTURNE does not have (reported by reason), 49 are wrappers or behaviour classes dropped from chains; about 30 element ids (page roots and the detail page's collapsible sections) plus the `*Page` and `btn*` patterns, 20 variables, 6 attribute rules, 3 heading tags. The main families:

| Jellyfin | NOCTURNE |
|---|---|
| `.skinHeader`, `.MuiAppBar-root` | `.header` |
| `.headerTop` / `.headerLeft` / `.headerTabs` / `.headerRight` | `.header-bar` / `.header-brand` / `.header-nav` / `.header-icons` |
| `.emby-tab-button(-active)` | `.header-link(.active)` |
| `.headerSearchButton`, `.headerUserButton`, `.headerBackButton`, `.pageTitle` | `.header-search`, `.header-account`, `.header-brand > .slot:first-child`, `.header-name` |
| `.homeSectionsContainer`, `.verticalSection` | `.page-home`, `:is(.row, .section)` |
| `.sectionTitleContainer(-cards)`, `.sectionTitle(-cards)` | `:is(.row-head, .section-head)`, `:is(.row-title, .section-title)` |
| `.itemsContainer`, `.emby-scroller`, `.vertical-wrap` | `:is(.row-cards, .section-strip)`, `:is(.row-panel, .section-strip)`, `.row-grid` |
| `.card`, `.cardBox`, `.cardScalable` | `:is(.card, .poster)` |
| `.cardImageContainer`, `.cardContent` | `:is(.card-image, .poster-art)` |
| `.cardFooter`, `.cardText(-first/-secondary)` | `:is(.card-caption, .poster-caption)`, `.card-title` and `.card-meta` (and their poster twins) |
| `.countIndicator`, `.itemProgressBar(Foreground)`, `.cardOverlayContainer` | `.card-badge`, `.card-progress(-fill)`, `.card-ring` |
| `.portraitCard`, `.backdropCard`, `.squareCard` (and `overflow*`) | `:is(.card, .poster)[data-shape=...]` (needs builder change 1) |
| `.itemDetailPage`, `.detailPagePrimaryContainer`, `.detailImageContainer`, `.nameContainer`, `.itemName`, `.itemMiscInfo` | `.page-detail`, `.detail-hero`, `.detail-poster`, `.detail-head`, `.detail-title`, `.detail-meta` |
| `.mainDetailButtons`, `.detailButton`, `.btnPlay`, `.btnShuffle`, `.btnPlaystate` | `.detail-actions`, `.button`, `.button[data-action=play]`, `...shuffle`, `...watched` |
| `.overview-text`, `.itemGenres`, `.detailLogo`, `.backdropImage`, `.mediaInfoItem` | `.detail-text`, `.detail-genres`, `.detail-logo`, `:is(.detail-backdrop, .hero-img)`, `.chip` |
| `.detailSection(Header/Content)`, `#castCollapsible`, `#similarCollapsible` | `.section(-head/-box)`, `.section[data-section=cast]`, `...similar` |
| `.listItem`, `.listItemImage`, `.listItemBody(Text)` | `:is(.episode, .entry)`, `:is(.episode-thumb, .entry-still)`, `:is(.episode-body, .entry-info)`, title and meta leaves |
| `.emby-button`, `.button-flat`, `.raised`, `.button-submit` | `.button`, `.button`, `.button`, `:is(.button.primary, .button.button-primary)` |
| `.osdTextContainer`, `.osdPositionSliderContainer`, `.btnPause`, `.btnNextTrack`, `.btnMute` | `.player-info`, `.player-seek`, `.player-btn[data-action=pause]`, `...next`, `...mute` |
| `[data-type=Movie]`, `html`, `body`, `.layout-desktop` | `[data-kind=movie]`, `:root`, `:root`, `:root` |

## Measurement

`NOCTURNE_COMPAT_THEMES=/tmp/compat-themes cargo test --release -p nocturne-style compat_measure -- --ignored --nocapture` (the ignored test in `crates/style/src/tests/compat.rs`) over the CSS of the awesome-jellyfin themes (the `themes.json` list of the conformance harness, `@import`ed files resolved one level deep, fetched at test time; nothing from them is stored). 2026-11-19, 21 themes, 15,795 selectors, 27,157 declarations. Columns: selectors translated; selectors with no counterpart by design (a role NOCTURNE does not have: forms, dialogs, side drawer, mobile layout, live TV...); selectors whose class or id name is not in the map (the actionable remainder: plugin classes, page-specific wrappers); declarations in a rule that kept at least one selector; declarations the engine compiled from those rules; the same count for the untranslated sheet restricted to rules that name no class, id or attribute (the only ones that could match without the layer).

| Theme | Selectors | Translated | By design | Not in map | Declarations | In kept rules | Compiled | Without layer |
|---|---|---|---|---|---|---|---|---|
| Abyss | 718 | 38% | 47% | 14% | 1016 | 38% | 36% | 2% |
| Better Jellyfin UI | 519 | 24% | 61% | 16% | 1152 | 22% | 19% | 2% |
| Catppuccin | 173 | 38% | 38% | 24% | 58 | 67% | 67% | 7% |
| ElegantFin | 950 | 42% | 41% | 18% | 1589 | 48% | 43% | 8% |
| Evergarden | 174 | 39% | 37% | 24% | 84 | 77% | 77% | 36% |
| Finimalism | 1672 | 40% | 36% | 24% | 2442 | 36% | 31% | 1% |
| Finity | 224 | 43% | 38% | 19% | 669 | 64% | 61% | 19% |
| Flow | 321 | 34% | 54% | 13% | 494 | 41% | 35% | 4% |
| Jamfin | 265 | 40% | 46% | 14% | 289 | 53% | 43% | 8% |
| JellyThemes | 757 | 30% | 56% | 14% | 1322 | 42% | 41% | 13% |
| Jellyfish | 1249 | 21% | 68% | 11% | 2550 | 19% | 17% | 2% |
| Nebula | 932 | 43% | 41% | 15% | 1593 | 44% | 40% | 2% |
| NetFin | 1099 | 42% | 41% | 17% | 1689 | 49% | 43% | 5% |
| Netflin | 1286 | 18% | 63% | 19% | 3013 | 23% | 19% | 4% |
| NeutralFin | 972 | 42% | 40% | 18% | 1537 | 49% | 44% | 8% |
| Scyfin | 271 | 32% | 58% | 9% | 442 | 41% | 31% | 4% |
| Ultrachromic | 228 | 52% | 37% | 11% | 217 | 57% | 54% | 2% |
| Zesty | 1081 | 28% | 59% | 13% | 1482 | 28% | 25% | 2% |
| Zombie | 1438 | 24% | 66% | 10% | 3040 | 23% | 21% | 2% |
| ijelly | 966 | 29% | 39% | 31% | 1590 | 36% | 34% | 5% |
| infinitv | 500 | 35% | 47% | 18% | 889 | 52% | 12% | 4% |
| **all** | 15795 | 33.0% | 50.0% | 17.0% | 27157 | 35.5% | 30.7% | 4.5% |

Reading the numbers. About half of every Jellyfin theme targets things NOCTURNE does not draw the Jellyfin way (the mobile layout alone is 8% of all selectors, forms and dialogs, the dashboard, live TV, the side drawer), so 33% translated is closer to two thirds of what is reachable. The declarations that compile are 7 times more than without the layer (4.5% to 30.7%); the untranslated figure is the optimistic one, since it counts rules on `:root`, `html`, `a` and tags that happen to exist in the trees. "Compiled" is not "visible": a declaration can compile and still describe a property the paint widget draws as a documented fallback (`backdrop-filter`, `transform`); those stay in the sheet report as `Approximated`. infinitv compiles 12% of what it reaches because most of its declarations are properties the engine rejects, not selectors. The most frequent reasons (selectors, all themes): name not in the map 3,161 plus 261 ids; `.layout-mobile` 1,274; attribute selectors 825 (`[href*=...]`, `[dir]`, `[data-theme]`); hand-written pages 630; form fields 496; wrappers with no element 323; menus 305; dialogs 257; side drawer 257; tags with no element 227; the player bar container 202; play overlay buttons 166; version and track selects 165.

## Builder changes needed (not made here; the trees are not mine)

These are the structures translation cannot express. Each lists the element, class and parent so a later step can add them without reading this layer.

1. **Card shape.** `a.card` (`home_tree::card_nodes`) and `a.poster` (`detail_tree`) get `data-shape="portrait|backdrop|square"` from the strip's `Art` (poster, landscape or backdrop, square). Unlocks `.portraitCard`, `.backdropCard`, `.squareCard` and the `overflow*` forms: several hundred selectors across the sample, the largest single family.
2. **Layout flag.** The root of every tree (`Tree::new`, `DTree`, `BarTree`) gets `data-layout="desktop"` or `"tv"` (couch mode). The map already sends `.layout-tv` to `:root[data-layout=tv]`; until the attribute exists those rules do not apply.
3. **Player bar container.** `div.player-bar` as a child of the root holding `.player-row-1` and `.player-row-2` (and the frame). `.videoOsdBottom`, `.videoOsdBottom-maincontrols` and `.osdControls` then map to `.player-bar` (202 selectors).
4. **Played versus favourite chip.** `a.card-chip` gets `data-action="favourite"` or `"watched"` (and `.on` already marks the state) so `.playedIndicator` can map to `.card-chip[data-action=watched].on` instead of any chip.
5. **Card play button.** `a.card-play` inside `.card-image` (a play fab shown on hover and focus, `data-action="play"`): the target of `.cardOverlayButton*`, `.cardOverlayFab-primary` (166 selectors). Needs a default-sheet rule so stock NOCTURNE stays as it is (hidden unless a theme shows it).
6. **Parent name line.** `div.detail-parent` (series or album link) above `.detail-title`: `.parentName`, `.musicParentName` (133).
7. **Track selection block.** `div.detail-tracks` after `.detail-actions` (slots for version, audio and subtitle choices): `.trackSelections`, `.selectContainer` (165).
8. **Library and search roots (step E).** `data-screen="library"` and `"search"` on the roots (the map assumes these names for `#moviesPage`, `#tvshowsPage`, `#musicPage`, `#searchPage`), `div.library-toolbar` with `a.button[data-action=sort|filter|view]` and `div.alpha-picker`: `.btnSort`, `.btnFilter`, `.btnSelectView`, `.alphaPicker*`, `.itemsViewSettingsContainer`. Library cards should carry the same classes as Home cards so everything in the card family applies.
9. **Player extras.** `div.player-header` (back and title across the top), `div.player-up-next`, `a.button.player-skip`: `.osdHeader`, `.upNextContainer`, `.skip-button` (251 selectors). Only if the player ever grows them.
10. **Home tiles.** `.homeLibraryButton` is mapped to `.section-tiles :is(.card, .poster)`; the "My media" tiles are cards today, so nothing to add.

Not planned (hand-written screens, reported by reason): sign-in, settings, dashboard, live TV guide, dialogs, action sheets, forms, the side drawer, multi-select, chapter cards, mini player.

## Next fixes the numbers point to

1. Builder changes 1 and 2 (card shape, layout flag): the largest translatable families, a few hundred selectors per theme.
2. Builder changes 3, 5, 6, 7: each is about 130 to 200 selectors across the sample.
3. The `NOCTURNE_THEME_REPORT` hook of the fixes worker should include `Engine::compat_report()` (translated, by design, not in the map, top reasons) so the conformance table's applied / not-applied column fills with these numbers.
4. Run the conformance harness again (baseline mean 41 / 39 / 45 / 47 for Home / Movies / Movie / Series): the palette gap (gap 1) is now reachable through the `--jf-palette-*` mirrors and `html`, `body`, `.skinHeader`, `.emby-button` rules; the header gap (gap 2) through `.headerTabs`, `.headerRight` and `.emby-tab-button`; cards (gap 4) through the card family. Check in a headless session that a `:root` background (from `html`, `body` and `.backgroundContainer`) is painted by `widget/styled.rs` (the style is computed; painting the root quad was not verified here) and that `:root[data-screen=...]` matches the root the trees create.
5. The unmapped long tail is plugin and page-specific (KefinTweaks, Jellyfin Enhanced, jellyseerr, Streamystats, subtitle renderers) and not worth entries; add names only when a theme in the conformance list needs them (`.watchlist`, `.person`, `.subtitle`, `.libraryDocument`, `.transparentDocument` are the most frequent today).
6. Light themes (Catppuccin Latte) still look wrong until the tokens that the tree builders read inline (`t::text()` and friends in the slots) follow `--nocturne-*`; the translator already mirrors the palette.
