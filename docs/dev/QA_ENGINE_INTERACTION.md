# QA: engine interaction (0.5.2, `--features theme-engine`)

Method: engine debug build at 1600x900 in a headless sway session, throwaway Jellyfin with the demo library on a loopback port, real virtual pointer and keyboard. The pointer tool in the repo only moves and clicks, so a throwaway copy with wheel, right-click, double-click and drag was built under `/tmp/qai/hp` (not committed). The legacy build (`NOCTURNE_ENGINE_UI=0`) was used for comparison where noted. Captures are under `/tmp/qai/` (they may be gone later).

## Findings, by severity

### Major

1. Library (Movies, poster and list views, every theme): the mouse wheel does not scroll the grid or list. 12 items, the third row is cut off. Steps: Movies, hover the grid, wheel. Expected: scrolls (the legacy build does). Actual: nothing; scrollbar drag, End, Home and Page Down work. Captures: `lw5.png`, `lw7.png`, legacy `leg_lib.png`.
2. Detail page (engine, every theme): the page does not scroll at all. Wheel, Page Down, End and dragging the scrollbar do nothing, so Cast & crew and "Included in" (reached by wheel in the legacy build) are unreachable on movies. Captures: `d4.png`, `d9.png`, legacy `ld.png`.
3. Classic theme, Home: the row paging arrows beside each row title do nothing when clicked. "Latest in Movies" has 9 titles and 8 are shown, so the ninth cannot be reached by arrow; the legacy build scrolls one card per click. Captures: `ar1.png`, `lcl6.png`.
4. Search overlay: Down does not move the highlighted result, and hovering the second result does not highlight it (click does open it). Not yet compared with legacy. Captures: `s6.png`, `s7.png`.

### Minor

5. Header link hover (for example Photos) stays highlighted when the pointer leaves the header straight into the Home hero. Capture: `s34.png`.
6. The header has no backdrop while scrolled: headings and card text show through behind the nav, which overlaps the logo and links at small window sizes. Captures: `k4.png`, `rs_small.png`.
7. A header item (Smart on NOCTURNE, Playlists on Spotlight) looks highlighted with the active dot without hover or focus. In the engine only (legacy shows no dot).
8. Theme Studio: the Home hero carousel keeps rotating while the Studio is open, so the picked element jumps to another slide's tree and edits or undo cannot be verified reliably. The hero should pause while the Studio is open. Capture: `st5.png`.
9. Theme Studio: after Ctrl+Shift+0 (safe look) the toast and Themes page are right, but the open Studio preview keeps the edited look. A text colour edit (`#ff0000` on `span.button-label`) showed no visible effect on the Play label. Picking by mouse, a radius edit, Undo/Redo enabling and Done work. Capture: `safe.png`.
10. At 500 px wide the header clips "Home" against Search; at 600 and 900 px the nav collapses to an overflow arrow (fine). Capture: `rs_lib2.png`.
11. One black video frame with no controls was captured 3 s after starting playback on a cycle of the open/close test. Resident memory was 858 MB (debug build) after 5 open/close cycles; no baseline was taken, so a leak is unconfirmed.
12. The breadcrumb of an episode opened from search reads "Home > Movies > Salt in the Ledger".

### Same as the legacy build (not engine regressions)

- Card and row hover stays highlighted when the pointer leaves the grid into the toolbar; toolbar and player bar buttons show no hover change.
- A two-line title is centred on its first line.
- Mark watched and Favourite are dimmed with writes off, and clicking them does nothing (by design).

## What works

