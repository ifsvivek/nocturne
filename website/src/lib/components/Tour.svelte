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
</script>

<!-- Four scenes. Each is a large still from the app with its copy set as a subtitle. -->
<div id="tour">
  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s1">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 1</span><h2 id="s1" class="scene-title">Browse</h2></header>
    <Shot name="home" bleed sizes="(min-width: 1680px) 1680px, 100vw" class="reveal" />
    <p class="subtitle">Home picks up where you left off with Continue Watching and Next Up. Libraries page through with sort, filters and an A-Z rail.</p>
    <div class="stagger wrap mt-[clamp(56px,7vw,96px)] grid gap-x-8 gap-y-12 md:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] md:items-end">
      <figure class="m-0"><Shot name="library" sizes="(min-width: 768px) 700px, 100vw" /><figcaption class="mt-4 text-sm text-ink-2">Posters, thumbs or lists, by genre, studio, year, tag, collection, playlist or favourite.</figcaption></figure>
      <figure class="m-0"><Shot name="search-crop" sizes="(min-width: 768px) 470px, 100vw" /><figcaption class="mt-4 text-sm text-ink-2"><kbd>Ctrl</kbd> <kbd>K</kbd> or <kbd>/</kbd> searches from any screen.</figcaption></figure>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s2">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 2</span><h2 id="s2" class="scene-title">Details</h2></header>
    <div class="stagger mx-auto grid w-[min(1520px,100%-40px)] gap-6 md:grid-cols-2">
      <Shot name="movie-detail" sizes="(min-width: 768px) 760px, 100vw" />
      <Shot name="series-episodes" sizes="(min-width: 768px) 760px, 100vw" />
    </div>
    <p class="subtitle">Movies, series, seasons and episodes get full pages with cast, chapters, similar items, extras and media info. Season tabs lead into an episode grid and autoplay into the next one.</p>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s3">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 3</span><h2 id="s3" class="scene-title">Playback</h2></header>
    <Shot name="playback" bleed sizes="(min-width: 1680px) 1680px, 100vw" class="letterbox" />
    <p class="subtitle">The film plays inside the window. libmpv renders with hardware decoding into a Wayland subsurface, and the controls are drawn over the picture.</p>
    <div class="wrap mt-14">
      <ul class="tick stagger gap-x-12 sm:grid-cols-2 lg:grid-cols-3">{#each player as p}<li>{p}</li>{/each}</ul>
    </div>
  </section>

  <section class="pb-[clamp(110px,14vw,220px)]" aria-labelledby="s4">
    <header class="reveal wrap mb-[clamp(32px,4vw,56px)] text-center"><span class="role">Scene 4</span><h2 id="s4" class="scene-title">Admin</h2></header>
    <div class="reveal mx-auto w-[min(1320px,100%-40px)]"><Shot name="admin-dashboard" sizes="(min-width: 1360px) 1320px, 100vw" /></div>
    <p class="subtitle">Administrators get the server dashboard in the same app: activity, sessions, devices, tasks, libraries, users, plugins, API keys and logs. Every change asks first.</p>
  </section>
</div>
