<script>
  import Section from "./Section.svelte";
  import { SCREENS } from "#lib/site.js";
</script>

<Section id="screens" no="04" tag="GALLERY" title="Screenshots">
  {#if SCREENS.length}
    <div class="grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(min(100%,340px),1fr))]">
      {#each SCREENS as s}
        <figure class="m-0 aspect-[16/10] overflow-hidden rounded-xl border border-line bg-surface">
          <img class="size-full object-cover" src={s.src} alt={s.alt} width={s.width} height={s.height} loading="lazy" decoding="async">
        </figure>
      {/each}
    </div>
  {:else}
    <div class="grid min-h-[260px] place-content-center gap-2 rounded-2xl border border-dashed border-line-strong bg-surface bg-[radial-gradient(ellipse_60%_80%_at_50%_0%,rgb(75_85_224/0.12),transparent_70%)] p-8 text-center">
      <span class="label">SCREENSHOTS</span>
      <p class="m-0 text-ink-2">Screenshots arrive with the first release.</p>
    </div>
  {/if}
</Section>
