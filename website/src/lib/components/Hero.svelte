<script>
  import { RELEASED, REPO, RELEASES } from "#lib/site.js";
  import Shot from "./Shot.svelte";
</script>

<section class="relative isolate -mt-[68px] overflow-hidden pt-[calc(68px+clamp(48px,8vw,96px))] pb-[clamp(72px,9vw,140px)]">
  <div class="drift pointer-events-none absolute -inset-x-[15%] -top-[20%] -z-10 h-[110%] bg-[radial-gradient(ellipse_42%_46%_at_74%_30%,rgb(75_85_224/0.42),transparent_70%),radial-gradient(ellipse_30%_30%_at_12%_8%,rgb(76_141_255/0.16),transparent_70%)] [mask-image:linear-gradient(to_bottom,black_55%,transparent)]" aria-hidden="true"></div>

  <div class="wrap grid items-center gap-y-14 xl:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] xl:gap-x-12">
    <div class="relative z-10">
      <p class="label rise">YOUR MEDIA. YOUR NIGHT.</p>
      <h1 class="display rise d1 mt-6 max-w-[15ch]">A native cinematic media experience for Linux.</h1>
      <p class="lede rise d2 mt-7 max-w-[44ch]">An unofficial Jellyfin client written in Rust. Video plays inside the window, and nothing on your server changes until you allow it.</p>
      <div class="rise d3 mt-10 flex flex-wrap items-center gap-3">
        {#if RELEASED}
          <a class="btn" href={RELEASES}>Download <i aria-hidden="true">&darr;</i></a>
          <a class="btn btn-ghost" href={REPO} rel="noopener">Source code</a>
        {:else}
          <a class="btn" href="#tour">Take the tour <i aria-hidden="true">&rarr;</i></a>
          <a class="btn btn-ghost" href="#safety">Read-only by default</a>
        {/if}
      </div>
    </div>

    <div class="stage relative xl:-mr-[clamp(0px,12vw,260px)] max-xl:mx-auto max-xl:w-[min(100%,920px)]">
      <div class="stage-tilt rise relative">
        <Shot name="home" eager sizes="(min-width: 1280px) 860px, (min-width: 960px) 920px, 100vw" />
        <div class="float absolute -bottom-[14%] -left-[6%] w-[54%] min-w-[170px] max-xl:-bottom-[18%] max-xl:left-auto max-xl:-right-1">
          <Shot name="playback" eager sizes="(min-width: 1280px) 470px, 54vw" class="shadow-[0_40px_90px_-20px_rgb(0_0_0/0.95)]" />
        </div>
      </div>
    </div>
  </div>
</section>
