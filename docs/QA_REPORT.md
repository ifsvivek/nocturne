# QA report

Scope: the released 0.2.0 (GPUI interface, Jellyfin only); the Iced build in development has not been through this
report yet. Wayland and X11 (Xwayland) on Linux, release build. Read-only checks ran against a
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
| Series page (logo hero, season tabs, episode card grid, cast, Next Up, More) | Y | partial | Y | Y | Two-season generated show on the throwaway: season tabs with counts, episode card grid, episode page with media info, Next Up starts playback and autoplays the next episode. Seen on a real two-season show in the second pass (season tabs with episode counts, episode cards) |
| Episode page, Person page | Y | partial | Y | Y | |
| Breadcrumbs, back/forward, Esc/Backspace | Y | Y | Y | Y | Header made more opaque so scrolled content no longer shows through |
| In-window playback: controls, audio/subtitle menus, speed, chapters, seek, fullscreen, trickplay hover, quality menu, stats overlay, clean stop | Y | Y (logic) | Y | Y (one playback) | Real-server playback made only GET requests. Stats overlay opened; in the second pass the Buffered row showed a numeric value (about 287 s) during a direct-play playback |
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

## Second pass: features added since the first pass

Same safety rules: the real server was used only through a GET-only recording proxy (199 requests, 0 non-GET,
one in-window playback with server changes off), every write and sign-in ran on a throwaway Jellyfin 12.4 with
isolated XDG directories and a unique keyring name. "UI" means driven through the release binary.

| Feature | Implemented | Unit-tested | Throwaway-integration-tested | Real-server read-only verified | Notes |
|---|---|---|---|---|---|
| Account switching (add account, switch without a password, Admin entry follows the account) | Y | Y | Y (crate live tests, two stored tokens) | n/a | UI: signed in as a second user, switched to the administrator and back. The expired-token (401) path of a switch is covered by a crate live test only |
| Plugin repositories, library rename, metadata / image providers | Y | Y (validators, mock server) | Y (crate live tests, all passed again) | Y (Providers and Plugins pages, view-only banner, actions dimmed) | Writes were not repeated through the UI in this pass (done in the first pass) |
| User access and limits | Y | Y | Y (crate live tests) | Y (library and feature switches listed, controls dimmed) | Dialogs for the less common limits are covered by unit tests only |
| Playlists (create, add, reorder, rename, delete) | Y | Y | Y (crate live tests) | Y (empty-state page only: the real server has none) | UI this pass: create with N and add a track from More; reorder, rename and delete were not repeated in the UI |
| Music queue | Y | Y | partial | n/a | UI: Add to queue from More, then "Play music queue (1)" and "Clear music queue" appeared in the account menu. Queue playback was not exercised in this pass |
| Shortcut overlay (`?`) | Y | Y | n/a | n/a | UI: opened and closed. Long descriptions were clipped at 1269x689 (fixed) |
| Quick Connect: authorize a device | Y | Y | Y | n/a | UI: approved a code requested from a second client; the server then reported it authenticated |
| Your devices | Y | Y | Y | n/a | UI: lists this device and the other session |
| Season cards, episode grid | Y | partial | Y | Y | Real two-season show and a generated three-season show |
| Photo viewer | Y | Y (actions) | Y | n/a (the real server has no photo library) | UI on generated photos: opens a folder grid, Right steps, `+` zooms, Esc closes |
| Collections and Playlists pages | Y | - | Y | Y (empty states) | |
| Search | Y | Y | Y | Y | Regression found and fixed: typing `a` opened the account menu instead of entering text |

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
- Second pass: typing the letter `a` in the search box opened the account menu (the new account shortcut was checked before the search box); the shortcut overlay clipped long descriptions on narrow windows.
- Second pass (test tooling): `scripts/test-jellyfin.sh` could start before Jellyfin was ready to be configured, and the admin live test that restarts the server restarted the default instance instead of the one under test.

## Settings drop-downs and scrolling (unreleased)

