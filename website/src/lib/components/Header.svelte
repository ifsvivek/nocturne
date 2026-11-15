<script>
  import { RELEASED, RELEASES } from "#lib/site.js";
  const links = [["Features", "/#features"], ["How it works", "/#how"], ["Safety", "/#safety"], ["Install", "/#install"]];
</script>

<header class="sticky top-0 z-50 border-b border-line bg-frame/80 backdrop-blur-md">
  <div class="wrap flex h-14 items-center gap-5">
    <a class="inline-flex items-center gap-2.5 font-mono text-[0.86rem] font-semibold tracking-[0.22em] text-ink no-underline" href="/" aria-label="NOCTURNE home">
      <img src="/icon.svg" width="22" height="22" alt=""><span>NOCTURNE</span>
    </a>
    <nav class="ml-auto hidden gap-1 md:flex" aria-label="Primary">
      {#each links as [name, href]}
        <a class="rounded-lg px-3 py-1.5 text-sm text-ink-2 no-underline hover:bg-white/5 hover:text-ink" {href}>{name}</a>
      {/each}
    </nav>
    <div class="ml-auto flex-none md:ml-0">
      {#if RELEASED}<a class="btn btn-sm" href={RELEASES}>Download</a>{:else}<span class="chip"><i></i>Coming soon</span>{/if}
    </div>
  </div>
</header>