- Mouse: header link hover and active state, hero buttons hover, card hover with the favourite and watched badges, click opens the right item, Home wheel, scrollbar drag in the library, popovers (sort, filters, More menu) open, close on Esc and on an outside click, player pause and seek by click, click on the video toggles pause.
- Keyboard: Tab order and a visible focus ring that scrolls the page, Right/Down spatial moves on Home and in the library, Down/End/Home/Page Down in the library, `/` search and typing, Esc back from detail, player and search, Enter on a card.
- Themes Marquee, Feed, Classic, Spotlight and Glass on top of the engine: Home, library, detail and search behave as in the default theme; no essential control was hidden.
- Jellyfin layout ("All" from the server's UI): Home (My media), library and detail behave like the default layout, including findings 1 and 2. No Play All, shuffle or paging element that looked clickable but was dead was found apart from finding 3; the album page's Play all and Shuffle were not clicked.
- Resizing a floating window (500 to 1400 px wide) on Home, library and detail re-laid out without a crash; opening and closing the player 5 times left the app responsive.

## Not covered

- Right-click, double-click and tooltips: the tooltip test failed to stay open because the player bar hides while the pointer rests, and no right-click or double-click target was exercised.
- The 5,000-item library scroll was not run (the demo library has 12 movies and a local source of that size was not built).
- Player Space on a clean state and fullscreen.

## Fixed in 0.5.2 (re-run with the real virtual pointer)

Tooling: `tools/headless-pointer` has `wheel <dx> <dy>` (discrete notches) and `drag <x1> <y1> <x2> <y2>` (press, 20 motion steps, release); `scripts/headless-session.sh pointer` passes them through. Same method as above: engine debug build, 1600x900, demo library.

| # | Finding | Result | Notes |
|---|---|---|---|
| 1 | Library wheel | PASS | Root cause: the header scene's widget took every wheel event wherever the pointer was (no bounds check), so the page below never saw it. The wheel is now taken only over the widget's own bounds (regression test `a_wheel_below_the_widget_is_not_taken`). Wheel over the Movies grid scrolls to the third row. |
| 2 | Detail page does not scroll | PASS | Same cause. Wheel scrolls to Cast & crew and More like this; dragging the page scrollbar thumb now works (the page's own bar was not draggable, only `data-scrollbar` containers were). Page Down and End on the detail page do nothing in the legacy build either (an Iced scrollable has no key scrolling), so they were not added. |
| 3 | Classic row arrows | PASS | The arrows sent `HomeStripPage`, which scrolls an Iced scrollable that the engine does not have; the engine now scrolls the scene's strip (clicking next on "Latest in Movies" reaches the ninth title). |
| 4 | Search Down / hover | PASS | `Message::Eng` does not mark the scenes stale, so the key moved the selection in the app but the tree was never rebuilt; Down twice now highlights the third result. Hover on a result does show the row highlight (pixel (12,14,22) to (21,22,31), the same 4 % white wash as the legacy `GLASS`); the report could not be reproduced for hover. |
| 5 | Header hover sticks | PASS | Hovering a header link on Home also moved the keyboard focus onto it (row 0 cell), and `:focus` is styled like `:hover`. The pointer no longer parks focus on the header row. The legacy build clears it. |
| 6 | Header backdrop | NOT A DEFECT | Compared with the legacy build at the same scroll (`keys-legacy-2.png` against `keys-engine-2.png`): the hand-written header is the same 90 % fill and the hero shows faintly through it too, as on the legacy detail page. A solid backdrop was tried and reverted because `scripts/headless-parity.sh` (keys-2) then differed from the legacy build. A solid header on scroll is a theme rule (`page[data-scrolled="true"] .header`, as `glass.css` does). |
| 7 | Header item lit without hover | PASS | The Smart slot used "a library or a smart collection is current" for its lit state; it now lights only for a smart collection (Libraries only for a library), as the hand-written links do. |
| 8 | Hero rotates in the Studio | PASS | New `Pauses::studio` reason (unit test extended). |
| 9 | Studio text colour, safe look | PASS | The colour edit works (scripted `color Color #ff0000` on `span.button-label` turns Play red); the earlier miss was the rotating hero replacing the picked slide. Ctrl+Shift+0 with the Studio open reset only the app state, not the scenes (the chord arrives as `Message::Eng`, which does not mark them stale); after the fix the open preview drops the red at once. |
| 10 | 500 px header | PASS | Below 640 px wide the brand is dropped so the link strip keeps room (500 px: "Home Movies >" then Search). The legacy build still overlaps "Home" with the chevron at 500 px. |
| 12 | Breadcrumb of an episode opened from search | NOT FIXED | The trail is the navigation history (Home, the page the search was started from, the item) in both builds, by design; a show path would need the series page pushed first, which is a navigation-model change. |
| 11 | RSS and black frame | NO LEAK | Fast-release build, 20 open/close cycles of the player in a headless sway (software GL): 389 MB at start, 611 after the first (libmpv and GL load), 730 after 3, 789 after 15 and flat for the next 5. The step and plateau are the allocator's high-water mark, not growth per cycle. The demo clip stayed on "Opening" in this environment, so decoding was not part of the measurement. Six captures at 1 s steps after Play showed no black frame (mean brightness constant); one frame before the first video frame is the transparent hole by design, not reproduced. |

Gates after the fixes: fmt, clippy `-D warnings` (feature on and off), `cargo test --workspace --locked` (both), `scripts/headless-input-check.sh` 7/7. `scripts/headless-parity.sh`: Home, library, thumb and search captures of the default, Feed and Classic looks pass; the keys sequences are timing-sensitive on a loaded machine (the scroll after a key differs by one pixel in some runs of `keys-2`, and `keys-5`, the Home after Escape, differs on purpose: the engine restores the scroll position, see THEME_ENGINE_PLAN section 9); the library key sequences passed in the runs made at a load below 4.