Throwaway Jellyfin on port 8098, isolated XDG dirs, developer-token mode, 1280x720 window, debug build.
Checked live: the page scrolls and End / Down move the selected row into view (the last row, Image cache, is
reachable); Enter opens the drop-down under the chip, or above it when there is no room below (it painted over the
rows, not under them); the menu scrolls, opens on the current entry (a hand-edited unknown value `xx-custom` appeared
as its own marked entry and as the chip text); Home / Down / Enter chose English and `settings.toml` then held
`audio_lang = "eng"` (stored format unchanged). Unit-tested: the menu model (open, clamp, Up / Down / Home / End,
Enter / Space, Esc, ignored keys), the choice lists and picks per setting, unknown stored values and language aliases,
and the scroll-child index and navigation-key logic. Not verified live: mouse clicks on chips and options, click-away
and mouse-wheel scrolling (the desktop was shared with other sessions and focus and pointer moved under the
test, so only the keyboard path was driven); Esc closing the menu was seen once but not repeated cleanly. Library
Sort / Filter menus, player menus and admin dialogs were checked for click-to-cycle controls: none exist, so nothing
else changed. Left alone on purpose: player repeat, A-B loop and speed controls (cycling is the expected behaviour
there) and the library `V` view shortcut.

## Profile picture in the header avatar (unreleased)

Throwaway Jellyfin on port 8099 (users alice, bob, admin), isolated XDG dirs and keyring name, a private copy of the
debug binary started from a directory without a `.env`, synthetic PNGs made with ffmpeg. Checked live: alice with an
uploaded picture showed it, clipped to a circle, in the header after a restart; the account menu header showed it at
40 px with the server address kept under the name; a picture replaced on the server (new tag) was shown after a restart;
setting one from the app (Profile image dialog) and removing one (Remove profile image) changed the header avatar at
once, with the initial back after removal and `accounts.toml` following the tag; adding bob and switching back and
forth updated the avatar without a restart; the "Switch to bob" row showed bob's picture and "Switch to alice" the
initial while alice had none; the Admin Users list showed both pictures and the initial for the admin with none.
Unit-tested: the disk cache key (server, user, tag, size), the avatar fallback (no tag, loading, failed, old tag, ready),
the image URL, and the `image_tag` field in `accounts.toml` (round trip, old files, only saved when it changes). Not
verified: a failing image download (initial fallback is unit-tested only) and mouse clicks (the pointer could not be
moved reliably; the keyboard path was used). Note for testers: a first attempt ran from the repository directory and
loaded the real `.env` developer token (GET reads only, nothing changed); it was stopped at once, and later runs used a
directory without `.env`.

## Profile picker (unreleased)

