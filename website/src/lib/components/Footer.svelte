<script>
  import { RELEASED, REPO, RELEASES } from "#lib/site.js";
</script>

<footer class="border-t border-line bg-gradient-to-b from-transparent to-indigo/5 pb-10 pt-14">
  <div class="wrap">
    <div class="grid gap-8 [grid-template-columns:repeat(auto-fit,minmax(180px,1fr))]">
      <div class="grid content-start gap-3">
        <a class="inline-flex items-center gap-2.5 font-mono text-[0.86rem] font-semibold tracking-[0.22em] text-ink no-underline" href="/" aria-label="NOCTURNE home"><img src="/icon.svg" width="22" height="22" alt=""><span>NOCTURNE</span></a>
        <p class="m-0 text-sm text-ink-2">Your media. Your night.<br>A native cinematic media experience for Linux.</p>
      </div>
      <div>
        <p class="label mb-3">Built with</p>
        <ul class="m-0 grid list-none gap-2 p-0 text-[0.92rem]">
          <li><a class="text-ink-2 no-underline hover:text-white" href="https://jellyfin.org" rel="noopener">Jellyfin</a></li>
          <li><a class="text-ink-2 no-underline hover:text-white" href="https://www.gpui.rs" rel="noopener">GPUI</a> <span class="text-ink-3">/</span> <a class="text-ink-2 no-underline hover:text-white" href="https://github.com/zed-industries/zed" rel="noopener">Zed</a></li>
          <li><a class="text-ink-2 no-underline hover:text-white" href="https://mpv.io" rel="noopener">mpv</a></li>
          <li><a class="text-ink-2 no-underline hover:text-white" href="https://www.rust-lang.org" rel="noopener">Rust</a></li>
        </ul>
      </div>
      <div>
        <p class="label mb-3">Project</p>
        <ul class="m-0 grid list-none gap-2 p-0 text-[0.92rem]">
          {#if RELEASED}
            <li><a class="text-ink-2 no-underline hover:text-white" href={REPO} rel="noopener">Source on GitHub</a></li>
            <li><a class="text-ink-2 no-underline hover:text-white" href={RELEASES} rel="noopener">Download</a></li>
          {/if}
          <li><a class="text-ink-2 no-underline hover:text-white" href="/privacy">Privacy</a></li>
          <li><a class="text-ink-2 no-underline hover:text-white" href="/#limits">Limitations</a></li>
        </ul>
      </div>
    </div>
    <div class="mt-10 grid max-w-[80ch] gap-1.5 border-t border-line pt-6 text-sm text-ink-2">
      <p class="m-0">NOCTURNE is an independent project, not affiliated with or endorsed by Jellyfin. Other names belong to their owners.</p>
      <p class="m-0">NOCTURNE is free software, licensed under the GNU General Public License, version 3 or (at your option) any later version (GPL-3.0-or-later). It comes with no warranty.</p>
      <p class="m-0">Copyright (C) 2026 ifsvivek</p>
    </div>
  </div>
</footer>
