<script>
  const steps = [
    ["Your Jellyfin server", "Your library, your users. NOCTURNE talks only to the server you configure."],
    ["NOCTURNE", "A native GPUI app: browse, search, details, the player UI and the admin dashboard."],
    ["libmpv", "Decodes and renders into a surface under the window. The controls are drawn on top."],
  ];
</script>

<section id="how" class="border-t border-line py-[clamp(56px,8vw,112px)] [content-visibility:auto] [contain-intrinsic-size:auto_420px]" aria-labelledby="how-h">
  <div class="wrap">
    <h2 id="how-h" class="h2 reveal max-w-[22ch]">How it works</h2>
    <ol class="m-0 mt-12 grid list-none gap-10 p-0 md:grid-cols-3 md:gap-0">
      {#each steps as [name, text], i}
        <li class="reveal relative grid content-start gap-3 md:pr-10">
          <span class="relative block h-px w-full bg-gradient-to-r from-accent/70 via-line-strong to-transparent {i === 2 ? 'md:to-transparent' : ''}" aria-hidden="true"></span>
          <strong class="text-xl font-medium tracking-tight {i === 1 ? 'text-accent-ink' : ''}">{name}</strong>
          <span class="max-w-[34ch] text-ink-2">{text}</span>
        </li>
      {/each}
    </ol>
  </div>
</section>