Throwaway Jellyfin on port 8100 (alice with an uploaded picture, bob, carol, admin), isolated XDG dirs, a unique
keyring service name, the debug binary started from a directory without a `.env` and with the developer variables unset.
Each user was signed in once through the app's own forms so the tokens were remembered. Checked live: the picker opens at
launch with the picture on alice and initials on bob and carol (pictures load without a token); Right / Left / Space /
Enter and a mouse click choose a profile and open Home as that user; ticking "Use as default profile" and choosing wrote
`default = true` for that account only and the next launch went straight to Home as that profile, also when another
profile had been used last; "Switch profile" reopened the picker with the default highlighted and the box ticked, and
choosing another profile without touching the box left the default alone; the Settings "Default profile" drop-down
changed the default to another profile and back to "Ask every time" (marker removed); signing out of the default account
removed the marker; revoking the app's tokens on the throwaway made the pick fall back to the sign-in form with the name
filled in, the password field focused and "Your session ended" shown, Esc returned to the picker, and a password sign-in
then worked; with one remembered account the launch showed the sign-in form, no picker. Unit-tested: the start-up
decision (0, 1, N accounts, default set, unset, stale or doubled marker, other server's default, developer-token mode),
selection (wrapping Left / Right, Up / Down in wrapped rows, Enter, Esc, ignored while signing in), the default box
(set, keep, clear), the `default` marker in `accounts.toml` (one per server, cleared by forgetting, round trip, older
files), the Settings row and the name prefill. Not verified: the wrapped layout in a narrow window (columns and
navigation are unit-tested only), a failed server at launch with a default set (falls back to the picker, not run), and
more than three profiles. All three tokens were revoked together because the app uses one device id for every account.

## Contrast and Settings controls (unreleased)

Throwaway Jellyfin on port 8101 (synthetic demo library), isolated XDG dirs, unique keyring service name, debug binary
started from a directory without a `.env` and with the developer variables unset; focus and pid were checked before every
keystroke. Checked live at 1280x720 (and Settings at 1900x1180): Settings closed, drop-down open (opens upward near the
page bottom, current choice ticked), scrolled; the switches (ON and OFF words), caret flip, keyboard focus ring and the
selected-row bar; Movies, an item's detail page and Admin Dashboard with the new tokens. Before and after screenshots
were compared by eye; contrast measured on real pixels (text against the local background): Settings caption 3.9 before,
8.0 after; description text 6.9; chip text 11.9; ON label 8.6 (the OFF label sat in the focus glow at 4.3 until the
focused switch got a solid fill). Unit-tested: all token and surface contrast pairs (text 4.5:1, outlines and focus
ring 3:1, secondary distinct from tertiary). Not verified live: the profile picker, dialogs and the player overlay with
the new tokens (their colours come from the same tokens and the audited list, but they were not opened), narrower
windows than 1280x720, and the drop-down at 1900x1180. Some overlay scrims and hover tints in the player still use raw
rgba values outside `theme.rs` (decorative, not text).

## Public screenshots (unreleased)

All images in `docs/screenshots/` (and the copies in `website/src/lib/screens/`) were re-captured from the release build
(`cargo build --release --locked`) at 3840x2160: a dedicated headless Hyprland output at that size with scale 2 (a 1920x1080
logical window rendered at 2x, so text and edges are native, not upscaled), the app window set to compositor fullscreen
(no title bar, panels or other windows; checked with `hyprctl clients`), window class `nocturne`, grabbed with `grim -o`.
The only library was the synthetic demo one from `scripts/make-demo-library.sh` on a throwaway Jellyfin (own port and
directories under `/tmp`, run with `NOCTURNE_JF_TEST_PORT`, `NOCTURNE_JF_TEST_DIR`, `NOCTURNE_DEMO_DIR` set explicitly),
with three generated profiles (Mara, an administrator with Continue Watching, Next Up, favourites and a collection; Theo;
Juno) that have synthetic profile pictures. The app ran with isolated XDG directories, a unique keyring service name, from a
directory without a `.env` and with the developer variables unset, so no real server was contacted. "Allow NOCTURNE to
change data on the server" was on (sync of watch progress off) so menus and the admin pages are not dimmed. The throwaway
server's published address was set to `https://media.example.com`. Two details that the server or app cannot hide were
masked in the pixels afterwards: the address line in the account menu (it shows the address the app connected to) and the
WebSocket port on the Dashboard; both were replaced with the placeholder text in the same font. The Hollow Meridian demo clip
was re-encoded at 2560x1440 (the generated clips are 1280x720) so the player frame is not upscaled soft. Files are lossy WebP
at quality 97 with sharp YUV (PSNR about 48 dB against the lossless capture on UI screens, 44 dB on the grainy video frame),
no downscale, metadata stripped (only the image chunk remains); lossless was several times larger. Checked: OCR of every
image found no address, port, path, token or name other than the three demo profiles; the Dashboard still shows the
throwaway server's random server id and the Users page a shortened user id (both meaningless). Not captured: any admin
Libraries page (it lists paths).

## Watch together (SyncPlay), Jellyfin 12.4.0 throwaway on port 8103

**Verified live** with two real app processes (users alice and bob, isolated XDG directories, unique keyring service and
`NOCTURNE_DEVICE_ID`, started without a `.env`, headless Hyprland outputs), a generated 5 minute 640x360 24 fps clip with a
burned-in timecode:
- alice creates a group, bob sees it listed and joins (the header pill shows the group and "2 watching"); alice starts the
  clip, both start together. Bob (the non-initiator) also started correctly when the group was created by alice.
- Pause by alice, seek +10 s while paused, unpause by alice; then pause, seek -10 s, unpause by bob: the other player followed
  every time. Both external mpv players were queried over their IPC sockets at the same instant: the position difference was
  0.0 ms in every sample after each transition and in 20 consecutive steady-state samples (positions are frame quantised, one
  frame at 24 fps is 41.7 ms; the largest single difference seen was one frame, 41.7 ms, right after a catch-up).
- Late join: bob left the group (local control returned: pausing bob did not pause alice), then rejoined 15 s later and caught
  up to the group's position within about 3 s, ending at a 0 to 41.7 ms difference.
- The stop key stops both players. Closing the video keeps the member in the group; "Play what the group is on" rejoins.
- Mixed backends: alice on the external mpv, bob on the embedded libmpv player. By timecode screenshots against alice's IPC
  position the difference was at most about 85 ms (one to two frames, and this includes the screenshot latency).
- Server stopped for 12 s while both were in the group: the pill read "reconnecting", then after the restart (groups do not
  survive it) the group state was dropped and local control returned.
- Clock estimate over loopback: offset about 1.4 ms, ping 2 ms. Integer ping accepted by the server.
- Server wait behaviour (ignored live test): with one member never sending Ready the group was held with `Pause` at 0 s and
  started with `Unpause` at 30.9 s.
- Drift correction never had to act in these runs (the players stayed within one frame), so SpeedToSync and SkipToSync were
  **not** exercised live.

**Unit tested only**: message decoding (real-shaped samples), clock estimator, command scheduling, duplicate handling, stale
queue rejection, late-join start position, drift thresholds and the correction timeline, Ready reports, stall detection, the
access gate (policy, switch, developer mode), item ordering, reconnect backoff, the WebSocket client against an in-process
server (decode, keep-alive answer, reconnect), REST bodies against a mock, the Home nav-highlight fix. Not verified: more than
two members, transcoded members, a user whose policy is `JoinGroups` or `None` against the real server, `LibraryAccessDenied`,
and the screen on a real library.

## Not verified / known gaps

- Real server HDR; the photo viewer and music on real data (the real server has no photo or music library).
- Cache size cap reached on a real library; multi-day soak.
- Session message action (no remote-controllable client available).
- (Fixed) the `crates/jellyfin-user` live test `playback_reporting_sets_resume_then_played` no longer assumes a 2 s clip: positions now scale with the item runtime.
- Accounts and playlists through the app UI on a throwaway: added a second account, switched to it and back without a password (the Admin entry followed the account), set and removed a profile image, set the display language, created a playlist (N), added items from an item's More menu, reordered and removed entries with the keys, renamed and deleted (typed name). Music queue: added an album's tracks from More, started the queue from the account menu, shuffle, repeat, remove; the queue survived stopping the player. The expired-token path of switching (401) is covered by a live crate test only; playback of the queued music happened in the external mpv window (embedded video was not used).
- Jellyfin lets any user see a public playlist but refuses edits (403) for non-owners; NOCTURNE uses CanDelete as the ownership test, so an administrator can edit every playlist.
- Plugin install, disable (after a restart) and uninstall, repository add / disable / remove, library rename (twice, back), provider reorder, and user access edits (feature switch, parental rating, tags, profile image upload and remove, display preferences read) were driven through the app UI against a throwaway Jellyfin 12.4. Plugin Enable, library access toggles and the other limit dialogs were checked through the crate live tests and dialog unit tests only.
- Jellyfin reports a pending plugin change as status "Restart" and keeps offering "Disable" until the server restarts; renaming a library gives it a new item id once the server has refreshed it; the profile image endpoint wants a base64 body, not raw bytes (the raw upload returns 500).
- Jellyfin lets any plugin endpoint "uninstall" a bundled plugin without error in one case we hit and refuses to disable it in another; NOCTURNE only offers these actions for removable plugins.
- Stale `nocturne-mpv-*` directories from earlier crashed runs may exist in `$XDG_RUNTIME_DIR`; the player only removes its own.

## 0.3 branch: clean-container install (2026-11-16)

Script: `scripts/test-install.sh` (Docker, throwaway containers, no sudo, no CI). Tarball from `scripts/package.sh tarball`
(`nocturne-0.2.0-x86_64-linux.tar.gz`, 11451356 B) built from the `iced-migration` head (GPUI `nocturne-app`).
Final run: `arch`, `nompv`, `fedora`, `pkgbuild` all PASS; `debian` and `ubuntu` PASS for dependency resolution
(binary does not run there, see below). Containers were `--rm`; pulled images were removed afterwards.

| Case | Result | Evidence |
|---|---|---|
| `archlinux:latest`: `NOCTURNE_YES=1 ./install.sh` as root | PASS | exit 0; mpv, libglvnd, vulkan-icd-loader, wayland, libxkbcommon, libxkbcommon-x11, libxcb, dbus all installed by pacman; `libmpv.so.2` in ldconfig |
| `ldd /usr/local/bin/nocturne` | PASS | no `not found` |
| Headless start (no `DISPLAY`/`WAYLAND_DISPLAY`) | PASS | no loader error; `panicked at crates/app/src/main.rs:69:14: open window: neither DISPLAY nor WAYLAND_DISPLAY is set`, exit 101 (no `--version` flag exists) |
| `./install.sh uninstall` | PASS | binary, desktop file and `share/licenses/nocturne` gone |
| Arch without mpv (`--no-deps`, other libs installed, mpv absent) | PASS | binary has no libmpv in `ldd` (dlopen); same windowing panic, no dynamic-loader error |
| `debian:stable`: installer dependency step | PASS | `apt-get update` + install of libmpv2, libegl1, libvulkan1, libwayland-client0, libwayland-egl1, libxkbcommon0, libxkbcommon-x11-0, libxcb1, libdbus-1-3, mpv all resolved; `dpkg -s` finds every one |
| `ubuntu:latest`: installer dependency step | PASS | same package list resolved and installed |
| Arch-built binary on Debian stable | does not run (expected) | `libm.so.6: version 'GLIBC_2.43' not found` and `'GLIBC_2.44' not found` |
| Arch-built binary on Ubuntu latest | does not run (expected) | `libm.so.6: version 'GLIBC_2.44' not found` |
| `fedora:latest`: dependency resolution | PASS | `dnf repoquery` finds mpv-libs, mpv, libglvnd-egl, vulkan-loader, libwayland-client, libwayland-egl, libxkbcommon(-x11), libxcb, dbus-libs in the default repos (no RPM Fusion needed in this image); `install.sh` exit 0 (384 packages). Binary execution on Fedora was not tested |
| `archlinux:base-devel`: `makepkg -si --noconfirm --nocheck` of `packaging/PKGBUILD` as a non-root user, source = `git archive HEAD` of this tree (temporary PKGBUILD copy, `sha256sums=SKIP`) | PASS | package built and installed; `Depends On` includes `mpv`; ldd clean; headless start reaches the windowing panic |

Findings:

- The shipped binary needs glibc 2.44 (built on a current Arch/CachyOS toolchain), so the tarball only runs on distros
  at least that new. Debian stable and Ubuntu cannot run it; building against an older glibc (or building from source on the
  target) is the only fix. The migration plan limits 0.3 to Arch, so this is recorded, not a bug.
- `install.sh` runs `pacman -S --needed` without `-y`/`-u` by design. On a stale system (the Docker `archlinux:latest`
  image has no sync database and is older than the repos), `pacman -Sy` followed by `pacman -S` is a partial upgrade
  and failed with file conflicts (openssl-3 `libcrypto.so.3` owned by `openssl`). The first harness run hit this and
  was a test artifact, so the script now runs `pacman -Syu` first, as a real up-to-date Arch machine already is. No
  change to `packaging/install.sh` was needed: every package name was proven right on Arch, Debian, Ubuntu and Fedora.
- Not tested: the PKGBUILD `check()` step (skipped with `--nocheck` to save time), a real display or GPU in the
  container (so only the "reaches windowing" stage), openSUSE, running the tarball binary on Fedora.
