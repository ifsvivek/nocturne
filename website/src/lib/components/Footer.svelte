<script>
  import { RELEASED, REPO, RELEASES, UI } from "#lib/site.js";
  // End credits: role on the left of the gutter, name on the right.
  const credits = [
    ["Media server", "Jellyfin", "https://jellyfin.org"],
    ["Interface", ...UI],
    ["Playback", "mpv and libmpv", "https://mpv.io"],
    ["Language", "Rust", "https://www.rust-lang.org"],
    ["Typeface", "Geist", ""],
    ["License", "GPL-3.0-or-later", ""],
  ];
  const link = "text-ink no-underline hover:text-accent-ink";
</script>

<footer class="rule bg-black pt-[clamp(96px,12vw,180px)] pb-14">
  <div class="wrap text-center">
    <p class="role">End credits</p>
    <dl class="stagger mx-auto mt-12 grid max-w-[640px] gap-y-5">
      {#each credits as [role, name, href]}
        <div class="grid grid-cols-2 gap-6">
          <dt class="text-right text-[0.72rem] leading-7 font-medium tracking-[0.3em] text-ink-3 uppercase">{role}</dt>
          <dd class="m-0 text-left text-[1.02rem] leading-7 tracking-[0.06em]">{#if href}<a class={link} {href} rel="noopener">{name}</a>{:else}{name}{/if}</dd>
        </div>
      {/each}
    </dl>

    <p class="m-0 mt-[clamp(72px,9vw,120px)] text-[clamp(1.2rem,0.9rem+1.5vw,2rem)] font-medium tracking-[0.32em] mr-[-0.32em]">NOCTURNE</p>
    <p class="m-0 mt-3 text-[0.72rem] font-medium tracking-[0.42em] mr-[-0.42em] text-ink-2">YOUR MEDIA. YOUR NIGHT.</p>

    <nav class="mt-12 flex flex-wrap justify-center gap-x-8 gap-y-3 text-[0.72rem] font-medium tracking-[0.26em] uppercase" aria-label="Footer">
      {#if RELEASED}
        <a class="text-ink-2 no-underline hover:text-white" href={RELEASES} rel="noopener">Download</a>
        <a class="text-ink-2 no-underline hover:text-white" href="/repo">Package repository</a>
        <a class="text-ink-2 no-underline hover:text-white" href={REPO} rel="noopener">Source</a>
        <a class="text-ink-2 no-underline hover:text-white" href="{REPO}/tree/main/docs" rel="noopener">Documentation</a>
      {/if}
      <a class="text-ink-2 no-underline hover:text-white" href="/#faq">FAQ</a>
      <a class="text-ink-2 no-underline hover:text-white" href="/#limits">Limitations</a>
      <a class="text-ink-2 no-underline hover:text-white" href="/privacy">Privacy</a>
    </nav>

    <div class="mx-auto mt-12 grid max-w-[72ch] gap-2 text-[0.8rem] text-ink-3">
      <p class="m-0">NOCTURNE is an independent, unofficial project, not affiliated with or endorsed by Jellyfin. Other names belong to their owners. Screenshots show a generated demo library.</p>
      <p class="m-0">Free software under the GNU General Public License, version 3 or (at your option) any later version (GPL-3.0-or-later), with no warranty. Geist is under the SIL Open Font License.</p>
      <p class="m-0">Copyright (C) 2026 ifsvivek</p>
    </div>
  </div>
</footer>
