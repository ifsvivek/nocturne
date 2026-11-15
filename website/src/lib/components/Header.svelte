<script>
  import { RELEASED, RELEASES } from "#lib/site.js";
  const links = [["Tour", "/#tour"], ["How it works", "/#how"], ["Safety", "/#safety"], ["Install", "/#install"], ["FAQ", "/#faq"]];
</script>

<header class="sticky top-3 z-50 mt-3">
  <div class="wrap flex h-14 items-center gap-4 rounded-full bg-frame/70 pl-5 pr-2 ring-1 ring-white/10 ring-inset shadow-[inset_0_1px_0_rgb(255_255_255/0.06),0_20px_50px_-30px_rgb(0_0_0/0.9)] backdrop-blur-xl">
    <a class="inline-flex items-center gap-2.5 text-[0.86rem] font-semibold tracking-[0.24em] text-ink no-underline hover:text-white" href="/" aria-label="NOCTURNE home">
      <img src="/icon.svg" width="24" height="24" alt=""><span>NOCTURNE</span>
    </a>
    <nav class="ml-auto hidden gap-0.5 md:flex" aria-label="Primary">
      {#each links as [name, href]}
        <a class="rounded-full px-3.5 py-1.5 text-sm text-ink-2 no-underline transition-colors duration-200 hover:bg-white/[0.06] hover:text-ink" {href}>{name}</a>
      {/each}
    </nav>
    <div class="ml-auto flex-none md:ml-0">
      {#if RELEASED}<a class="btn btn-sm" href={RELEASES}>Download</a>{:else}<span class="chip min-h-9 text-[0.7rem]">Coming soon</span>{/if}
    </div>
  </div>
</header>
