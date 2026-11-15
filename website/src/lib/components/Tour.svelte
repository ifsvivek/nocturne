<script>
  import Shot from "./Shot.svelte";
  const rows = [["Genres", "Studios", "Years", "Tags", "Collections"], ["Playlists", "Favourites", "A-Z rail", "Poster, thumb or list"]];
  const player = [
    "Audio and subtitle menus, delays and styling",
    "Speed and picture controls",
    "Chapters and trickplay thumbnails on the seek bar",
    "Skip intro and credits, next-episode autoplay, queue",
    "Quality choice with transcode fallback",
    "Mini player and a stats overlay",
    "MPRIS media keys and idle inhibit",
  ];
</script>

<section id="tour" class="section relative isolate" aria-labelledby="tour-h">
  <div class="wrap">
    <div class="reveal max-w-[44rem]">
      <h2 id="tour-h" class="h2">The whole library, one window.</h2>
      <p class="lede mt-5">Every screenshot here is the real application, captured against a generated demo library.</p>
    </div>

    <!-- Browse: asymmetric bento, three cells. -->
    <div class="stagger mt-[clamp(48px,6vw,88px)] grid gap-4 md:grid-cols-2 lg:grid-cols-12 lg:grid-rows-[auto_auto]">
      <article class="reveal relative isolate flex min-h-[420px] flex-col overflow-hidden rounded-[1.25rem] bg-surface ring-1 ring-line ring-inset md:col-span-2 lg:col-span-8 lg:row-span-2">
        <div class="pool -right-24 -top-24 size-[420px] bg-indigo/25" aria-hidden="true"></div>
        <div class="max-w-[34rem] p-[clamp(24px,3.4vw,44px)]">
          <h3 class="h3">Browse like you mean it</h3>
          <p class="lede mt-3">Home with Continue Watching and Next Up, then paged libraries with sort and filters. Virtualised grids, blurhash placeholders and a disk image cache keep it quick.</p>
        </div>
        <div class="mt-auto pl-[clamp(24px,3.4vw,44px)]">
          <Shot name="library" sizes="(min-width: 1024px) 780px, 100vw" class="kenburns -mb-6 rounded-br-none" />
        </div>
      </article>

      <article class="reveal flex flex-col overflow-hidden rounded-[1.25rem] bg-surface ring-1 ring-line ring-inset lg:col-span-4">
        <div class="p-[clamp(24px,3vw,36px)] pb-5">
          <h3 class="h3">Search from anywhere</h3>
          <p class="keys muted mt-2">Press <kbd>Ctrl</kbd> <kbd>K</kbd> or <kbd>/</kbd> on any screen and start typing.</p>
        </div>
        <div class="mt-auto px-[clamp(16px,2vw,24px)] pb-[clamp(16px,2vw,24px)]">
          <Shot name="search-crop" sizes="(min-width: 1024px) 380px, (min-width: 768px) 50vw, 100vw" />
        </div>
      </article>

      <article class="reveal relative isolate overflow-hidden rounded-[1.25rem] bg-[linear-gradient(150deg,rgb(75_85_224/0.28),rgb(12_14_22/0.4)_60%)] p-[clamp(24px,3vw,36px)] ring-1 ring-white/10 ring-inset lg:col-span-4">
        <h3 class="h3">Every way in</h3>
        <!-- Each row repeats its chips four times; the copies are hidden from assistive tech and dropped without motion. -->
        <div class="marquee -mx-[clamp(24px,3vw,36px)] mt-6 grid gap-2 overflow-hidden">
          {#each rows as row, r}
            <ul class="marquee-track m-0 list-none p-0 {r ? 'rev' : ''}">
              {#each [0, 1, 2, 3] as copy}
                {#each row as w}<li class="rounded-full bg-white/[0.06] px-3.5 py-1.5 text-sm whitespace-nowrap text-ink ring-1 ring-white/10 ring-inset" aria-hidden={copy ? "true" : undefined}>{w}</li>{/each}
              {/each}
            </ul>
          {/each}
        </div>
      </article>
    </div>

    <!-- Details: copy left, two overlapping windows right. -->
    <div class="mt-[clamp(96px,12vw,180px)] grid items-center gap-14 lg:grid-cols-[minmax(0,4fr)_minmax(0,8fr)] lg:gap-16">
      <div class="reveal grid content-start gap-10">
        <div>
          <h3 class="h3">Details that go deep</h3>
          <p class="lede mt-3">Movie, series, season and episode pages with cast, chapters, similar items, extras and media info. People have pages too.</p>
        </div>
        <div class="rule pt-8">
          <h3 class="h3">Series, season by season</h3>
          <p class="lede mt-3">Season tabs with counts, an episode card grid, Next Up and autoplay into the next episode.</p>
        </div>
      </div>
      <div class="reveal relative pb-[22%] lg:-mr-[clamp(0px,4vw,64px)]">
        <div class="depth-back"><Shot name="movie-detail" sizes="(min-width: 1024px) 720px, 84vw" class="w-[84%] ml-auto lift" /></div>
        <div class="depth-front absolute bottom-0 left-0 w-[66%]">
          <Shot name="series-episodes" sizes="(min-width: 1024px) 580px, 66vw" class="lift shadow-[0_50px_100px_-30px_rgb(0_0_0/0.95)]" />
        </div>
      </div>
    </div>
  </div>

  <!-- Playback: the theatre. Wider than the text column, grows to full size as it scrolls in. -->
  <div class="relative isolate mt-[clamp(110px,14vw,210px)]">
    <div class="pool left-1/2 top-[30%] h-[60%] w-[70%] -translate-x-1/2 bg-[rgb(40_150_150/0.12)]" aria-hidden="true"></div>
    <div class="wrap">
      <div class="reveal max-w-[44rem]">
        <h2 class="h2">Playback inside the window.</h2>
        <p class="lede mt-5">libmpv renders with hardware decoding into a Wayland subsurface beneath a transparent GPUI window. The video sits in the app and the controls are drawn over the picture.</p>
      </div>
    </div>
    <div class="mx-auto mt-[clamp(40px,5vw,72px)] w-[min(1440px,100%-24px)]">
      <div class="widen">
        <Shot name="playback" sizes="(min-width: 1460px) 1440px, 100vw" class="rounded-[1.75rem] [&_img]:rounded-[1.4rem] [&::after]:rounded-[1.4rem]" />
      </div>
    </div>
    <div class="wrap mt-12">
      <ul class="tick stagger grid gap-x-12 gap-y-3 sm:grid-cols-2 lg:grid-cols-3">
        {#each player as p}<li>{p}</li>{/each}
      </ul>
    </div>
  </div>

  <!-- Admin: window left, copy right. -->
  <div class="wrap mt-[clamp(110px,14vw,210px)] grid items-center gap-12 lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)] lg:gap-16">
    <div class="swing lg:-ml-[clamp(0px,4vw,64px)]">
      <Shot name="admin-crop" sizes="(min-width: 1024px) 720px, 100vw" class="lift" />
    </div>
    <div class="reveal">
      <h2 class="h2">The admin dashboard is in the same app.</h2>
      <p class="lede mt-5">Administrator accounts get server info, activity, live sessions, devices, scheduled tasks, libraries, users, plugins, API keys and logs. One app, no second binary.</p>
      <p class="lede mt-4 text-ink">Every change asks first, and none are possible until you allow writes.</p>
    </div>
  </div>
</section>
