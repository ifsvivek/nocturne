<script>
  import Section from "./Section.svelte";
  const cell = "grid content-start gap-2.5 bg-surface p-[clamp(22px,3vw,32px)] transition-colors duration-[140ms] hover:bg-raised";
</script>

<Section id="features" no="02" tag="FEATURES" title="Built for the dark room" blurb="Concrete, not aspirational. Everything here exists in the code today; the limitations section says what is still rough.">
  <div class="grid gap-px overflow-hidden rounded-2xl border border-line bg-line md:grid-cols-2">
    <article class="{cell} md:col-span-2 bg-[radial-gradient(ellipse_50%_90%_at_100%_0%,rgb(75_85_224/0.16),transparent_70%)]">
      <span class="label">VIDEO</span>
      <h3 class="m-0 text-xl font-medium leading-snug">In-window GPU video</h3>
      <p class="m-0 max-w-[70ch] text-ink-2">libmpv renders through its OpenGL API into a Wayland subsurface beneath a transparent GPUI window, so playback sits inside the app with the controls drawn over the picture. Hardware decoding is on (NVDEC verified, no CPU frame copies); the prototype held 4K60 HEVC 10-bit with no dropped frames.</p>
    </article>
    <article class={cell}>
      <span class="label">PLAYER</span>
      <h3 class="m-0 text-xl font-medium leading-snug">A real player</h3>
      <ul class="tick">
        <li>Audio and subtitle menus, subtitle presets</li>
        <li>Speed, aspect, audio and subtitle delay</li>
        <li>Chapters and trickplay thumbnails on the scrub bar</li>
        <li>Skip intro and credits, next-episode autoplay, queue</li>
        <li>Quality choice with transcode fallback</li>
        <li>MPRIS media keys, idle inhibit, mini-player, stats overlay</li>
      </ul>
    </article>
    <article class={cell}>
      <span class="label">LIBRARY</span>
      <h3 class="m-0 text-xl font-medium leading-snug">Browse like you mean it</h3>
      <ul class="tick">
        <li>Sort, filters and an A-Z rail, virtualised grids</li>
        <li>Global search on <kbd>Ctrl</kbd>+<kbd>K</kbd></li>
        <li>Genres, studios, years, tags, collections, favourites</li>
        <li>Detail pages, cast carousels, person pages, episode card grid</li>
        <li>Blurhash placeholders and a disk image cache</li>
      </ul>
    </article>
    <article class={cell}>
      <span class="label">ACCOUNTS</span>
      <h3 class="m-0 text-xl font-medium leading-snug">Sign in your way</h3>
      <p class="m-0 text-ink-2">Username and password, or Quick Connect from another device. Tokens are stored in the system keyring (Secret Service: GNOME Keyring, KWallet, KeePassXC); there is no plaintext fallback on disk.</p>
    </article>
    <article class={cell}>
      <span class="label">ADMIN</span>
      <h3 class="m-0 text-xl font-medium leading-snug">Dashboard in the same app</h3>
      <p class="m-0 text-ink-2">Administrators get an extra section: server info, activity, live sessions, devices, scheduled tasks, libraries, users, plugins, API keys and logs. One app, no second binary. Every destructive action asks first.</p>
    </article>
  </div>
</Section>
