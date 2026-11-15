<script>
  import { shot } from "#lib/site.js";
  const video = shot("video-crop");
  const controls = shot("controls-crop");
  const steps = [
    ["Your server", "NOCTURNE talks only to the Jellyfin server you configure. Browsing and playback only read from it."],
    ["Controls, drawn by GPUI", "The whole interface is a native GPUI app rendered on the GPU: libraries, details, the player controls and the admin dashboard."],
    ["A transparent window", "The window is transparent and paints nothing where the video sits, so the picture underneath shows through."],
    ["Video, by libmpv", "libmpv decodes and renders through OpenGL and EGL into a Wayland subsurface below the window. On X11, video plays in an external mpv window instead."],
  ];
</script>

<section id="how" class="section relative isolate overflow-hidden rule" aria-labelledby="how-h">
  <div class="pool right-[-10%] top-[20%] size-[560px] bg-indigo/20" aria-hidden="true"></div>
  <div class="wrap grid items-center gap-16 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
    <div>
      <h2 id="how-h" class="h2 reveal">How it works</h2>
      <ol class="m-0 mt-10 grid list-none gap-0 p-0">
        {#each steps as [name, text], i}
          <li class="reveal grid gap-1.5 border-l py-4 pl-6 {i === 0 ? 'border-accent' : 'border-line-strong'}">
            <strong class="text-[1.06rem] font-semibold text-ink">{name}</strong>
            <span class="max-w-[46ch] text-ink-2">{text}</span>
          </li>
        {/each}
      </ol>
    </div>

    <!-- The window taking itself apart: flat (what you see), then tilted and split into its three real layers. -->
    <figure class="explode reveal m-0 grid aspect-[1/0.82] place-items-center">
      <div class="explode-stack relative aspect-[24/13] w-[66%] -translate-x-[8%] max-sm:w-[60%] max-sm:-translate-x-[14%]">
        <div class="layer-1 absolute inset-0">
          <img class="size-full rounded-xl object-cover shadow-[0_60px_80px_-30px_rgb(0_0_0/0.9)] ring-1 ring-white/10" src={video.src} srcset={video.srcset} sizes="(min-width: 1024px) 480px, 60vw" alt={video.alt} width={video.width} height={video.height} loading="lazy" decoding="async">
          <span class="tag">Video &middot; libmpv</span>
        </div>
        <div class="layer-2 absolute inset-0 rounded-xl bg-[linear-gradient(140deg,rgb(255_255_255/0.07),rgb(255_255_255/0.01))] ring-1 ring-white/30">
          <span class="tag">Transparent window</span>
        </div>
        <div class="layer-3 absolute inset-0 flex items-end rounded-xl ring-1 ring-accent/40">
          <img class="w-full rounded-b-xl mix-blend-screen" src={controls.src} srcset={controls.srcset} sizes="(min-width: 1024px) 480px, 60vw" alt={controls.alt} width={controls.width} height={controls.height} loading="lazy" decoding="async">
        </div>
        <!-- Own element: the blend mode above flattens its layer, which would squash a label inside it. -->
        <div class="layer-3 pointer-events-none absolute inset-0"><span class="tag tag-accent">Controls &middot; GPUI</span></div>
      </div>
      <figcaption class="sr-only">The video path as three layers: libmpv video at the bottom, the transparent GPUI window above it, the GPUI controls on top. The view loops from the assembled window to the separated layers.</figcaption>
    </figure>
  </div>
</section>
