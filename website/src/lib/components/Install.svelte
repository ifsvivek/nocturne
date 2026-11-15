<script>
  import { RELEASED, REPO, RELEASES, VERSION } from "#lib/site.js";
  const pre = "m-0 mt-5 overflow-x-auto rounded-xl bg-frame px-4 py-3.5 text-[0.82rem] leading-[1.75] text-ink ring-1 ring-line ring-inset";
  const rows = [
    ["Platform", "Linux, x86_64. Nothing else is built or supported.", "Supported"],
    ["libmpv", "The mpv package on Arch. Version 0.41 tested.", "Verified"],
    ["Display", "Wayland for in-window video. On X11 the UI runs and video plays in an external mpv window (NOCTURNE_VIDEO=external forces that anywhere).", "Wayland and X11 fallback verified"],
    ["Graphics", "A Vulkan driver for the UI, OpenGL/EGL for video.", "Other GPU setups not yet tested"],
    ["Keyring", "A Secret Service provider (gnome-keyring, KWallet or KeePassXC) to remember your sign-in.", "Required to stay signed in"],
    ["Jellyfin", "A Jellyfin server, developed against 12.x.", "Older servers untested"],
    ["Distribution", "Packages are tested on Arch Linux only. The tarball binary is built on a rolling distribution, so it needs a glibc at least as new.", "Debian, Ubuntu, Fedora, openSUSE untested"],
  ];
  const docs = [["Features", "FEATURES"], ["Architecture", "ARCHITECTURE"], ["Writes", "WRITES"], ["Packaging", "PACKAGING"], ["QA report", "QA_REPORT"], ["Roadmap", "ROADMAP"]];
</script>

<section id="install" class="section relative isolate rule" aria-labelledby="install-h">
  <div class="wrap">
    <div class="reveal max-w-[46rem]">
      <h2 id="install-h" class="h2">Requirements and install</h2>
      {#if RELEASED}
        <p class="lede mt-5">Version {VERSION}. Pick a package, or build it yourself.</p>
      {:else}
        <p class="lede mt-5">Coming soon. Install commands and download links appear here with the first release.</p>
      {/if}
    </div>

    <!-- Three ways in, one panel split by hairlines rather than three boxed cards. -->
    <div class="reveal mt-[clamp(40px,5vw,64px)] grid overflow-hidden rounded-[1.25rem] bg-surface ring-1 ring-line ring-inset lg:grid-cols-3">
      <div class="p-[clamp(24px,3vw,36px)]">
        <h3 class="h3">Arch Linux</h3>
        <p class="muted mt-2">A PKGBUILD that builds from the source archive of the release tag.</p>
        {#if RELEASED}<pre class={pre}><code>git clone {REPO}.git
cd nocturne/packaging
makepkg -si</code></pre>{/if}
      </div>
      <div class="border-line p-[clamp(24px,3vw,36px)] max-lg:border-t lg:border-l">
        <h3 class="h3">Tarball</h3>
        <p class="muted mt-2">A generic x86_64 build with an install script. Installs to <code>/usr/local</code>, and the same script uninstalls.</p>
        {#if RELEASED}<pre class={pre}><code>tar xf nocturne-{VERSION}-x86_64-linux.tar.gz
cd nocturne-{VERSION}-x86_64-linux
sudo ./install.sh</code></pre>
          <p class="mt-4 text-sm text-ink-2">Get it from <a href={RELEASES} rel="noopener">the latest release</a>.</p>{/if}
      </div>
      <div class="border-line p-[clamp(24px,3vw,36px)] max-lg:border-t lg:border-l">
        <h3 class="h3">From source</h3>
        <p class="muted mt-2">Stable Rust 1.89 or newer and the libmpv development files. A clean build takes several minutes.</p>
        {#if RELEASED}<pre class={pre}><code>git clone {REPO}.git
cd nocturne
cargo build --release --locked
./target/release/nocturne</code></pre>{/if}
      </div>
    </div>

    <div class="reveal mt-[clamp(56px,7vw,96px)]">
      <h3 class="h3">What it needs</h3>
      <table class="mt-6 w-full border-collapse text-left text-[0.95rem]">
        <caption class="sr-only">Requirements and compatibility</caption>
        <thead class="max-md:sr-only">
          <tr class="text-[0.75rem] uppercase tracking-[0.16em] text-ink-3">
            <th scope="col" class="pb-3 pr-6 font-medium">Needs</th><th scope="col" class="pb-3 pr-6 font-medium">Detail</th><th scope="col" class="pb-3 font-medium">Status</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as [k, d, st]}
            <tr class="border-t border-line max-md:grid max-md:gap-1 max-md:py-5">
              <th scope="row" class="whitespace-nowrap font-semibold text-ink md:w-40 md:py-5 md:pr-6 md:align-top">{k}</th>
              <td class="text-ink-2 md:py-5 md:pr-6 md:align-top">{d}</td>
              <td class="text-ink md:w-64 md:py-5 md:align-top">{st}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if RELEASED}
      <div class="reveal mt-12 flex flex-wrap items-center gap-x-6 gap-y-3">
        <span class="text-sm font-semibold text-ink">Documentation</span>
        {#each docs as [name, f]}<a class="text-sm no-underline" href="{REPO}/blob/main/docs/{f}.md" rel="noopener">{name} &rarr;</a>{/each}
      </div>
    {/if}
  </div>
</section>
