# Changelog

All notable changes are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- Settings is a two-pane page: a sidebar of sections (General, Appearance & Themes, Home, Playback, Sources & Servers, Sync & Trakt, Requests, Gateway, Privacy & Data, Updates, About) with icons, and the selected section with its rows in rounded cards. A search field at the top of the sidebar (or `/`) filters rows across all sections. Tab and Shift+Tab move between sections; the arrow keys walk every row and cross into the next section. Narrow windows show the section list and drill into one section.
- One elevation scale: raised cards and overlays (dialogs, menus, toasts, tooltips, the search panel) get soft shadows over the existing hairline outlines.
- Empty Requests, Discover, Calendar and My themes views show an icon and a sentence instead of a bare line of text.

### Fixed
- The header is opaque, so page text no longer shows through it while scrolling (Settings content now also starts below the header).

### Added
- `nocturne-server`: a headless server with independently enabled modules behind one REST + WebSocket API, first-run admin setup, session tokens and API keys, a local-admin unix socket, and a CLI (`install`, `status`, `config`, `apikey`, ...).
- Library manager in the server: monitored movies, shows, music and books, quality profiles, user-added Torznab / Newznab indexers, automatic and interactive search, a download queue on the built-in torrent engine, import into your library folders under naming templates, failed-download handling, RSS, missing and upgrade searches, and native requests.

## [0.7.0] - 2026-11-27

NOCTURNE can now serve all your servers and local folders to Jellyfin apps on your phone or TV as one server, and can send requests to Radarr and Sonarr without a Seerr server.

### Highlights
- The gateway: one Jellyfin-compatible address over every server and local folder. Jellyfin apps browse one merged library, play from whichever server has the copy they pick, and (when you allow it) see and control the players on all your servers. It runs on this computer only by default, keeps every server token to itself, and can keep running after NOCTURNE is closed.
- Requests in the header's top right, with a Discover view of trending, popular and upcoming movies and shows, search and filters.
- Requests without a Seerr server: a list kept on your computer that turns Available when a source has the title, optionally sent to your own Radarr and Sonarr after a confirmation.

### Fixed
- Themes that shrink Search to an icon no longer show the Requests button as a second search icon.

### Known limitations
- The gateway is Linux only for now (not in the Windows and macOS previews), and the official phone and TV apps have not been tried by the project yet; report what your app does.
- A change made directly on a server reaches apps connected to the gateway after a short delay, not at once.
- The gateway does not transcode local files (direct play only) and has one sign-in for all devices.

### Added
- Gateway foundation (0.7): the new optional `nocturne-gateway` binary (axum, off unless started) with the allow-list router, gateway sign-in (one user, Argon2id passphrase, revocable device tokens, Quick Connect), the id map, binding modes with bring-your-own TLS, a control socket for the app, and UDP discovery in local-network mode. Library, streaming and remote-control routes follow. The Linux packages ship the second binary.
- Gateway remote control (0.7): a merged session list across your servers and the apps signed in to the gateway, and commands (play, pause, seek, next, volume, tracks, messages) delivered to those apps over the gateway's own WebSocket or forwarded to the device's own server. Off by default behind Settings > Gateway > Allow remote control; commands to a device on a real server also need server changes on, only a fixed list of player commands is accepted, at most 20 per second per device. The `/socket` WebSocket pushes session changes only while a client listens and coalesces library and watched-state news.
- Settings > Gateway: switch the gateway on and off (NOCTURNE starts and stops the helper), choose who may connect, port, caching, remote control, per-server views and local-network discovery; see status, the address with Copy and a QR code, the generated password (shown once, regenerate any time), approve Quick Connect codes and revoke signed-in devices. "Keep running when NOCTURNE is closed" installs a systemd user service on Linux. A server whose ServerId is the gateway's own cannot be added as a source.
- In-app updates also install `nocturne-gateway` next to `nocturne` when the release archive contains it.
- Requests entry in the header's top right, next to Search, shown when a Seerr server is signed in or requests without Seerr are on. It replaces the Requests link in the link row.
- Discover on the Requests page: browse trending, popular and upcoming movies and shows, search, and filter by movies or shows, genre and year. Cards show the poster, year, rating and status (Available on your servers, Pending, Requested, Queued, Downloading, Not requested), a title any source has opens its page, and Request uses the confirmation dialog with the season choice. Titles come from your Seerr server, else your own TMDB key, else Trakt; Radarr or Sonarr alone can search. With none connected it says what to connect.
- Requests without a Seerr server (Settings > Requests, off by default): a list kept on this computer (`wishlist.json`). A request turns Available by itself when the title or the requested seasons appear on any source, with a notice. Nothing leaves the computer.
- Optional Radarr and Sonarr (Settings > Requests): connect your own with an address and API key (the key stays in the keyring), test the connection and pick the quality profile and root folder from what the service lists. With "Add requests to Radarr and Sonarr" on, a request made without Seerr is also added there, after the confirmation dialog (movies to Radarr, shows with the picked seasons to Sonarr), and its status follows the service. New crate `nocturne-arr`, `scripts/test-arr.sh` for throwaway services.
- `scripts/mock-requests.py`: mock TMDB, Seerr, Radarr and Sonarr with invented titles for headless screenshots of the Requests page.
- Gateway library and playback (0.7): views, items (modern and legacy forms), Home rows, search, seasons and episodes, filter lists and images, merged across servers by provider id with one media source per server, plus local folders as direct-play items. Streams are proxied with Range, HLS playlists are rewritten so no server token, address or file path reaches a client, and watched and resume state is kept in a gateway overlay and forwarded to the item's own server only with the write switches on. Paths are matched case-insensitively. The test script gains a second instance (`test-jellyfin.sh start 2`).
- Gateway hardening (0.7): a session's now-playing item shows the real title, type, runtime and artwork tags (never a file path), the WebSocket pushes watched, favourite and resume changes and library news after a reload, and a security review with new tests (headers never forwarded to servers, redirects never followed, every write refused with server changes off, WebSocket and login limits, loopback and LAN binding, local files confined to their folders). jellyfin-mpv-shim was driven end to end through the gateway.

## [0.6.0] - 2026-11-21

NOCTURNE can now play in VLC or an mpv window, shows what is airing next in a Calendar, sends requests to a Seerr server, and installs themes from a gallery.

