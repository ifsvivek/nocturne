<script>
  import { RELEASED } from "#lib/site.js";
  const links = [["Scenes", "/#tour"], ["Behind the scenes", "/#how"], ["Read-only", "/#safety"], ["Install", "/#install"], ["FAQ", "/#faq"]];
</script>

<!-- Sits in the top letterbox bar of the opening; a plain bar on the other pages. -->
<header class="absolute inset-x-0 top-0 z-50">
  <div class="wrap flex h-[clamp(64px,11vh,104px)] items-center gap-8 text-[0.72rem] font-medium uppercase tracking-[0.26em]">
    <a class="inline-flex items-center gap-3 text-ink no-underline hover:text-white" href="/" aria-label="NOCTURNE home">
      <img src="/icon.svg" width="22" height="22" alt=""><span class="tracking-[0.32em]">NOCTURNE</span>
    </a>
    <nav class="ml-auto hidden gap-7 lg:flex" aria-label="Primary">
      {#each links as [name, href]}<a class="text-ink-2 no-underline hover:text-white" {href}>{name}</a>{/each}
    </nav>
    {#if RELEASED}<a class="ml-auto text-ink no-underline lg:ml-0" href="/repo">Download</a>{:else}<span class="ml-auto text-ink-3 lg:ml-0">Coming soon</span>{/if}
  </div>
</header>
