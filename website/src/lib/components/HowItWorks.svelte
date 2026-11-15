<script>
  import Section from "./Section.svelte";
  const steps = [
    ["SOURCE", "Jellyfin server", "Your library, your users. NOCTURNE talks only to the server you configure.", false],
    ["CLIENT", "NOCTURNE", "A native GPUI app: browse, search, details, the player UI, the admin dashboard.", true],
    ["OUTPUT", "libmpv", "Decodes and renders into a surface under the window; the controls are drawn on top.", false],
  ];
</script>

<Section id="how" no="01" tag="PIPELINE" title="How it works">
  <ol class="m-0 grid list-none overflow-hidden rounded-2xl border border-line bg-surface p-0 md:grid-cols-3">
    {#each steps as [tag, name, text, hl], i}
      <li class="grid content-start gap-1.5 p-6 {i ? 'border-t border-line md:border-l md:border-t-0' : ''} {hl ? 'bg-gradient-to-b from-accent/10 to-transparent shadow-[inset_0_1px_0_rgb(76_141_255/0.4)]' : ''}">
        <span class="label">{tag}</span>
        <strong class="text-xl font-medium">{name}</strong>
        <span class="text-ink-2">{text}</span>
      </li>
    {/each}
  </ol>
  <p class="label mt-4 text-center md:text-left" aria-hidden="true">Jellyfin server &rarr; NOCTURNE &rarr; libmpv</p>
</Section>
