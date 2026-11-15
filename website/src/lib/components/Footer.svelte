<script>
  import { RELEASED, REPO, RELEASES } from "#lib/site.js";
  const link = "text-ink-2 no-underline hover:text-white";
</script>

<footer class="relative isolate overflow-hidden rule pt-[clamp(56px,7vw,96px)]">
  <div class="pool bottom-[-40%] left-1/2 h-[80%] w-[80%] -translate-x-1/2 bg-indigo/15" aria-hidden="true"></div>
  <div class="wrap">
    <div class="grid gap-10 md:grid-cols-[minmax(0,6fr)_minmax(0,3fr)_minmax(0,3fr)]">
      <div class="grid content-start gap-4">
        <a class="inline-flex items-center gap-2.5 text-[0.86rem] font-semibold tracking-[0.24em] text-ink no-underline hover:text-white" href="/" aria-label="NOCTURNE home"><img src="/icon.svg" width="24" height="24" alt=""><span>NOCTURNE</span></a>
        <p class="m-0 max-w-[36ch] text-ink-2">Your media. Your night. A native cinematic media experience for Linux.</p>
      </div>
      <div>
        <p class="m-0 mb-4 text-sm font-semibold text-ink">Built with</p>
        <ul class="m-0 grid list-none gap-2.5 p-0 text-[0.95rem]">
          <li><a class={link} href="https://jellyfin.org" rel="noopener">Jellyfin</a></li>
          <li><a class={link} href="https://www.gpui.rs" rel="noopener">GPUI</a> <span class="text-ink-3">from</span> <a class={link} href="https://github.com/zed-industries/zed" rel="noopener">Zed</a></li>
          <li><a class={link} href="https://mpv.io" rel="noopener">mpv</a></li>
          <li><a class={link} href="https://www.rust-lang.org" rel="noopener">Rust</a></li>
        </ul>
      </div>
      <div>
        <p class="m-0 mb-4 text-sm font-semibold text-ink">Project</p>
        <ul class="m-0 grid list-none gap-2.5 p-0 text-[0.95rem]">
          {#if RELEASED}
            <li><a class={link} href={RELEASES} rel="noopener">Download</a></li>
            <li><a class={link} href={REPO} rel="noopener">Source on GitHub</a></li>
            <li><a class={link} href="{REPO}/tree/main/docs" rel="noopener">Documentation</a></li>
          {/if}
          <li><a class={link} href="/#faq">FAQ</a></li>
          <li><a class={link} href="/#limits">Limitations</a></li>
          <li><a class={link} href="/privacy">Privacy</a></li>
        </ul>
      </div>
    </div>

    <div class="mt-14 grid gap-2 rule pt-8 text-sm text-ink-3 [&>p]:max-w-[86ch]">
      <p class="m-0">NOCTURNE is an independent, unofficial project, not affiliated with or endorsed by Jellyfin. Other names belong to their owners. Screenshots show a generated demo library.</p>
      <p class="m-0">Free software under the GNU General Public License, version 3 or (at your option) any later version (GPL-3.0-or-later), with no warranty. Set in Geist, under the SIL Open Font License.</p>
      <p class="m-0">Copyright (C) 2026 ifsvivek</p>
    </div>
  </div>
  <p class="pointer-events-none m-0 mt-10 select-none text-center text-[clamp(4rem,19vw,20rem)] font-bold leading-[0.72] tracking-[-0.05em] text-white/[0.035]" aria-hidden="true">NOCTURNE</p>
</footer>
