<script>
  import { RELEASED, REPO, UI, shot } from "#lib/site.js";
  import Next from "./Next.svelte";
  const s = shot("playback");
</script>

<!-- The opening: black letterbox bars, and between them the app playing a film. -->
<section class="grid min-h-[100svh] grid-rows-[auto_minmax(52svh,1fr)_auto] bg-black">
  <div class="h-[clamp(64px,11vh,104px)]"></div>
  <div class="screen relative overflow-hidden">
    <img class="absolute inset-0 size-full object-cover object-[50%_78%]" src={s.src} srcset={s.srcset} sizes="100vw" alt={s.alt} width={s.width} height={s.height} fetchpriority="high" loading="eager" decoding="async">
    <div class="absolute inset-0 bg-[radial-gradient(ellipse_70%_60%_at_50%_45%,rgb(4_4_5/0.15),rgb(4_4_5/0.75))]" aria-hidden="true"></div>
    <div class="relative grid h-full place-items-center px-5 py-16 text-center">
      <div>
        <h1 class="title-in m-0 mr-[-0.32em] text-[clamp(2.1rem,1rem+6.2vw,7.5rem)] font-medium leading-none tracking-[0.32em] text-ink">NOCTURNE<span class="sr-only">: a native cinematic media center for Linux</span></h1>
        <p class="fade-in m-0 mt-6 mr-[-0.42em] text-[clamp(0.72rem,0.64rem+0.35vw,0.95rem)] font-medium tracking-[0.42em] text-ink">YOUR MEDIA. YOUR NIGHT.</p>
      </div>
    </div>
  </div>
  <div class="wrap fade-in flex flex-wrap items-center justify-between gap-x-10 gap-y-5 py-[clamp(20px,4vh,40px)]">
    <p class="m-0 max-w-[50ch] text-ink-2">A native media center for Linux. Local folders and Jellyfin servers in one cinematic library, read-only until you allow it. <Next /></p>
    <div class="flex flex-wrap gap-3">
      {#if RELEASED}
        <a class="btn" href="/repo">Download</a>
        <a class="btn btn-ghost" href={REPO} rel="noopener">Source</a>
      {:else}
        <a class="btn" href="#tour">Watch the tour</a>
      {/if}
    </div>
  </div>
</section>

<!-- Opening credits: what it plays, what it is made of, one card at a time. -->
<section class="py-[clamp(110px,16vw,240px)] text-center" aria-label="Credits">
  <ul class="stagger m-0 grid list-none gap-[clamp(48px,7vw,88px)] p-0">
    <li><span class="role">Featuring</span><span class="name">Your folders</span><Next class="mt-4 ml-0" /></li>
    <li><span class="role">And</span><span class="name">Jellyfin servers</span></li>
    <li><span class="role">Written in</span><span class="name">Rust</span></li>
    <li><span class="role">Drawn by</span><span class="name">{UI[0].split(",")[0]}</span></li>
    <li><span class="role">Played by</span><span class="name">libmpv</span></li>
    <li><span class="role">Made for</span><span class="name">Linux</span></li>
  </ul>
</section>
