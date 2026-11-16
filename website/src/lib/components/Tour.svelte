<script>
  import Shot from "./Shot.svelte";
  import Next from "./Next.svelte";
  import { has } from "#lib/site.js";
  const sources = [
    "First run asks: a local library, a server, or both",
    "Several Jellyfin servers, each with its own account",
    "A source filter, and a small mark on every card",
    "Copies merged only on IMDb, TMDB or TVDB ids, never on a title alone",
    "Kodi-style .nfo files and poster and fanart next to your files",
    "One server offline, the rest keep playing",
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
  ];
  // Slot widths for the browser's srcset choice: full-bleed (max 1680), the 3:2 split in the 1200 wrap, two-up at 1520, one framed 1320.
  const bleed = "(min-width: 1680px) 1680px, 100vw";
  const left = "(min-width: 1240px) 701px, (min-width: 768px) calc((100vw - 72px) * 0.6), calc(100vw - 40px)";
  const right = "(min-width: 1240px) 467px, (min-width: 768px) calc((100vw - 72px) * 0.4), calc(100vw - 40px)";
  const two = "(min-width: 1560px) 748px, (min-width: 768px) calc(50vw - 32px), calc(100vw - 40px)";
</script>

<!-- Seven scenes. The first is the story (many sources, one library); the others are large stills from the app with
     their copy set as a subtitle. -->
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
    {#if has("first-run") && has("sources")}
      <div class="stagger mx-auto mt-[clamp(56px,7vw,96px)] grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
        <Shot name="first-run" sizes={two} />
        <Shot name="sources" sizes={two} />
      </div>
    {/if}
    <div class="wrap mt-14">
      <ul class="tick stagger gap-x-12 sm:grid-cols-2 lg:grid-cols-3">{#each sources as p}<li>{p}</li>{/each}</ul>
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
      <Shot name="movie-detail" sizes={two} />
      <Shot name="series-episodes" sizes={two} />
    </div>
    <p class="subtitle">Movies, series, seasons and episodes get full pages with cast, chapters, similar items, extras and media info. Season tabs lead into an episode grid and autoplay into the next one. When a title lives on more than one source, the page lists the other copies as alternate sources, with quality and availability. <Next /></p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s3">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 4</span><h2 id="s3" class="scene-title">Playback</h2></header>
    <Shot name="playback" bleed sizes={bleed} class="letterbox" />
    <p class="subtitle">The film plays inside the window, whether it streams from a server or sits on your disk. libmpv renders with hardware decoding into a Wayland subsurface, and the controls are drawn over the picture.</p>
    <div class="wrap mt-14">
      <ul class="tick stagger gap-x-12 sm:grid-cols-2 lg:grid-cols-3">{#each player as p}<li>{#if Array.isArray(p)}{p[0]} <Next v={p[1]} />{:else}{p}{/if}</li>{/each}</ul>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s4">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 5</span><h2 id="s4" class="scene-title">Profiles</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="picker-crop" sizes={two} />
      <Shot name="account-menu" sizes={two} />
    </div>
    <p class="subtitle">With two or more remembered accounts on a Jellyfin server, NOCTURNE opens on Who's watching?. Pick a profile with the mouse or the arrow keys and Enter, no password. Tick Use as default profile to skip the picker next time. The account menu has Switch profile, and profile pictures show in the header and the menu. Switching is a local sign-in: nothing is written to the server.</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s5">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 6</span><h2 id="s5" class="scene-title">Settings</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="settings" sizes={two} />
      <Shot name="settings-dropdown" sizes={two} />
    </div>
    <p class="subtitle">Options with several choices are drop-downs: click the value, pick one, and Esc closes it unchanged. On and Off settings are switches. A Default profile drop-down sets who opens at launch, and a Sources section adds, orders and signs in to your folders and servers. <Next /> Changes apply immediately and are saved on this device.</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s6">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 7</span><h2 id="s6" class="scene-title">Admin</h2></header>
    <div class="reveal mx-auto w-[min(1320px,100%-40px)]"><Shot name="admin-dashboard" sizes="(min-width: 1360px) 1320px, calc(100vw - 40px)" /></div>
    <p class="subtitle">Administrators get each server's dashboard in the same app: activity, sessions, devices, tasks, libraries, users, plugins, API keys and logs. Every change asks first.</p>
  </section>
</div>
