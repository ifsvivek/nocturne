<script>
  import Shot from "./Shot.svelte";
</script>

<section id="tour" class="py-[clamp(64px,9vw,136px)]" aria-labelledby="tour-h">
  <div class="wrap">
    <div class="reveal max-w-[60ch]">
      <h2 id="tour-h" class="h2">A tour of the app</h2>
      <p class="lede mt-4">Every screenshot is the real application, captured against a generated demo library.</p>
    </div>

    <!-- Browse: copy left, shot right -->
    <div class="mt-[clamp(40px,6vw,80px)] grid items-center gap-8 min-[900px]:grid-cols-[5fr_7fr] min-[900px]:gap-14">
      <div class="reveal grid content-start gap-4">
        <h3 class="h3">Browse like you mean it</h3>
        <p class="lede">Home with Continue Watching and Next Up, then paged libraries with sort, filters, an A-Z rail and poster, thumb or list views.</p>
        <ul class="tick">
          <li>Genres, studios, years, tags, collections, playlists and favourites</li>
          <li>Global search on <kbd>Ctrl</kbd>+<kbd>K</kbd> or <kbd>/</kbd></li>
          <li>Virtualised grids, blurhash placeholders and a disk image cache</li>
        </ul>
      </div>
      <div class="reveal"><Shot name="library" sizes="(min-width: 900px) 660px, 100vw" /></div>
    </div>

    <!-- Details and search: uneven bento, three cells -->
    <div class="mt-[clamp(72px,10vw,150px)] grid gap-5 min-[900px]:grid-cols-12">
      <article class="reveal grid content-start gap-4 min-[900px]:col-span-7">
        <Shot name="movie-detail" fit="min-[900px]:h-[340px] object-cover object-top" sizes="(min-width: 900px) 680px, 100vw" />
        <div class="px-1.5">
          <h3 class="h3">Details that go deep</h3>
          <p class="lede mt-2">Movie, series, season and episode pages with cast, chapters, similar items, extras and media info. People have pages too.</p>
        </div>
      </article>
      <article class="reveal grid content-start gap-4 min-[900px]:col-span-5">
        <Shot name="search" fit="min-[900px]:h-[340px] object-cover object-top" sizes="(min-width: 900px) 480px, 100vw" />
        <div class="px-1.5">
          <h3 class="h3">Search from anywhere</h3>
          <p class="lede mt-2">Press <kbd>Ctrl</kbd>+<kbd>K</kbd> or <kbd>/</kbd> on any screen and start typing.</p>
        </div>
      </article>
      <article class="reveal grid content-start gap-4 min-[900px]:col-span-12 min-[900px]:grid-cols-[7fr_5fr] min-[900px]:items-center min-[900px]:gap-14">
        <Shot name="series-episodes" sizes="(min-width: 900px) 680px, 100vw" />
        <div class="px-1.5">
          <h3 class="h3">Series, season by season</h3>
          <p class="lede mt-2">Season tabs with counts, an episode card grid, Next Up and autoplay into the next episode.</p>
        </div>
      </article>
    </div>

    <!-- Playback: full width -->
    <div class="mt-[clamp(72px,10vw,150px)]">
      <div class="reveal"><Shot name="playback" sizes="(min-width: 1220px) 1180px, 100vw" /></div>
      <div class="mt-9 grid gap-8 min-[900px]:grid-cols-2 min-[900px]:gap-14">
        <div class="reveal grid content-start gap-4">
          <h3 class="h3">Playback inside the window</h3>
          <p class="lede">libmpv renders with hardware decoding into a Wayland subsurface beneath a transparent GPUI window. The video sits in the app and the controls are drawn over the picture.</p>
        </div>
        <ul class="tick reveal">
          <li>Audio and subtitle menus, delays and styling</li>
          <li>Speed and picture controls, chapters and trickplay thumbnails on the seek bar</li>
          <li>Skip intro and credits, next-episode autoplay, queue</li>
          <li>Quality choice with transcode fallback, mini player, stats overlay</li>
          <li>MPRIS media keys and idle inhibit</li>
        </ul>
      </div>
    </div>

    <!-- Admin: shot left, copy right -->
    <div class="mt-[clamp(72px,10vw,150px)] grid items-center gap-8 min-[900px]:grid-cols-[7fr_5fr] min-[900px]:gap-14">
      <div class="reveal min-[900px]:order-none"><Shot name="admin-dashboard" sizes="(min-width: 900px) 680px, 100vw" /></div>
      <div class="reveal grid content-start gap-4">
        <h3 class="h3">The admin dashboard is in the same app</h3>
        <p class="lede">Administrator accounts get server info, activity, live sessions, devices, scheduled tasks, libraries, users, plugins, API keys and logs. One app, no second binary.</p>
        <p class="lede">Every change asks first, and none are possible until you allow writes.</p>
      </div>
    </div>
  </div>
</section>
