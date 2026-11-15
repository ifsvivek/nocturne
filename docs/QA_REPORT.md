# QA report

Scope: Wayland and X11 (Xwayland) on Linux, release build. Read-only checks ran against a
a real Jellyfin server in API-key developer mode (a GET-only recording proxy counted requests: 0 non-GET
requests over the whole run, including one in-window playback). Everything with accounts or writes ran
only against the throwaway server from `scripts/test-jellyfin.sh`. No personal media details are
recorded here.

Legend: Y = yes, n/a = not applicable, - = not done, partial = see notes.

| Feature | Implemented | Unit-tested | Throwaway-integration-tested | Real-server read-only verified | Notes |
|---|---|---|---|---|---|
| Home (hero, Continue Watching, Next Up, latest rows) | Y | partial | Y | Y | Empty rows show "nothing here yet" (fixed) |
| Library grid: sort, filters, A-Z, Poster/Thumb/List | Y | Y (query building) | Y | Y | Fixed: sort menu painted under the grid; List rows were not full width |
| Genres, Studios, Years, Tags tabs | Y | partial | Y | Y | Sort/filter/view buttons hidden on name lists (did nothing) |
| Collections, Playlists, Favourites pages | Y | - | Y | Y (empty states) | Empty state said "loading" forever; now "Nothing here yet" |
| Search (Ctrl+K and /) | Y | Y | Y | Y | |
| Movie page | Y | Y | Y | Y | Hero was clipped at 1280x800; min height raised |
| Series page (logo hero, season tabs, episode card grid, cast, Next Up, More) | Y | partial | Y | Y | Two-season generated show on the throwaway: season tabs with counts, episode card grid, episode page with media info, Next Up starts playback and autoplays the next episode. Not seen on real multi-season data |
| Episode page, Person page | Y | partial | Y | Y | |
| Breadcrumbs, back/forward, Esc/Backspace | Y | Y | Y | Y | Header made more opaque so scrolled content no longer shows through |
| In-window playback: controls, audio/subtitle menus, speed, chapters, seek, fullscreen, trickplay hover, quality menu, stats overlay, clean stop | Y | Y (logic) | Y | Y (one playback) | Real-server playback made only GET requests. Stats overlay opened; the buffered value was not recorded numerically |
| Progress/played/favourite writes | Y | Y | Y | n/a (never on real) | Verified: writes-off sends nothing (server state unchanged, no session); writes-on updates favourite, played and resume position |
| Login, session restore, sign-out | Y | Y | Y | n/a | Restore via keyring across relaunch |
| Quick Connect | Y | Y | Y | n/a | Approved from a second session on the throwaway |
| 401 mid-session (token revoked on server) | Y | partial | Y | n/a | Returns to sign-in with "Your session ended". Admin page now reports its own 401 (was a known gap) |
| Server stop/start banner and reconnect | Y | Y | Y | n/a | Banner with retry; page reloads on return |
| Writes-off vs writes-on matrix | Y | Y | Y | n/a | See docs/WRITES.md |
| Admin: all 17 screens render | Y | Y | Y | n/a | View-only banner with writes off, controls dimmed |
| Admin writes via confirmation dialog | Y | Y | Y | n/a | Create/disable/delete user, refresh library, run task, create/delete API key, set config and transcoding field: all verified on the throwaway. Session message not exercised (no client advertised remote control) |
| Admin Esc | Y | Y | Y | n/a | One Esc leaves Admin; Backspace steps to the sidebar |
| Admin: Transcoding (view, edit 3 fields), Backups (list), library display/NFO/trickplay flags (read) | Y | - | Y | n/a | Backups endpoint exists on this server version (empty list) |
| Admin: Branding, Networking, task triggers, Trickplay, library add / remove / folders / options, plugin install / disable / enable / uninstall | Y | Y (mock server, validators, dialogs) | Y | n/a | Crate calls all ran against a throwaway Jellyfin 12.4. In the app: a login message save, a LAN network change (typed NETWORK), a daily trigger add and delete, and a library add and remove (media folder left in place) were done through the dialogs. Plugin install / disable / uninstall ran through the crate only; the catalog and the install dialog were seen in the app. Bundled plugins reject disable (server error), so the app offers those actions only for removable plugins |
| Account: Authorize a device, Your devices, `?` shortcut overlay | Y | Y | Y | n/a | Authorize approved a real Quick Connect code from the UI. The server lists only sessions it has seen since it started, so this device is always shown first |
| Photo viewer | Y | Y (actions) | Y | - | Full window, prev/next, zoom, Esc. Photo folders open as grids. Re-checked on generated throwaway photos: opens at 1/4, Right goes to 2/4, `+` zooms |
| Book / audiobook pages | Y | Y | Y | - | "Not playable in NOCTURNE yet", no Play button |
| HDR filter | n/a | n/a | n/a | n/a | Server exposes no HDR filter |
| Image / disk cache | Y | Y (LRU eviction) | Y | Y | Warm relaunch: 0 image requests. After the browse soak the cache held about 20 MB; the size cap was not reached, eviction is covered by the unit test only |
| Memory over 15 minutes of browsing (43 samples) | Y | n/a | n/a | Y | Resident set about 574 MB at the start, 589 MB at the end, flat after minute 1 |
| Lifecycle: 20 launch/quit cycles with playback | Y | n/a | Y | n/a | 17 window-close and 3 SIGTERM exits: no orphan mpv, no leftover sockets, no panics. Playback start itself was checked separately, not asserted per cycle |
| Startup | Y | n/a | Y | n/a | Window mapped in about 0.8 s, first painted content about 1.1 s |
| SIGTERM | Y | - | Y | n/a | Now a clean quit (was a hard kill). Settings are written on every change, so nothing is pending |
| X11 fallback | Y | n/a | Y | n/a | Reached the connect screen, signed in, external mpv started and exited with the app. Note: `wtype` cannot type into Xwayland; used `ydotool` |

