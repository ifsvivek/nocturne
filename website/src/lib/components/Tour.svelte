<script>
  import Shot from "./Shot.svelte";
  import Next from "./Next.svelte";
  import { shipped } from "#lib/site.js";
  // 0.5.2 copy is not marked ahead of time: it shows only once VERSION reaches 0.5.2 and replaces the 0.5 copy it outdates.
  const v052 = shipped("0.5.2");
  // 0.6.0 copy (player choice, Calendar, Requests, links, themes gallery) is gated the same way and text only until its captures exist.
  const v060 = shipped("0.6.0");
  const sources = [
    "A guided setup: local folders, a server, or both, then the promises and the real read-only switch",
    "Several Jellyfin servers, each with its own account",
    "A source filter, and a small mark on every card",
    "Copies merged only on IMDb, TMDB or TVDB ids, never on a title alone",
    "Kodi-style .nfo files and poster and fanart next to your files",
    "One server offline, the rest keep playing",
    ["Download a server film, episode or show to this computer; the copy appears as a second source", "0.4.0"],
    ["Smart collections: saved rules over every source, such as unwatched films added this week", "0.4.0"],
    ["Online details for local files, off by default: TMDB with your own key, TVmaze, MusicBrainz", "0.4.0"],
  ];
  const player = [
    "Audio and subtitle menus, delays and styling",
    "Speed and picture controls",
    "Chapters and trickplay thumbnails on the seek bar",
    "Skip intro and credits, next-episode autoplay, queue",
    "Quality choice with transcode fallback",
    "Mini player, stats overlay, MPRIS media keys",
    ["Local files play through the same player, with their own resume points", "0.3.0"],
    ["Watch together (SyncPlay) on a Jellyfin server", "0.3.0"],
    ["A second subtitle track at once, and audio and subtitle languages remembered per show on this computer", "0.4.0"],
    ["HDR video: play it in the window in SDR, or in an HDR pop-out window. Real HDR output depends on your display and mpv 0.42 or newer", "0.4.0"],
    ["Find subtitles online for local files: OpenSubtitles, with your own key", "0.4.0"],
    ["Playback reports tell the server whether it is direct play or a transcode", "0.4.0"],
    ...(v060 ? ["Choose where video plays: in the window (the default), in VLC or in an mpv window, each offered only when installed. VLC is controlled over a local, password-protected interface, so your place, progress sync, Trakt and Stop still work; use VLC's own subtitle and audio menus there"] : []),
  ];
  const couch = [
    "D-pad or left stick to move, A to select, B to go back",
    "Y to search, X to favourite, Start for the account menu",
    "LB and RB to page, LT and RT to seek 30 seconds",
    "Remote and media keys work like the keyboard",
  ];
  // Each one is off until you turn it on.
  const services = [
    ["Trakt", "Scrobble what you watch and import your watched history. Connect Trakt signs in through your browser with one click; your own Trakt app stays an advanced option. Only titles with IMDb, TMDB or TVDB ids are sent.",
      "Trakt is a trademark of Trakt, Inc. NOCTURNE uses the Trakt API but is not affiliated with or endorsed by Trakt."],
    ["Remote control", "Other Jellyfin apps see this computer as a device and can play, pause, seek, skip, stop, and change the volume and tracks. Needs server changes on. Tried through the server, not yet from a phone app.", ""],
    ["One watched state", "Mark a title watched and its copies on your other sources follow, matched by IMDb, TMDB or TVDB id, never by title. Server copies change only with server changes on, each on its own server.", ""],
  ];
  // Themes and Server UI: [title, text]. 0.5.2 rebuilt both on the native CSS engine.
  const look = v052 ? [
    ...(v060 ? [["Themes gallery", "Settings, Themes opens on a Gallery with the tabs Official, Community and My themes: a card for each theme with a preview, its author and licence, and one click to install. Downloads come from this website over https, are checked against a published sha256, and stop when you turn theme downloads off. Anyone can propose a community theme by pull request. Themes you add by hand keep working and are marked as not from the gallery."]] : []),
    ["Theme Studio", "Themes, Open Studio: point at any part of the live app and change it with controls for colour, size, spacing, shape, shadow, type, layout and hover, on that element only or on all like it. Undo and redo, and a CSS tab that stays in sync both ways: the Studio writes real CSS you can edit by hand, save, use for one server or all, and export."],
    ["NOCTURNE and Glass", "Two built-in themes: NOCTURNE, the default look, and Glass, with a black canvas, a glass header pill, a full-bleed hero that rotates and shows the title as logo art, pill buttons and an accent rail before each section title. Spotlight, Marquee, Feed and Classic are starters to build from."],
    ["Paste a Jellyfin theme", "Paste a stylesheet, its @import line or an https address. Its imports, fonts and pictures are fetched, the preview covers the whole app until you Keep or Revert, and Settings, Themes lists what was fetched, which fonts loaded and what did not apply."],
    ["Fetched safely", "Theme files come over https from public addresses only (your own server excepted), with no cookies or sign-in and with size, time and count limits. They are kept in the cache and fetched again only when you ask, and one switch in Settings, Themes turns all downloading off."],
    ["Your server's own look", "When a source uses its server's UI and the server has its web client, Home, the header, libraries and the item page are built natively with the web client's own page structure, then styled with the server's own stylesheets, its branding and your theme, in the browser's order. Most community Jellyfin themes look as they do in the browser. Nothing is bundled and nothing runs: the stylesheets and the icon font are read from the server."],
    ["One server's UI over another", "Each source, All and your local folders can use NOCTURNE's layout, their own server's UI or another server's: its home sections and its look, filled with what you are browsing. Only in this app: nothing is written to either server."],
    ["Safe look", "Ctrl+Shift+0, or starting with NOCTURNE_SAFE_THEME=1, brings back the built-in look at once, whatever theme is saved."],
    ["Home sections", "Settings, Sources, Sections reorders and switches the server's home sections for your account. That is a change on the server, so it needs server changes on."],
  ] : [
    ["Themes", "Colours, corner radii, the window background and a font. Cards: shape, size, caption, shadow and a hover zoom. Home rows as a carousel or a grid, the hero as a billboard, a banner or none, the header's height and items, and the order of Home's sections."],
    ["Marquee and Feed", "Two built-in examples to start from: Marquee, a billboard and wide carousels without captions; Feed, a grid of thumbnails with the title and details below."],
    ["Import", "Paste a stylesheet, open a .css file or type one https address, fetched once and refreshed only when you ask. A preview lasts 15 seconds unless you keep it. Nothing in a stylesheet runs or loads, and every line NOCTURNE cannot use is listed with its line and column."],
    ["Per server", "One theme for the whole app, and optionally a different one for each server."],
    ["Server UI", "Optional per server, off for new ones. With that server alone in the source filter, Home follows the server's own layout: its home sections in its order and its libraries in your order. The server's branding CSS becomes its theme, and its sign-in page shows the server's login disclaimer."],
    ["Home sections", "Settings, Sources, Sections reorders and switches the server's home sections for your account. That is a change on the server, so it needs server changes on."],
  ];
  // Scene 11, shown from 0.6.0: [title, text].
  const extras = [
    ["Calendar", "A header link, or the c key, opens month, week and agenda views of upcoming episodes. Each server's list is merged, and with Trakt connected your Trakt calendar is added. Every entry has the poster, season and episode, the title and the air time in your time zone, and says On your server, Airing soon or Not on your servers. Enter plays an episode a server has. Reading only; an optional row on Home shows the next seven days."],
    ["Requests with Seerr", "Sign in to your Seerr server (Jellyseerr and Overseerr work too) with your own server account. With Allow requests on, a title that is not in your library shows a Request button in Search and on the Calendar, with a season choice for shows, and a Requests page lists what you asked for and its status. A request goes to the Seerr server only, after a confirmation, and nothing is written to a media server. Off by default; NOCTURNE does not download anything itself."],
  ];
  const tick = (p) => (Array.isArray(p) ? p : [p, ""]);
  // Slot widths for the browser's srcset choice: full-bleed (max 1680), the 3:2 split in the 1200 wrap, two-up at 1520, one framed 1320.
  const bleed = "(min-width: 1680px) 1680px, 100vw";
  const left = "(min-width: 1240px) 701px, (min-width: 768px) calc((100vw - 72px) * 0.6), calc(100vw - 40px)";
  const right = "(min-width: 1240px) 467px, (min-width: 768px) calc((100vw - 72px) * 0.4), calc(100vw - 40px)";
  const two = "(min-width: 1560px) 748px, (min-width: 768px) calc(50vw - 32px), calc(100vw - 40px)";
  const framed = "(min-width: 1360px) 1320px, calc(100vw - 40px)";
  const narrow = "(min-width: 800px) 760px, calc(100vw - 40px)";
