<script>
  import Shot from "./Shot.svelte";
  const player = [
    "Audio and subtitle menus, delays and styling",
    "Speed and picture controls",
    "Chapters and trickplay thumbnails on the seek bar",
    "Skip intro and credits, next-episode autoplay, queue",
    "Quality choice with transcode fallback",
    "Mini player, stats overlay, MPRIS media keys",
  ];
  // Slot widths for the browser's srcset choice: full-bleed (max 1680), the 3:2 split in the 1200 wrap, two-up at 1520, one framed 1320.
  const bleed = "(min-width: 1680px) 1680px, 100vw";
  const left = "(min-width: 1240px) 701px, (min-width: 768px) calc((100vw - 72px) * 0.6), calc(100vw - 40px)";
  const right = "(min-width: 1240px) 467px, (min-width: 768px) calc((100vw - 72px) * 0.4), calc(100vw - 40px)";
  const two = "(min-width: 1560px) 748px, (min-width: 768px) calc(50vw - 32px), calc(100vw - 40px)";
</script>

<!-- Six scenes. Each is a large still from the app with its copy set as a subtitle. -->
<div id="tour">
  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s1">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 1</span><h2 id="s1" class="scene-title">Browse</h2></header>
    <Shot name="home" bleed sizes={bleed} class="reveal" />
    <p class="subtitle">Home picks up where you left off with Continue Watching and Next Up. Libraries page through with sort, filters and an A-Z rail.</p>
    <div class="stagger wrap mt-[clamp(56px,7vw,96px)] grid gap-x-8 gap-y-12 md:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] md:items-end">
      <figure class="m-0"><Shot name="library" sizes={left} /><figcaption class="mt-4 text-sm text-ink-2">Posters, thumbs or lists, by genre, studio, year, tag, collection, playlist or favourite.</figcaption></figure>
      <figure class="m-0"><Shot name="search-crop" sizes={right} /><figcaption class="mt-4 text-sm text-ink-2"><kbd>Ctrl</kbd> <kbd>K</kbd> or <kbd>/</kbd> searches from any screen.</figcaption></figure>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s2">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 2</span><h2 id="s2" class="scene-title">Details</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="movie-detail" sizes={two} />
      <Shot name="series-episodes" sizes={two} />
    </div>
    <p class="subtitle">Movies, series, seasons and episodes get full pages with cast, chapters, similar items, extras and media info. Season tabs lead into an episode grid and autoplay into the next one.</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s3">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 3</span><h2 id="s3" class="scene-title">Playback</h2></header>
    <Shot name="playback" bleed sizes={bleed} class="letterbox" />
    <p class="subtitle">The film plays inside the window. libmpv renders with hardware decoding into a Wayland subsurface, and the controls are drawn over the picture.</p>
    <div class="wrap mt-14">
      <ul class="tick stagger gap-x-12 sm:grid-cols-2 lg:grid-cols-3">{#each player as p}<li>{p}</li>{/each}</ul>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s4">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 4</span><h2 id="s4" class="scene-title">Profiles</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="picker-crop" sizes={two} />
      <Shot name="account-menu" sizes={two} />
    </div>
    <p class="subtitle">With two or more remembered accounts on a server, NOCTURNE opens on Who's watching?. Pick a profile with the mouse or the arrow keys and Enter, no password. Tick Use as default profile to skip the picker next time. The account menu has Switch profile, and profile pictures show in the header and the menu. Switching is a local sign-in: nothing is written to the server.</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s5">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 5</span><h2 id="s5" class="scene-title">Settings</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="settings" sizes={two} />
      <Shot name="settings-dropdown" sizes={two} />
    </div>
    <p class="subtitle">Options with several choices are drop-downs: click the value, pick one, and Esc closes it unchanged. On and Off settings are switches. A Default profile drop-down sets who opens at launch. Changes apply immediately and are saved on this device.</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s6">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 6</span><h2 id="s6" class="scene-title">Admin</h2></header>
    <div class="reveal mx-auto w-[min(1320px,100%-40px)]"><Shot name="admin-dashboard" sizes="(min-width: 1360px) 1320px, calc(100vw - 40px)" /></div>
    <p class="subtitle">Administrators get the server dashboard in the same app: activity, sessions, devices, tasks, libraries, users, plugins, API keys and logs. Every change asks first.</p>
  </section>
</div>