## Defects fixed in this pass

- Admin page 401 did not return to sign-in; the admin client now records a 401 and the app acts on it.
- Two Esc presses were needed to leave Admin; one now leaves.
- Sort/filter menus rendered under the grid.
- List view rows did not span the width.
- Empty library tabs showed a permanent "loading"; empty home rows showed grey skeletons.
- Name-list tabs showed controls that did nothing.
- Hero clipped its buttons on small windows; header let content show through.
- "1 seasons" wording.
- Admin: "last used" showed a 739934-day age for never-used keys.
- Admin: detail panel with nothing to show was blank; now says it is view only.
- Photo albums and folders opened as a broken detail page; Book items showed a Play button that failed.
- SIGTERM killed the app without cleanup; the now-playing poster file in `$XDG_RUNTIME_DIR` was never removed.

## Not verified / known gaps

- Real server with several seasons (season cards), real server HDR, Photo viewer on real data.
- Cache size cap reached on a real library; multi-day soak.
- Session message action (no remote-controllable client available).
- (Fixed) the `crates/jellyfin-user` live test `playback_reporting_sets_resume_then_played` no longer assumes a 2 s clip: positions now scale with the item runtime.
- Accounts and playlists through the app UI on a throwaway: added a second account, switched to it and back without a password (the Admin entry followed the account), set and removed a profile image, set the display language, created a playlist (N), added items from an item's More menu, reordered and removed entries with the keys, renamed and deleted (typed name). Music queue: added an album's tracks from More, started the queue from the account menu, shuffle, repeat, remove; the queue survived stopping the player. The expired-token path of switching (401) is covered by a live crate test only; playback of the queued music happened in the external mpv window (embedded video was not used).
- Jellyfin lets any user see a public playlist but refuses edits (403) for non-owners; NOCTURNE uses CanDelete as the ownership test, so an administrator can edit every playlist.
- Plugin install, disable (after a restart) and uninstall, repository add / disable / remove, library rename (twice, back), provider reorder, and user access edits (feature switch, parental rating, tags, profile image upload and remove, display preferences read) were driven through the app UI against a throwaway Jellyfin 12.4. Plugin Enable, library access toggles and the other limit dialogs were checked through the crate live tests and dialog unit tests only.
- Jellyfin reports a pending plugin change as status "Restart" and keeps offering "Disable" until the server restarts; renaming a library gives it a new item id once the server has refreshed it; the profile image endpoint wants a base64 body, not raw bytes (the raw upload returns 500).
- Jellyfin lets any plugin endpoint "uninstall" a bundled plugin without error in one case we hit and refuses to disable it in another; NOCTURNE only offers these actions for removable plugins.
- Stale `nocturne-mpv-*` directories from earlier crashed runs may exist in `$XDG_RUNTIME_DIR`; the player only removes its own.
