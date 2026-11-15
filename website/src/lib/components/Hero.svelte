<script>
  import { RELEASED, REPO, RELEASES } from "#lib/site.js";
  import Shot from "./Shot.svelte";
</script>

<section class="relative isolate overflow-hidden -mt-[68px] pb-[clamp(56px,8vw,112px)] pt-[calc(68px+clamp(40px,7vw,88px))]">
  <div class="drift pointer-events-none absolute -inset-x-[10%] -top-[10%] -z-10 h-[90%] bg-[radial-gradient(ellipse_48%_50%_at_72%_18%,rgb(75_85_224/0.4),transparent_70%),radial-gradient(ellipse_34%_34%_at_22%_6%,rgb(76_141_255/0.2),transparent_70%)]" aria-hidden="true"></div>
  <div class="wrap">
    <h1 class="rise m-0 -ml-[0.04em] bg-gradient-to-b from-white from-30% to-[#8f9cf0]/80 bg-clip-text pb-2 text-[clamp(2.5rem,11.6vw,8.75rem)] font-light leading-[0.95] tracking-[0.12em] text-transparent">NOCTURNE</h1>
    <p class="rise d1 mt-5 font-mono text-[clamp(0.78rem,1.5vw,1rem)] tracking-[0.34em] text-accent-ink">YOUR MEDIA. YOUR NIGHT.</p>
    <p class="rise d2 mt-6 max-w-[48ch] text-pretty text-[clamp(1.05rem,1.7vw,1.3rem)] leading-relaxed text-ink-2">A native cinematic media experience for Linux. A fast Jellyfin client with video that plays inside the window.</p>
    <div class="rise d3 mt-9 flex flex-wrap gap-3">
      {#if RELEASED}
        <a class="btn" href={RELEASES}>Download</a>
        <a class="btn btn-ghost" href={REPO} rel="noopener">Source</a>
      {:else}
        <a class="btn" href="#tour">See the app</a>
        <a class="btn btn-ghost" href="#safety">Read-only by default</a>
      {/if}
    </div>
  </div>

  <div class="wrap rise d4 relative mt-[clamp(40px,6vw,80px)]">
    <Shot name="home" eager hover={false} sizes="(min-width: 1220px) 1180px, 100vw" class="shadow-[0_60px_140px_-50px_rgb(75_85_224/0.7)]" />
    <div class="par absolute -bottom-[clamp(28px,5vw,64px)] right-[clamp(8px,3vw,40px)] w-[48%] min-w-[180px] max-w-[520px] max-md:w-[58%]">
      <Shot name="playback" eager hover={false} sizes="(min-width: 1220px) 520px, 58vw" class="shadow-[0_30px_80px_-20px_rgb(0_0_0/0.9)]" />
    </div>
  </div>
</section>