### Highlights
- Choose where video plays: the in-window player, VLC or an mpv window, each offered only when installed. Resume, progress sync, Trakt and Stop keep working in all three.
- Calendar: month, week and agenda views of upcoming episodes from every server and your Trakt calendar, marked On your server, Airing soon or Not on your servers, with an optional "Coming this week" row on Home.
- Seerr requests: ask for movies and shows that no source has, from Search, item pages and the Calendar, and follow them on a Requests page. Off by default, confirmed every time, and nothing is written to a media server.
- Themes gallery: Settings > Themes opens on a gallery of official and community themes with previews and one-click install, checked against a checksum. Two new official themes, Daylight (light) and High contrast.

### Known limitations
- MPC-HC on Windows and IINA on macOS are not offered yet. In VLC, subtitles and audio tracks are chosen in VLC's own menus.
- One Seerr server; 4K requests and Seerr's server, profile and folder choices are not offered.
- Flatpak was evaluated ([docs/dev/FLATPAK.md](docs/dev/FLATPAK.md)) but is not shipped.
- Windows and macOS remain unsigned previews; macOS plays video in mpv's own window.

### Added
- Choose where video plays: Settings > Video player > Player offers In-window (the default), VLC and an mpv window, each only when installed. VLC is driven over a loopback-only control interface protected by a random password, so your place, progress sync, Trakt scrobbling and Stop work as with the other players; subtitle and audio menus are not available in VLC (use VLC's own). A stream that needs extra request headers plays in-window. `NOCTURNE_VIDEO` still overrides the setting, and a player that is no longer installed falls back to the in-window one with a one-line notice.
- Request movies and shows from a Seerr server (also Jellyseerr or Overseerr): Settings > Requests > Seerr server signs you in with your own Jellyfin account (Quick Connect, or username and password; only the session is kept, in the system keyring). With Allow requests on (off by default), Search shows a Request group for titles that are not in your library, with a dialog that lets you pick seasons for a show. A Requests page in the header lists your requests with their status and cancels your own pending ones after a confirmation, and a movie or show page shows its Seerr status. A request goes to the Seerr server only, never to a media server, and needs no server-changes switch. Tested against a throwaway Seerr only (`scripts/test-seerr.sh`).
- Calendar: a header link (or `c` on Home and library pages) opens month, week and agenda views of upcoming episodes. Each server's upcoming list is merged across servers, and with Trakt connected your Trakt calendar is added (read only). Entries show the show's poster, "S1·E5 Title" and the air time in your time zone and 12 or 24 hour style, marked "On your server" (play it now), "Airing soon" (a server has the show) or "Not on your servers" (Trakt only). Enter or a click plays an episode a server has or opens the show; arrows, Tab, PgUp, PgDn, `m` `w` `l` and `t` drive it from the keyboard. A server's own layout uses a plain list version of the page. The same episode on several servers is one entry (shared series id plus season and episode number).
- Calendar: with Seerr requests on, a show in "Not on your servers" (a Trakt entry with a TMDB id) gets a Request button in the week and agenda views and the `r` key in every view, which opens the Request dialog; a show Seerr already knows wears its status (Requested, Approved, Available) instead.
- Settings > Calendar > "Coming this week on Home" (off by default): a Home row of the episodes your servers list for the next seven days.
- `scripts/make-demo-library.sh build --upcoming` adds a demo show whose episodes air in the next weeks, for Calendar screenshots.
- Themes gallery: Settings > Themes has tabs Official, Community and My themes. Official lists NOCTURNE's own themes (Marquee, Feed, Classic, Spotlight, Glass, and the new Daylight light theme and High contrast) with a preview, author, licence and description, and installs one with a click; Update replaces an installed copy when the gallery has a newer one. Downloads come from the NOCTURNE website over https, are checked against the gallery's sha256, respect the "Themes may load from the web" switch, and the index is cached with a Refresh button and an offline notice. An installed theme is a normal saved theme; themes you add by hand stay and are marked "not from the gallery". Anyone can propose a community theme by pull request: see `themes/README.md`.
- Settings > Themes is two views under the header, Gallery and Editor (Gallery first; the last choice is kept for the session). Gallery is full width: the Official, Community and My themes tabs, a search over name, description and author, and a grid of large cards (1 per row at 900 px, 2 at 1280, 4 at 1920) with a 16:9 preview, status (Installed, Update, Applied), Install or Update, Apply, and Edit, which opens the Editor on that theme. The built-in NOCTURNE look is a card in Official too. Editor keeps the theme lists, the CSS editor and the report.
- `scripts/themes-index.py` checks the `themes/` folders and writes the gallery index and files into `website/static/themes/` (`--check` for CI).
- `docs/dev/FLATPAK.md`: evaluation of a Flatpak (manifest sketch, permissions, libmpv, keyring, size, Flathub steps), no packaging shipped.

## [0.5.2] - 2026-11-20

NOCTURNE now draws its main screens with its own CSS theme engine, can show a Jellyfin server in that server's own look, and lets you build themes inside the app.

### Highlights
- A native theme engine (no browser) draws Home, the header, the library, search, the item page and the player's controls, with real flex, grid and positioned layout. With no theme the look is unchanged.
- Your server's own look: a server with a custom theme is shown with its web client's page structure and stylesheets, including the stylesheets its plugins add and the fonts and pictures they load. Scripts never run. Any source can borrow another server's look.
- Theme Studio: point at any element of the live screens and change it with controls; it writes real CSS you can keep editing by hand.
- Two built-in themes, NOCTURNE and Glass. Paste or import any Jellyfin web theme and preview it before keeping it.
- Update now for pacman, apt and dnf installs, through PackageKit and the desktop's own password prompt.
- Windows preview: video plays inside the app window, text renders with bundled fonts, and starting playback no longer freezes the app.
- Smoother and lighter: an idle window no longer redraws, large libraries use far less memory, and a stuck video engine can no longer freeze the app.

### Known limitations
- Theme effects without a native equivalent yet: backdrop blur becomes a translucent fill; masks, conic gradients and `filter: blur()`, `saturate()` and `contrast()` are not drawn; what a theme does with JavaScript does not happen. Settings > Themes lists everything that did not apply.
- Fonts declared inside a server plugin's stylesheets are not loaded yet.
- Windows and macOS remain unsigned previews; macOS plays video in mpv's own window.
- `NOCTURNE_ENGINE_UI=0` brings back the hand-written screens for this release if a theme-engine screen misbehaves.

### Added
- Glass: refined to match its reference design. The home hero is 85% of the window, the header gets link icons and a back button on item pages (no breadcrumb strip), the library page has a count chip and a vertical A to Z strip, episode cards carry glass buttons over the artwork, and Search is a centred field over rows of poster cards. Themes can set the side gutter (`--page-pad`), a back button in the header (`--header-back`) and a hero height in `vh`; the header scene over item pages carries a `page-header` class so a theme can leave it clear.
- Settings > Themes: a Copy button on the report panel copies the open tab (This theme or Server branding) as plain text, for sharing in a bug report.
- Jellyfin layout mode reads the stylesheets a server's start page adds (plugins such as SleekFin), as a browser would and never running scripts: `/web/index.html` is read, its `<link rel="stylesheet">` (not `alternate`, `disabled` or `media="print"`; other `media` kept as `@media`) and `<style>` blocks are applied in document order after the web client's own sheets and before the branding CSS and your theme. Only the server's own files are fetched, plus other https addresses under the remote-styles rules (public hosts, size and count caps); at most 60 sheets and 4 MB. `@import` inside them is followed, results are cached per start-page text, and Themes > Refresh imports reads them again. Settings > Themes > Server branding lists per server how many stylesheets, how many rules the engine read, what was skipped and how many scripts were not run.
- Themes page: the long explanation is behind "How themes work", the lists have the left column to themselves, and the report (This theme / Server branding, grouped with one item per line) sits full width under the editor, which grows with the window; short or narrow windows scroll the page.
- Theme engine: `radial-gradient()`, `repeating-linear-gradient()` and `repeating-radial-gradient()` are drawn (Iced only draws linear ones): each is rasterised once per box size into a cached picture layer in the right stacking order, never per frame (conic gradients are still skipped). `@keyframes` with `animation` (and its longhands) animate opacity, transform and colour; frames are requested only while something runs, endless animations at 30 per second, none with reduced motion, and never on an essential control. `filter: brightness()` dims an element (a theme's backdrop dimming), and `initial`/`unset` put the default back (ElegantFin's `margin-top: unset` had left the item ribbon about 100 px too high).
- Jellyfin layout mode: Home and library pages have the page-sized `.backgroundContainer` themes tint (Jellyfish's teal), Continue Watching and Next Up carry the attributes themes use to lay them out (Zombie's big card with small ones), and the item page keeps a hidden previous page as the web client does.
- Theme engine: gradient colour hints (`linear-gradient(180deg, transparent, 17vh, #111 50vh)`) and `vw`/`vh` stop positions, `[class*="..."]` selectors, `content: attr(title)`, and text beside `::before`/`::after` bullets (section titles of Zombie and Nebula no longer clip). The item page's body no longer carries `withSectionTabs`, so a theme's dim layer over the backdrop (ElegantFin) is the translucent gradient instead of a solid one, and the clear logo always fills its own box.
- Jellyfin layout mode draws the pictures a stylesheet names with `url()`: background images with `background-size`, `-position` and `-repeat` (gradients written above a picture paint over it), `content: url()` on the header logo, themed tile pictures and inline SVG; the server's own address may be http or on the LAN, any other host needs https, a public address and remote styles on. Unsupported gradients (conic) are skipped instead of dropping the whole declaration.
- Jellyfin layout Home: the Collections and Playlists libraries are My Media tiles in the server's order, and tiles without a picture get the web client's colour class, icon and a theme's own tile picture.
- Jellyfin layout mode: when a source uses a server's UI and the server has its web client, Home, the header, the library and the item page are built with jellyfin-web's own page structure and styled with the server's own stylesheets (fetched from the server, cached, never bundled), then its branding CSS and your theme, so Jellyfin web themes look as they do in the browser. The engine learned `rem` from the root font, floats, absolute boxes against positioned ancestors, `inherit` and more.
- Jellyfin layout mode draws the web client's icons: the server's own Material Icons font is fetched from the server at run time (GET, size-capped, cached next to its stylesheets, never bundled), registered with the text shaper, and the item page's buttons, rating star and selector arrows use jellyfin-web's own `span.material-icons.<name>` elements, so a theme restyles them as on the web. Ligature text (`play_arrow` in a Material Icons span) shapes to the glyph; without the font the page shows plain glyphs instead of boxes.
- Jellyfin layout mode item page: the Video, Audio and Subtitles lines of the current file (jellyfin-web's `form.trackSelections`, read-only), "Ends at", Resume with its "Play from the start" button, Shuffle and Watched on shows, and the backdrop dimmed by the page-sized `.backgroundContainer.withBackdrop` layer the theme sets the opacity of (as the web client does from script) instead of a fixed opacity.
- Theme Studio (Themes > Open Studio): pick any element of the live Home, header, library and search screens and change it with colour, size, spacing, shape, shadow, accent bar, typography, layout and hover controls ("this element only" or "all like this"), with undo and redo, a CSS tab that stays in sync both ways, starters, save, apply globally or per server, and export as `.css`.
- Update now for pacman, apt and dnf installs: when PackageKit is available the update banner can refresh and update only the `nocturne` package through the desktop's normal authentication prompt, then offer Restart; the command stays visible if it is refused or fails.
- Themes can rebuild the familiar server web home layout: new parts for the unplayed-count badge, progress bar, episode line, caption alignment, row arrows, header server name, link icons and Watch together button, and library tiles, plus a built-in "Classic" example theme.
- Themes follow `@import` and load web fonts: stylesheets that import other stylesheets (Jellyfin web themes, font sheets) now apply, and `@font-face` fonts in TTF, OTF, WOFF and WOFF2 are downloaded and used. https only, no cookies or sign-in, size, time and count limits, kept in the cache folder and refreshed only by hand or when the text changes. Settings > Themes has a switch that turns all downloading off.
- One server's UI over another: each source, "All" and local folders can use NOCTURNE's layout, their own server's UI or another server's (its home sections and look, filled with what you are browsing). Local only; nothing is written to either server. A signed-out or unreachable UI server falls back to the NOCTURNE layout with a note.
- Settings > Sources has a "UI:" button on every source and on a new "All sources" row to choose whose UI is used (NOCTURNE, this server, another server); a fallback is explained on the row. Settings > Themes lists the built-in themes first, has "Refresh imports", shows what was fetched and which fonts loaded, and previews a pasted or imported theme on the whole app until Keep or Revert (a floating bar on every screen).
- Safe look: Ctrl+Shift+0 or starting with `NOCTURNE_SAFE_THEME=1` restores the built-in look at once, whatever theme is saved.
- Data for the Glass look (UI follows): title logo art for server items (own or the show's) and local `logo.png` / `clearlogo.png` sidecars (next to the movie or in the show folder, read only); Settings > Home banner (rotate by itself, interval 5 to 30 s); the banner's slide selection and timer (several sources in order, no duplicates, pauses while hovered, focused, expanded, hidden or playing, none with reduced motion); expandable descriptions; loading states for skeleton placeholders.
- Glass, a second built-in theme next to NOCTURNE (also a Theme Studio starter): black canvas, a glass header pill with the server name at the left, a full-bleed hero with the title as logo art, light and glass pill buttons, accent rails before section titles and squircle cards with their titles below. Backdrop blur falls back to the translucent fill.
- The Home hero is a carousel (theme engine): slides from continue watching, next up and the newest movies and shows rotate on a timer (Settings: auto-rotate and interval) that pauses while the window is inactive, a button is held, a description is open, the focus is in use, a dialog or the player is open; Left and Right at the hero's buttons step slides, themes can show arrows, dots and a progress hairline, and the leaving slide fades out. The hero description clamps to three lines and opens on a click.
- Themes can style a skeleton for the loading hero and rows (`.hero-skeleton`, `[data-region="loading"]`) and a scrim behind the header once the page has scrolled (`page[data-scrolled="true"]`).
- Theme Studio works on the detail page (movie, series and episode) and on the player's controls bar too: the same pick, inspect and live edit, and the panel docks beside the player so the video keeps the rest of the window. While picking, hosted widgets (the header's Smart menu, search, avatar and history, the player's transport buttons, seek and volume tracks) select their element instead of acting; leaving pick mode restores them.

### Changed
- The theme engine is on by default: Home, the header, Library, Search, the item page and the player's controls are element trees styled by the stylesheet (the look is unchanged with no theme). It is the `theme-engine` cargo feature, now a default feature of `nocturne-app`; `cargo build --no-default-features` builds the hand-written screens only, and `NOCTURNE_ENGINE_UI=0` shows them at run time for this release.
- Loaded theme fonts apply at once to headings, labels and buttons (text without an explicit font keeps the launch font).
- `sources.toml` is version 2: the per-server Server UI switch is now `ui_from = "self"` (migrated automatically).

### Fixed
- Player: the next-episode card says "Next episode in 9 s" (it had no unit), the end time follows the title and status ("clip  Ends at 7:37 PM"), and the buffered range of the seek bar is a thinner, lighter line so it no longer reads as playback progress next to the elapsed time.
- One clock formatter: the player's "Ends at", the item page's "Ends at" and the episode cards' "ends" all follow the system locale's 12 or 24 hour style (read from `nl_langinfo`; other platforms keep 12 hour), where the player showed 24 hour and the pages 12 hour.
- Player menus: the audio menu marks the playing track (a lone track was never marked), and the Settings button, which also holds Quality, shows a gear instead of a sun that looked like brightness.
- Home's hero eyebrow reads "01 / 05 · Movie" (the slide among the carousel's, or just the kind) instead of the "0x00 · movie" index.
- Season tabs read "Season 1 · 4 episodes" (the real episode count, asked for with `ChildCount`) instead of a bare number after the name; without a count they say "· N unwatched".
- Settings rows keep a readable width (1280 px) so a label and its control are no longer a screen apart; the account menu's greyed items say "Needs server changes on" under the label, and its header shows the source's name instead of its address.
- The header lights no link on Settings, Sources or Themes (it kept Home or Movies lit).
- Jellyfin layout mode: a stylesheet's `background-size` in `em` is honoured (`auto 1.2em`), which draws the critic rating's tomato at icon size instead of a stretched red block; "Sort: Name ↑" stays on one line; the library tile's icon is the web client's 5 em.
- Clicking a My Media tile in a server's own look no longer crashes: the paint widget kept hover, press and drag node numbers across a page swap and indexed the smaller new page with them; they are now dropped when the page is replaced.
- Safe look (Ctrl+Shift+0) is the built-in NOCTURNE look on every page, including pages in a server's own look (the server's layout is dropped while it is on), instead of changing only the header and leaving a broken library behind.
- The Admin link in the header opens Admin from Settings, Themes and Sources (they were drawn over it), and those pages no longer show a leftover browsing breadcrumb.
- The header's brand names the server whose look is shown (else the one picked in the source filter), not the last server signed in.
- The breadcrumb strip is opaque, so buttons and text scrolling under it on a detail page no longer show through.
- The breadcrumb shows the logical path: a detail page opened from search over another detail page no longer names that page as its parent (a show above its episode still does).
- The item page shows each fact once: the overview only in the hero, the details table without rating and genres (and in the left column, not far right), no hero copy of year, runtime and genres in the default look, and no stray section counts. The NOCTURNE look's detail layout changed on purpose; Glass needs `.detail-year, .detail-runtime, .detail-genres { display: flex }` to keep its meta line (Spotlight has it).
- Server look, My Media: each tile names its server when several are shown, and the row has the strip scrollbar and arrows.
- A settings file that cannot be read (a duplicate key from a hand edit) is no longer silently reset: a copy is kept as `settings.toml.bad-<time>`, repeated keys are repaired (the last value wins), and a notice names the copy.
- A server whose branding CSS is non-empty now uses its own look (Jellyfin layout mode) by default and servers without branding keep NOCTURNE's; the choice in Settings > Sources > UI is stored even when it is NOCTURNE, so only "never chosen" gets the default, and the row says so ("default: this server's look, it has a custom theme"). "All sources" that was never chosen follows the look chosen for the first server, which the All row explains.
- "UI: <other server>" now shows in the server's own view and in "All sources": the other server's base stylesheet and branding apply to the items of the source it was chosen for.
- Settings > Themes > Server branding reports what the theme engine read and skipped for each server (the same grouping as a pasted engine sheet) instead of the old translator's "not understood" counts and its `@media` warning.
- Google Fonts: duplicate `@font-face` sources are downloaded once, the Latin and Latin Extended subsets load first, other-script subsets are skipped with one note per family, the limit is 24 distinct files (it was 12 files counted per weight), and a family no longer fails because of the limit.
- Jellyfin layout mode: the mouse wheel, keys and the scrollbar now work on Home (the page was as high as the window, so nothing could scroll).
- Theme engine: `em` lengths (`width`, `height`, `margin`, `gap`, `border-radius`, ...) are measured against the element's own final font size whatever order the stylesheet declares them in. jellyfin-web sets `.indicator { height: 2em; width: 2em }` before `.countIndicator { font-size: 88% }`, and `.detailButton-icon { font-size: 1.6em !important; width: 1em }`, so the badge circle and the icon boxes had the wrong size (and an `em` radius was always 16 px based); an icon next to a label could overlap it.
- Theme engine: text directly inside a flex or grid container is placed by `justify-content` and `align-items` like the browser's anonymous item, so the unplayed-count number sits in the middle of its round badge instead of in its top-left corner.
- Jellyfin layout: a widget hosted in a larger box (the 32 px user picture in the header's 40 px round avatar) is centred in it, so the blue ring is even around the picture instead of showing as a crescent.
- The film-grain overlay is gone: its 512 px speckle tile was stretched to the window, so on large screens it showed as a faint block pattern over pages, buttons and video.
- Playback no longer freezes the app when the video engine gets stuck: with no usable audio server, closing a video right after opening it could leave mpv's core thread blocked inside PipeWire's shutdown, and every synchronous libmpv call from the window then waited for it, so the page sat on "Opening", the window stopped responding and SIGTERM was ignored. Calls to the embedded player now run on a worker with a short wait, teardown gives up on a stuck thread after about a second and the process then ends without it, an item that has not opened after 30 seconds fails with "Couldn't start playback: ...", and a stuck engine is reported after 15 seconds.
- Memory: browsing a large library with artwork no longer grows the process by hundreds of MiB. glibc kept the freed picture-decode buffers of every worker thread (one arena each, and a rising mmap threshold): the allocator is now set to one arena and fixed 1 MiB thresholds at start, which also takes about 70 MiB off the idle baseline in a software-GL session; and the engine's library grid lets go of the pictures of cards far from the rows in view (they are fetched again from the disk cache on return). Numbers in `docs/dev/PERF.md`.
- An idle window no longer redraws: the app drew about 6 times a second on Home and 10 times a second in the Library with nothing happening, because a 200 ms polling tick ran all the time and every Iced message redraws the window. The tick now runs only while something needs it (a recent action, playback, a toast, a download, an offline server, onboarding), terminate signals and a second launch reach the app without it, and the Home hero wakes the app only when its slide changes (once a second only if the theme draws the progress hairline). Idle Home with the hero off, Library, Search, the item page and Settings draw 0 frames in 10 s.
- Engine: boxes that only touch (fractional layout) were treated as overlapping and one of them was drawn beneath the page, so a card could vanish; the harness loads Catppuccin with its flavour palette.
- Engine screens (`theme-engine`): the mouse wheel scrolls the library grid and the detail page again (a header strip above them swallowed it), and the page scrollbar can be dragged.
- Engine Home: the row paging arrows of the Classic look scroll their strip; hovering a header link no longer leaves it lit after the pointer leaves; the Smart link no longer lights on every library page; in a window narrower than 640 px the brand makes room for the links.
- Engine search overlay: the Up and Down keys move the highlighted result.
- Theme Studio: the Home hero stops rotating while the Studio is open, and the safe look chord also resets the open Studio preview.
- The built-in Marquee and Classic starters no longer make the theme engine warn about `display: carousel` and `content: server`; they are accepted as the layout layer's words.
- Home rows now ask the server for each item's overview and genres, so the hero shows the description; long wrapped text in the theme engine is no longer dropped when the renderer breaks one line more than the measure did.
- The hero's Play label keeps a contrasting colour under a theme that colours buttons.
- Pasting or importing a theme that is (or contains) an `@import` now fetches it for the preview too, shows "Fetching imports and fonts..." while it downloads, and feeds the fetched, inlined stylesheet (and its fonts) to the theme engine for previews, the Theme Studio, saved themes and server UIs instead of the raw text.
- Windows: text no longer shows as random symbols. The UI relied on fonts the PC does not have (Noto Sans, JetBrains Mono Nerd Font); Noto Sans (Regular, Medium, Bold) and JetBrains Mono (Regular, Bold) are now embedded in the app on every platform (SIL OFL-1.1, see THIRD_PARTY_NOTICES.md).
- Windows: the app no longer freezes ("Not Responding") when playback starts, and the stats overlay shows values again. Starting mpv, connecting to its pipe and every property read waited on the UI thread (up to 300 ms each, several per tick, longer than mpv needs to open its window on a slow PC or virtual machine); all of it now runs on worker threads and properties come from a cache kept current by mpv's property observers. The same change applies to the external mpv window on Linux and macOS.
- Windows: video plays inside the app window (a child window that mpv draws into) instead of a separate window. The controls bar and menus sit outside the picture (the window cannot be drawn over): the picture shrinks above the bar while it shows, stays visible around menus and panels (the window is cut out where they are drawn) and hides only under full-window overlays and prompts. If mpv cannot start in the window, the separate mpv window opens with a one-line notice; `NOCTURNE_VIDEO=external` forces it. Needs testing on a real PC.
- Signing in to a server whose old token was still in the keyring no longer opens with "Your session ended. Please sign in again."; the note only shows when the session restored at launch was rejected or the session ended while the app was open.

### Development
- The engine's time budget is 300 ms per pass (it was 50 ms and switched heavy themes off in debug builds); `NOCTURNE_ENGINE_TIMING=1` logs every pass, and `cargo build --profile conformance` builds optimised code with the debug-only snapshot hooks the conformance harness needs.
- Debug builds can write what the active theme did (tokens applied, not applied by kind, imports fetched, fonts loaded) as JSON with `NOCTURNE_THEME_REPORT=<file>` and the `theme-report@<ms>` key hook; `scripts/theme-conformance.sh` fills its applied / not applied column from it.
- `tools/headless-pointer` gains `wheel <dx> <dy>` and `drag <x1> <y1> <x2> <y2>` for headless QA.
- `scripts/headless-parity.sh` captured the thumb view at a random moment (the `v` key raced the page load); the debug hook `view-thumb` waits for the library page and sets the view explicitly, and `PARITY_ONLY=<text>` runs a subset.
- The app's test run no longer crashes now and then with SIGSEGV: tests that build an `iced_test` simulator ran concurrently and now take a shared lock.
- `scripts/headless-input-check.sh` no longer loses chords: it waits for the virtual keyboard to appear, refocuses, checks the focus and only then sends the keys; each check starts from a known state and is repeated once if a chord went missing.
- `scripts/theme-conformance.sh` runs each theme with the import its own project documents (ElegantFin no longer resolves to a Jellyfin 12 fork) and makes "All" use the server's UI, so the NOCTURNE side shows the server's sections.

## [0.5.1] - 2026-11-19

### Added
- macOS packaging also produces `nocturne-<version>-aarch64-macos.tar.gz` (the `NOCTURNE.app` bundle) with its sha256, the artifact the in-app updater installs; until now macOS only had the `.dmg` and showed Download.

### Fixed
- Themes: importing a CSS file or address while a saved theme is selected no longer overwrites that theme; it fills the editor as a new theme named after the file, and Save adds it.
- `scripts/headless-input-check.sh` is steadier: the paste check reads the field through the clipboard instead of a fixed screen crop, and chords that go missing are retried.

## [0.5.0] - 2026-11-19

### Added
- Updates: NOCTURNE checks once a day (Settings > Updates turns it off). Per-user installs offer Download and install, then Restart to update; every download is checked against the announced size, sha256 and an ed25519 signature before anything is replaced, and a failed update leaves the current version untouched. Package-managed installs show the pacman, apt or dnf command instead.
- Themes can restyle layout with CSS on named parts, and the app follows it: cards (aspect, width, caption position, radius, shadow, typography, hover zoom), Home rows (carousel or grid, gap, padding, section order and hiding), hero (billboard, banner or none, height, overlay), header (height, hidden items, static or overlaid, link typography), button hover scale and the player bar background. Two built-in examples, Marquee and Feed, can be copied from Settings > Themes. `--nocturne-density`, `font-size` (whole-interface zoom) and `backdrop-blur` (a soft placeholder on the detail page) are applied approximately; `letter-spacing` is read but not drawn.
- Themes: Settings > Themes opens a page to import a CSS theme (paste, a `.css` file, or one https address you type, fetched once and refreshed only when you ask) and apply it to the whole app, optionally with a different theme per server. NOCTURNE reads only what it can map to its own look (colours, corner radii, window background, a font family, backdrop opacity) and lists everything else with its line and column; nothing is executed or downloaded from inside the stylesheet. A preview lasts 15 seconds unless you keep it ([CUSTOMIZATION_PLAN.md](docs/dev/CUSTOMIZATION_PLAN.md)).
- Trakt connects with one click: NOCTURNE ships a public client id (PKCE, no secret), so Settings > Trakt shows just "Connect Trakt". "Advanced: use your own Client ID" (or `NOCTURNE_TRAKT_CLIENT_ID`) still overrides it, and a build without a built-in id asks for one as before ([TRAKT.md](docs/TRAKT.md)).
- Pushing a `vX.Y.Z` tag builds every format on GitHub-hosted runners (tarball, .deb, .rpm, Windows installer and zip, macOS .dmg, best-effort AppImage) and creates a draft release with `SHA256SUMS` and the CHANGELOG section as notes; the repositories are still signed and published locally (`.github/workflows/release.yml`, [PACKAGING.md](docs/PACKAGING.md)).
- apt and dnf repositories next to the pacman one (`https://nocturne.ifsvivek.in/repo/deb` and `/repo/rpm`, signed with the same key): Debian/Ubuntu and Fedora users get NOCTURNE updates through `apt upgrade` / `dnf upgrade` and their software centres. Built by `scripts/publish-repo-deb-rpm.sh` (called by `scripts/publish-repo.sh`); the key is also published as `repo/NOCTURNE-GPG-KEY` (armored) and `repo/NOCTURNE-GPG-KEY.gpg` (binary).
- Server UI (per server, Settings > Sources > Layout, off for new servers): with one server selected in the source filter, Home follows that server's own layout: its home sections in its order, a My media row of its libraries in your order without the ones you hid on the server, and Latest rows without the excluded libraries. The server's branding CSS becomes that server's theme (below a theme you pick for it, above the global one; listed in Settings > Themes with what could not be mapped). The sign-in page shows the server's login disclaimer. Live TV rows and the audio and book continue rows are skipped. The unified Home (filter All) is unchanged ([SERVER_UI_PLAN.md](docs/dev/SERVER_UI_PLAN.md)).
- Settings > Sources > Sections edits the server's home sections (reorder, on / off) for your account. It is a server change: only with "allow server changes" on, never with a developer token, and it replaces only the home section entries of your display preferences on that server, leaving every other setting as it was ([WRITES.md](docs/WRITES.md)).

### Fixed
- Local library: after a scan was interrupted before saving its index, a newly found file could take the id of an existing item; ids are now recovered from the indexed items too.
- The connect and sign-in forms put the cursor in the first field when they open, so you can type straight away and Tab cycles the fields and buttons.
- Every text field can now be edited like a normal one: select with the mouse or Shift+arrows, double-click a word, Ctrl+A, Ctrl+C / Ctrl+X / Ctrl+V. The sign-in, add-server, account dialog, admin dialog and SyncPlay name fields used a hand-rolled input that could only append and delete characters. Password fields cannot be copied or cut.

### Changed
- Packaging: the deb, rpm, Arch package and `install.sh` no longer depend on X11 libraries (`libX11`, `libxcb`, `libXcursor`, `libXi`, `libxkbcommon-x11`) now that Linux is Wayland-only.
- **Linux is Wayland-only.** X11 support is removed: Iced is built without its X11 backend (no X11 libraries are loaded) and on a session with only `DISPLAY` set NOCTURNE prints that X11 is not supported and exits. The external mpv player remains the fallback when in-window video cannot start and for the HDR pop-out.

## [0.4.0] - 2026-11-18

What is verified and what is not is listed per feature in [FEATURES.md](docs/FEATURES.md).

### Added
**Playback**
- Secondary subtitle (Subtitles menu, "Secondary" section; Shift+C cycles it; top by default).
- Audio and subtitle language remembered per show (and per movie) across restarts, in a local file under `$XDG_DATA_HOME/nocturne/`, never on a server; "Original audio" is a choice in the Audio menu.
- HDR video: for an HDR stream the player asks whether to play in the window in SDR or open an HDR pop-out window (mpv's own window, driven from the app). New setting "HDR video" (Ask / Play in the window in SDR / Open in HDR pop-out window) ([HDR.md](docs/HDR.md)). Not yet seen on an HDR display.
- Player menus by keyboard: T Subtitles, V Audio, O (or Menu) Settings, Q Quality; Shift+Left / Right seeks 30 s.
- Local files play the subtitles you downloaded; the Subtitles menu offers "Find subtitles online" (when the source's subtitle download is on) and a download is added to the playing video at once.

**Library and sources**
- Home "Latest" rows use the server's `Items/Latest`, so new episodes of shows already in the library surface (grouped under the show); Play on a show card starts its next unwatched episode (`Shows/NextUp`).
- "Included in" strip on movie and show pages (collections the item belongs to, Jellyfin 12+).
- Optional extra HTTP header per server (Settings > Sources > Header) for servers behind a gateway. The value is kept in the system keyring, never in a file.
- Smart collections (header: Smart): saved rules over every source, e.g. unwatched 4K movies, added this week, rated 8+ dramas from the 90s. Five starters; create, edit, delete and reorder your own with a live match count. Stored only in `smart.json` on this computer. Resolution rules need a server (local folders sit them out).
- Optional online details for local media (Local folders > Online details, off by default, per source): TMDB (your own free key, kept in the keyring), TVmaze and MusicBrainz with the Cover Art Archive. Only title, year, ids and season / episode numbers (album and artist for music) are sent, never file names or paths. Unclear matches are listed under "Choose the right match". Results sit below your own edits, `.nfo` files and sidecar art, and TMDB data is refreshed or dropped within six months. Provider credits and the TMDB notice are shown ([METADATA.md](docs/METADATA.md)).
- Optional subtitle download for local files: "Find subtitles online" searches OpenSubtitles with your own API key (and, for downloads, your OpenSubtitles login; both in the keyring) and keeps the file in NOCTURNE's own folder, never next to media.
- Offline copies: "Download to this computer" in the ... menu of a server movie, episode, season or show saves the original file(s) into a local library folder (or NOCTURNE's downloads folder), named the Jellyfin way with a small `.nfo` so the copy shows as a second source of the same title. GET only, resumable, never overwrites, one transfer at a time, queue survives restarts (account menu > Downloads), external subtitles come along.

**Across services**
- One watched state across sources (Settings > Watched state, opt-in): marking a title watched also marks its copies on your other sources, matched by IMDb / TMDB / TVDB id (episodes: series ids plus season and episode), never by title alone. Server copies change only when server changes are on, each on its own server; local copies always follow. Changes that arrive from elsewhere (Trakt import) never spread further ([WRITES.md](docs/WRITES.md)).
- Trakt (Settings > Trakt, opt-in): connect your own Trakt API app with a browser sign-in (PKCE, loopback redirect; no credentials shipped, no client secret; client id and sign-in live in the keyring), scrobble movies and episodes that have provider ids, and optionally import watched history into local items (server items only with server changes on) ([TRAKT.md](docs/TRAKT.md)).
- Remote control from other Jellyfin apps (opt-in, off by default, needs server changes on): this computer appears as a controllable device; play, pause, seek, next / previous, stop, volume, mute, audio / subtitle track and on-screen messages sent from another app are carried out ([WRITES.md](docs/WRITES.md)).

**Couch mode and platforms**
- Couch mode (Settings > Living room: Off / On / Automatic): a 10-foot layout with a larger UI, thicker focus ring, a hint bar, and gamepad support (`gilrs`, a background thread only while the mode is not Off): D-pad / left stick move, A select, B back, Y search, X favourite, Start account menu, LB / RB page, LT / RT seek 30 s. Automatic turns on while a gamepad is connected or on a 1900 px+ display. Remote / media keys share the keyboard path.
- macOS and Windows code behind the platform seams (preview: built by a one-shot CI workflow, never run on a real PC or Mac): per-OS paths, keyring backends, named-pipe mpv IPC, per-OS libmpv loading, single-instance guard, playback through the external mpv window. `scripts/check-cross.sh` runs the compile checks in Docker ([PLATFORMS.md](docs/PLATFORMS.md)).
- `.deb` and `.rpm` packages, built inside Debian / Ubuntu and Fedora containers and install-tested in clean containers (the Fedora one needs RPM Fusion for `mpv-libs`). No AppImage: it would be 107 MB and clash over libmpv / glibc ([PACKAGING.md](docs/PACKAGING.md)).
- Header links that scroll sideways show a fade and chevron at the hidden end and follow the focused link.

### Changed
- Playback reports carry the play method, media source and play session id, so the server can tell direct play from a transcode.
- Pasted API keys (TMDB, OpenSubtitles, Trakt client id) and the extra-header value lose surrounding whitespace and one pair of quotes.
- libmpv older than 0.40 is tolerated (background options are skipped).
- With no graphical session the app prints a clear message instead of panicking.

### Fixed
- Online details can no longer attach to the wrong local file after an interrupted scan (item ids are never reused, each online entry is tied to the file it was fetched for, and purged items lose their entries and artwork).
- The online subtitle finder is usable with arrows, Enter and a gamepad, and a download without an OpenSubtitles login explains that a login is needed.
- A failed playback report shows a quiet notice (once per playback) instead of being swallowed; the final "stopped" report is retried once.
- 0.3.1 fix batch: the divider rule no longer runs through the genre chips on detail pages, and the chip band grows when chips wrap.
- Repository setup instructions work in fish (a one-line `printf` instead of a heredoc), with troubleshooting notes.

## [0.3.0] - 2026-11-16

A new foundation: the interface is rebuilt on Iced, and NOCTURNE becomes a media center for local folders and several Jellyfin servers in one library.

### Added
- **Local library.** Add folders in Settings > Sources (portal chooser or typed path). NOCTURNE scans them (video, audio and image files; names such as `Title (Year)` and `Show/Season 01/... S01E02 ...`; `.nfo` files and sidecar artwork; stream details from `ffprobe` when installed), with live progress, cancel, incremental rescans and availability for unplugged drives. Files that cannot be matched are listed under Unmatched and can be matched by hand. Resume position and watched state for local items are kept in NOCTURNE's own data directory. Local files are never modified, moved or deleted.
- **Several servers, one library.** Any number of Jellyfin servers, each with its own account, plus local folders, behind one catalog. Home, browse and search merge them; a header filter narrows to one source and small source marks show where an item lives. A server that is signed out or offline shows a quiet banner and retries with backoff without affecting the others. Settings > Sources shows status and offers sign in again, rescan, enable / disable, priority order and remove.
- **Duplicates as versions and sources.** The same title found in several places appears once (matched on provider ids, never on title alone). The detail page has one "Versions and sources" picker listing a server's own versions (for example 4K and 1080p) and copies on other servers or in local folders, with "Always prefer this source".
- **First-run setup.** A short onboarding flow (five steps for a fresh install, two for an upgrader) offers Local library, Connect to a server, or Both; Settings > Run setup again repeats it.
- **Single instance.** Launching NOCTURNE again raises the running window instead of opening a second one.
- **Player loading overlay.** Opening a stream shows a Stremio-style splash (the item's backdrop, its logo filling with the buffer and a status line); a lighter version appears on stalls during playback.
- **Reduce motion** setting (follows the desktop preference by default).
- **Watch together (SyncPlay):** press `g` or use the account menu to list, create, join and leave groups on your server. In a group, choosing something to play starts it for everyone, and play, pause, seek, stop and next-item stay in step across members' players (clock sync against the server, command scheduling, small drift corrected by nudging mpv's speed and large drift by seeking, and the Ready / Buffering handshake so the group waits for the slowest member). A pill in the header and on the player shows the group and has Leave. Works with the embedded and the external mpv player. It needs server changes switched on (leaving never does) and a user whose policy allows SyncPlay. `NOCTURNE_DEVICE_ID` overrides the device id the server keys sessions by.

### Changed
- **The interface is built on Iced 0.14; GPUI is removed.** Same design, layouts and keys. Video still plays inside the window on Wayland (libmpv's OpenGL render API in a subsurface below the transparent window) and in an external mpv window on X11. The pinned `gpui` and the patched `vendor/xattr` crate are gone.
- **libmpv is loaded at run time** instead of linked, so a missing `libmpv.so.2` is an error the app can handle: browsing keeps working, playback falls back to the `mpv` program, and otherwise the app shows a per-distribution install hint. The tarball `install.sh` now checks the runtime libraries and installs missing ones through the distribution's package manager (`--no-deps` skips it). The Arch package declares `mpv` explicitly.
- **Release builds no longer read `.env`.** Only debug builds look for one, so a stray file in a parent directory cannot point an installed app at a server. `NOCTURNE_JF_URL` / `NOCTURNE_JF_TOKEN` from the environment still add a transient read-only source.
- Server changes go only to the server an item came from; the single `allow_server_writes` switch still gates all of them.
- Upgrading from 0.2: your server login becomes a source automatically (`sources.toml`); settings are kept; the old `servers.toml` and `accounts.toml` are left in place. Nothing is changed on any server.
- Documentation rewritten for the new architecture (`docs/ARCHITECTURE.md`, `docs/SOURCES.md`, `docs/DECISIONS.md`, `docs/PLATFORMS.md`, `docs/HDR.md`, `docs/METADATA.md`). The "Linux only" statements became "Linux first; macOS and Windows planned, not available".

### Fixed
- Clicking Home with the mouse could leave the header highlighting a library (for example Movies) instead of Home, because the Home page kept the keyboard focus on a header entry.
- Wide-tracked uppercase labels dropped or clipped glyphs; letter-spaced text now renders every character.
- Buttons in dialogs and the Sources screens are normal-sized again after the port.

## [0.2.0] - 2026-11-15

### Added
- Profile picker ("Who's watching?"): when the last used server has two or more remembered accounts and no default, the app opens a full-screen picker of large round profile tiles (profile picture, or the initial, with the name below) and an "Add profile" tile that opens the usual sign-in form. Pick with the mouse, Left / Right (Up / Down in wrapped rows) and Enter; Esc does nothing. Choosing a profile signs in with its stored token, no password, and shows Home; if the server rejects the token the sign-in form opens with the name already filled in. A "Use as default profile" box (Space or click; off by default) makes the chosen profile open at launch without the picker. The account menu has "Switch profile" (always opens the picker, never clears the default by itself), Settings has a "Default profile" drop-down ("Ask every time" or a profile), and removing or signing out the default account clears it. One remembered account behaves as before, and developer-token mode never shows the picker. Switching profile is a local sign-in with an existing token: nothing is written to the server and the server-changes switch is not involved. `accounts.toml` gains an optional `default = true` marker on one entry per server (no token; older files load unchanged).

### Changed
- Contrast and Settings controls: tertiary text rose from 3.5:1 to at least 4.6:1 and secondary text is lighter, so every text token now reaches WCAG AA 4.5:1 on every surface (a unit test checks all token and surface pairs). Interactive controls (buttons, inputs, chips, avatar buttons, profile tiles) got a new `edge` outline of at least 3:1. Settings value controls are now 32 px filled drop-downs with 13 px text, a caret and clear hover, focus, pressed and open states; On / Off settings are switches that keep the ON / OFF word; the selected row has an accent bar; section captions, descriptions and row spacing are larger and easier to read.
- Settings: options with several choices (audio and subtitle language, subtitles, buffer ahead, refill below, max RAM buffer, grid density, image cache) are now drop-downs. Click the value (or press Enter / Space) to open a menu under it with the current choice marked; pick one to apply, Esc or a click elsewhere closes it unchanged, and Up / Down / Home / End / Enter work from the keyboard. On / Off settings still flip with one click. The language lists now offer Automatic plus 28 common languages instead of nine; saved values keep their format and an unknown saved value is shown as it is.

### Fixed
- The round avatar button in the header showed only the letter initial even when the account had a profile image. It now shows the picture (clipped to a circle), also larger in the account menu next to the name, on the "Switch to ..." rows and in the Admin Users list. The initial stays while the picture loads, if it fails, and when none is set. The picture is read with GET `/Users/{id}/Images/Primary`, cached on disk by server, user and image tag, and follows a change at once: setting or removing it from the app, or switching account, needs no restart. `accounts.toml` now also keeps the server's public image tag per account (no token, no image data).
- The tarball installer (`install.sh`) now writes the absolute binary path into the desktop entry, refreshes the desktop and icon caches, and says what to do when the app does not appear in the launcher. In 0.1.0 some launchers did not list NOCTURNE after a tarball install.
- Settings: the page could not be scrolled, so the settings below the fold were out of reach. It now scrolls with the wheel, trackpad, PgUp / PgDn, Home / End and the arrow keys, which keep the selected row in view.

## [0.1.0] - 2026-11-15

First release. Linux only.

### Added
- Native GPUI desktop client for Jellyfin with a near-black, hairline, single-accent design.
- Browsing: home (Continue Watching, Next Up, latest), paged libraries with sort, filters, A-Z and three views, genres, studios, years, tags, collections, playlists, favourites, search, people pages, detail pages for movies, series, seasons (season cards) and episodes, a photo viewer and basic book pages.
- In-window GPU video through libmpv on Wayland, with an external mpv fallback on other sessions; chapters, trickplay, audio and subtitle menus, delays, speed, picture controls, skip intro and credits, next-episode autoplay, queue, quality selector with transcode fallback, mini player, MPRIS, idle inhibit, buffering watermarks.
- Accounts: password sign-in, Quick Connect (including authorizing another device), keyring-backed session restore, switching between remembered accounts without a password, own profile image and display language, own devices, session-ended and offline handling, and a global `?` shortcut reference.
- Playlists you own (create, add, remove, reorder, rename, delete) and a music queue (play next, add, reorder, remove, shuffle, repeat).
- Admin section for administrator accounts: dashboard, users (access and limits, profile image), libraries (add, remove, rename, folders, options, providers, refresh schedule), scheduled tasks and triggers, devices, activity, logs, API keys, branding, networking (safe subset), transcoding, trickplay, backups, plugins and plugin repositories. Every change is behind a confirmation dialog.
- Read-only by default: one master switch (`allow_server_writes`) gates every server change; the browsing crate is GET-only. See [docs/WRITES.md](docs/WRITES.md).
- Packaging: release tarball with installer, Arch PKGBUILD, desktop entry, icons, AppStream metadata.

### Known limitations
See [docs/ROADMAP.md](docs/ROADMAP.md).

[0.3.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.3.0
[0.2.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.2.0
[0.1.0]: https://github.com/ifsvivek/nocturne/releases/tag/v0.1.0