</script>

<!-- Ten scenes (an eleventh from 0.6.0). The first is the story (many sources, one library); the others are large stills from the app with
     their copy set as a subtitle. 0.4 and 0.5 stills are 3840x2160 headless captures (see README). -->
<div id="tour">
  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s0">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 1</span><h2 id="s0" class="scene-title">One library</h2><Next class="mt-5 ml-0" /></header>
    <!-- Sources run as beams into one slit of light (the startup screen's moonlight), and leave as one line: the library. -->
    <figure class="flow reveal wrap my-0 grid h-[clamp(200px,30vw,360px)] grid-cols-[auto_minmax(0,1fr)_auto] items-stretch gap-x-[clamp(10px,2vw,28px)]">
      <ul class="m-0 grid list-none grid-rows-3 p-0 text-right">
        {#each ["Local folders", "Jellyfin server", "Another server"] as name}<li class="flow-src grid place-items-center justify-end">{name}</li>{/each}
      </ul>
      <svg class="size-full overflow-visible" viewBox="0 0 600 300" preserveAspectRatio="none" aria-hidden="true">
        <defs>
          <linearGradient id="slit" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="s-ink s-0"/><stop offset="0.5" class="s-ink"/><stop offset="1" class="s-ink s-0"/></linearGradient>
          <linearGradient id="flare" x1="0" y1="0" x2="1" y2="0"><stop offset="0" class="s-ink"/><stop offset="0.45" class="s-accent-ink s-80"/><stop offset="1" class="s-accent s-0"/></linearGradient>
          <mask id="wipe" maskUnits="userSpaceOnUse" x="0" y="0" width="600" height="300"><rect class="wipe" x="0" y="0" width="440" height="300" fill="white"/></mask>
          <radialGradient id="night" cx="0.5" cy="0.5" r="0.5"><stop offset="0" class="s-night"/><stop offset="1" class="s-night s-0"/></radialGradient>
        </defs>
        <ellipse class="flow-glow" cx="440" cy="150" rx="150" ry="150" fill="url(#night)"/>
        <g mask="url(#wipe)">
          <path class="beam" d="M0 50 C 230 50, 300 150, 438 150"/>
          <path class="beam" d="M0 150 L 438 150"/>
          <path class="beam" d="M0 250 C 230 250, 300 150, 438 150"/>
        </g>
        <rect class="slit" x="438" y="20" width="4" height="260" fill="url(#slit)"/>
        <rect class="flare" x="442" y="149" width="158" height="2" fill="url(#flare)"/>
      </svg>
      <p class="flow-out m-0 grid place-items-center">One library</p>
      <figcaption class="sr-only">Local folders, a Jellyfin server and another server flow into one library.</figcaption>
    </figure>
    <p class="subtitle">Point NOCTURNE at folders on your disk, at one or several Jellyfin servers, or at both. Everything lands in one catalog you browse and search in one place, and the same film on two sources shows once, with the other copy listed as an alternate.</p>
    <div class="reveal mx-auto mt-[clamp(56px,7vw,96px)] w-[min(1320px,100%-40px)]"><Shot name="first-run" sizes={framed} /></div>
    <div class="stagger mx-auto mt-6 grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <figure class="m-0"><Shot name="sources" sizes={two} /><figcaption class="mt-4 text-sm text-ink-2">Settings, Sources: the order is the priority when a title is in more than one place.</figcaption></figure>
      <figure class="m-0"><Shot name="local-folders" sizes={two} /><figcaption class="mt-4 text-sm text-ink-2">Local folders: indexed in place. Online details are optional, per provider, with their credits on the page.</figcaption></figure>
    </div>
    <div class="stagger wrap mt-[clamp(56px,7vw,96px)] grid gap-x-8 gap-y-12 md:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] md:items-end">
      <figure class="m-0"><Shot name="smart" sizes={left} /><figcaption class="mt-4 text-sm text-ink-2">A smart collection: a saved rule over every source, kept on this computer.</figcaption></figure>
      <figure class="m-0"><Shot name="smart-menu-crop" sizes={right} /><figcaption class="mt-4 text-sm text-ink-2">Five starters in the header's Smart menu; make your own with a live match count.</figcaption></figure>
    </div>
    <figure class="reveal mx-auto mt-[clamp(56px,7vw,96px)] mb-0 w-[min(760px,100%-40px)]"><Shot name="downloads-crop" sizes={narrow} /><figcaption class="mt-4 text-center text-sm text-ink-2">Download to this computer: the copy lands in your local library as a second source. Plain reads, resumable, one at a time.</figcaption></figure>
    <div class="wrap mt-14">
      <ul class="tick stagger gap-x-12 sm:grid-cols-2 lg:grid-cols-3">{#each sources.map(tick) as [t, v]}<li>{t}{#if v} <Next {v} />{/if}</li>{/each}</ul>
      <p class="m-0 mt-10 max-w-[80ch] text-sm text-ink-3">Online details come from <a href="https://www.themoviedb.org" rel="noopener">TMDB</a>, <a href="https://www.tvmaze.com" rel="noopener">TVmaze</a>, MusicBrainz and the Cover Art Archive. This product uses the TMDB API but is not endorsed or certified by TMDB. TV data from TVmaze.com, licensed CC BY-SA 4.0.</p>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s1">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 2</span><h2 id="s1" class="scene-title">Browse</h2></header>
    <Shot name="home" bleed sizes={bleed} class="reveal" />
    <p class="subtitle">Home picks up where you left off with Continue Watching and Next Up, from every source at once. Libraries page through with sort, filters and an A-Z rail.</p>
    <div class="stagger wrap mt-[clamp(56px,7vw,96px)] grid gap-x-8 gap-y-12 md:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] md:items-end">
      <figure class="m-0"><Shot name="library" sizes={left} /><figcaption class="mt-4 text-sm text-ink-2">Posters, thumbs or lists, by genre, studio, year, tag, collection, playlist or favourite.</figcaption></figure>
      <figure class="m-0"><Shot name="search-crop" sizes={right} /><figcaption class="mt-4 text-sm text-ink-2"><kbd>Ctrl</kbd> <kbd>K</kbd> or <kbd>/</kbd> searches every source from any screen.</figcaption></figure>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s2">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 3</span><h2 id="s2" class="scene-title">Details</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="alternates-crop" sizes={two} />
      <Shot name="series-episodes" sizes={two} />
    </div>
    <p class="subtitle">Movies, series, seasons and episodes get full pages with cast, chapters, similar items, extras and media info. Season tabs lead into an episode grid and autoplay into the next one. When a title lives on more than one source, Versions and sources lists every copy with its quality, and Play uses the one you prefer. <Next /> Movie and show pages also list the collections a title is included in (Jellyfin 12 and newer). <Next v="0.4.0" /></p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s3">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 4</span><h2 id="s3" class="scene-title">Playback</h2></header>
    <Shot name="playback" bleed sizes={bleed} class="letterbox" />
    <p class="subtitle">The film plays inside the window, whether it streams from a server or sits on your disk. libmpv renders with hardware decoding into a Wayland subsurface, and the controls are drawn over the picture.</p>
    <div class="stagger mx-auto mt-[clamp(56px,7vw,96px)] grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <figure class="m-0"><Shot name="loading" sizes={two} /><figcaption class="mt-4 text-sm text-ink-2">While it loads, the film's backdrop fills the screen and its title fills with light as the buffer grows.</figcaption></figure>
      <figure class="m-0"><Shot name="hdr-crop" sizes={two} /><figcaption class="mt-4 text-sm text-ink-2">An HDR film asks: SDR in the window, or the HDR pop-out window. Real HDR needs an HDR display and mpv 0.42 or newer.</figcaption></figure>
    </div>
    <div class="wrap mt-14">
      <ul class="tick stagger gap-x-12 sm:grid-cols-2 lg:grid-cols-3">{#each player.map(tick) as [t, v]}<li>{t}{#if v} <Next {v} />{/if}</li>{/each}</ul>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s7">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 5</span><h2 id="s7" class="scene-title">Living room</h2><Next v="0.4.0" class="mt-5 ml-0" /></header>
    <div class="reveal mx-auto w-[min(1320px,100%-40px)]"><Shot name="couch" sizes={framed} /></div>
    <p class="subtitle">Couch mode turns NOCTURNE into a ten-foot screen: a larger layout, a thicker focus ring and a hint bar, driven by a gamepad, a remote or the keyboard. Set it On, or Automatic to switch it on when a gamepad connects or on a large display.</p>
    <div class="wrap mt-14">
      <ul class="tick stagger gap-x-12 sm:grid-cols-2">{#each couch as c}<li>{c}</li>{/each}</ul>
      <p class="m-0 mt-8 text-sm text-ink-3">Not yet tried with a real gamepad or on a TV.</p>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s8">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 6</span><h2 id="s8" class="scene-title">Across services</h2><Next v="0.4.0" class="mt-5 ml-0" /></header>
    <p class="subtitle">Three connections beyond your sources. Each is off until you turn it on.</p>
    <dl class="stagger mx-auto mt-14 grid w-[min(760px,100%-40px)] gap-y-10">
      {#each services as [t, d, credit]}
        <div class="rule pt-5"><dt class="role">{t}</dt><dd class="m-0 mt-3 text-ink-2">{d}{#if credit}<span class="mt-3 block text-sm text-ink-3">{credit}</span>{/if}</dd></div>
      {/each}
    </dl>
    <figure class="reveal mx-auto mt-14 mb-0 w-[min(760px,100%-40px)]"><Shot name="trakt-crop" sizes={narrow} /><figcaption class="mt-4 text-center text-sm text-ink-2">Connect Trakt: one click, then a sign-in on trakt.tv in your browser. Your own Trakt app's Client ID stays an advanced option.</figcaption></figure>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s4">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 7</span><h2 id="s4" class="scene-title">Profiles</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="picker-crop" sizes={two} />
      <Shot name="account-menu" sizes={two} />
    </div>
    <p class="subtitle">With two or more remembered accounts on a Jellyfin server, NOCTURNE opens on Who's watching?. Pick a profile with the mouse or the arrow keys and Enter, no password. Tick Use as default profile to skip the picker next time. The account menu has Switch profile, and profile pictures show in the header and the menu. Switching is a local sign-in: nothing is written to the server.</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s5">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 8</span><h2 id="s5" class="scene-title">Settings</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="settings" sizes={two} />
      <Shot name="settings-dropdown" sizes={two} />
    </div>
    <p class="subtitle">Options with several choices are drop-downs: click the value, pick one, and Esc closes it unchanged. On and Off settings are switches. A Default profile drop-down sets who opens at launch, and a Sources section adds, orders and signs in to your folders and servers. <Next /> A server behind a gateway or reverse proxy can get an extra header, kept in the keyring. <Next v="0.4.0" /> Every text field edits like any other: select with the mouse or Shift and the arrows, double-click a word, copy, cut and paste. <Next v="0.5.0" /> Changes apply immediately and are saved on this device.</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s9">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 9</span><h2 id="s9" class="scene-title">Your look</h2><Next v="0.5.0" class="mt-5 ml-0" /></header>
    <div class="reveal mx-auto w-[min(1320px,100%-40px)]"><Shot name="themes" sizes={framed} /></div>
    <p class="subtitle">{v052
      ? "Themes are CSS that NOCTURNE lays out and draws itself, with no browser inside: real flex, grid and positioning on Home, the header, libraries, search, the detail page and the player's controls. Build one by pointing at the app, or paste one from the web."
      : "Themes are CSS that NOCTURNE reads itself and draws natively, with no browser inside. They restyle NOCTURNE's own parts, in Settings, Themes."}</p>
    <div class="stagger mx-auto mt-[clamp(56px,7vw,96px)] grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <figure class="m-0"><Shot name="marquee" sizes={two} /><figcaption class="mt-4 text-sm text-ink-2">Marquee: a billboard under a see-through header and wide carousels without captions.</figcaption></figure>
      <figure class="m-0"><Shot name="feed" sizes={two} /><figcaption class="mt-4 text-sm text-ink-2">Feed: no billboard, a grid of thumbnails with the title and details below.</figcaption></figure>
    </div>
    {#if v052}
    <div class="stagger mx-auto mt-[clamp(56px,7vw,96px)] grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <figure class="m-0"><Shot name="glass" sizes={two} /><figcaption class="mt-4 text-sm text-ink-2">Glass: a black canvas, a glass header pill, a full-bleed hero and squircle cards.</figcaption></figure>
      <figure class="m-0"><Shot name="studio" sizes={two} /><figcaption class="mt-4 text-sm text-ink-2">Theme Studio: pick an element of the live screen and change it with controls.</figcaption></figure>
    </div>
    {/if}
    <dl class="stagger mx-auto mt-14 grid w-[min(1000px,100%-40px)] gap-x-14 gap-y-10 md:grid-cols-2">
      {#each look as [t, d]}
        <div class="rule pt-5"><dt class="role">{t}</dt><dd class="m-0 mt-3 text-ink-2">{d}</dd></div>
      {/each}
    </dl>
    <div class="stagger wrap mt-[clamp(56px,7vw,96px)] grid gap-x-8 gap-y-12 md:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] md:items-end">
      <figure class="m-0"><Shot name="server-ui" sizes={left} /><figcaption class="mt-4 text-sm text-ink-2">Server UI: with Study alone in the source filter, Home follows that server's own layout and branding.</figcaption></figure>
      <figure class="m-0"><Shot name="sections-crop" sizes={right} /><figcaption class="mt-4 text-sm text-ink-2">Settings, Sources, Sections: the server's home sections for your account, editable once server changes are on.</figcaption></figure>
    </div>
    <p class="mx-auto mt-10 mb-0 w-[min(1000px,100%-40px)] text-sm text-ink-3">{v052
      ? "Not a browser: nothing in a theme runs, so what a theme builds with JavaScript is missing, and effects without a native equivalent differ: backdrop blur becomes a translucent fill, and masks, conic gradients and blur, saturate or contrast filters are not drawn. Settings, Admin and setup keep their own screens and take a theme's colours, radii and fonts."
      : "Not arbitrary web CSS: each page keeps its structure, and a Jellyfin web theme applies only partly, mostly its colours, radii and font. Letter spacing is read but not drawn; density, type size and backdrop blur are applied approximately."}</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s6">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 10</span><h2 id="s6" class="scene-title">Admin</h2></header>
    <div class="reveal mx-auto w-[min(1320px,100%-40px)]"><Shot name="admin-dashboard" sizes={framed} /></div>
    <p class="subtitle">Administrators get each server's dashboard in the same app: activity, sessions, devices, tasks, libraries, users, plugins, API keys and logs. It stays view-only until changes are allowed, and every change asks first.</p>
  </section>

  {#if v060}
  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s10">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 11</span><h2 id="s10" class="scene-title">Calendar and requests</h2></header>
    <p class="subtitle">Three more ways to fill the evening, each optional and each off or empty until you use it.</p>
    <dl class="stagger mx-auto mt-14 grid w-[min(1000px,100%-40px)] gap-x-14 gap-y-10 md:grid-cols-2">
      {#each extras as [t, d]}
        <div class="rule pt-5"><dt class="role">{t}</dt><dd class="m-0 mt-3 text-ink-2">{d}</dd></div>
      {/each}
    </dl>
  </section>
  {/if}
</div>
