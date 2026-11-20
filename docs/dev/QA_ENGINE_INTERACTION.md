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
