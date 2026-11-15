<script>
  import { RELEASED, REPO, RELEASES } from "#lib/site.js";
  import StatusChip from "./StatusChip.svelte";
  import BrandVisual from "./BrandVisual.svelte";
</script>

<section class="relative overflow-hidden pb-[clamp(40px,6vw,80px)] pt-[clamp(56px,10vw,120px)]">
  <div class="pointer-events-none absolute -inset-x-[10%] -top-1/5 h-[120%] bg-[radial-gradient(ellipse_50%_45%_at_70%_12%,rgb(75_85_224/0.34),transparent_70%),radial-gradient(ellipse_36%_30%_at_30%_0%,rgb(76_141_255/0.16),transparent_70%)]" aria-hidden="true">
  </div>
  <div class="wrap relative">
    <StatusChip class="mb-7" />
    <h1 class="m-0 -mr-[0.12em] bg-gradient-to-b from-white from-20% to-[#8f9cf0]/90 bg-clip-text text-[clamp(2.6rem,12.4vw,9.5rem)] font-light leading-[0.95] tracking-[0.12em] text-transparent">NOCTURNE</h1>
    <p class="mt-[22px] font-mono text-[clamp(0.8rem,1.6vw,1rem)] tracking-[0.36em] text-accent-ink">YOUR MEDIA. YOUR NIGHT.</p>
    <p class="mt-[22px] max-w-[56ch] text-[clamp(1.02rem,1.6vw,1.2rem)] text-ink-2">A native cinematic media experience for Linux. A fast Jellyfin desktop client with video that plays inside the window and controls that sit over it.</p>
    <div class="mt-9 flex flex-wrap gap-3">
      {#if RELEASED}
        <a class="btn" href={RELEASES}>Download</a>
        <a class="btn btn-ghost" href={REPO} rel="noopener">Source</a>
      {:else}
        <a class="btn" href="#features">Explore the features</a>
        <a class="btn btn-ghost" href="#how">How it works</a>
      {/if}
    </div>
  </div>
  <div class="wrap"><BrandVisual /></div>
</section>
