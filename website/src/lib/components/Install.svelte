<script>
  import { RELEASED, REPO, RELEASES, VERSION } from "#lib/site.js";
  const pre = "m-0 mt-5 overflow-x-auto rounded-sm bg-surface px-4 py-3.5 text-[0.8rem] leading-[1.75] text-ink ring-1 ring-line ring-inset";
  const specs = [
    ["Platform", "Linux, x86_64. Nothing else is built or supported.", "Supported"],
    ["libmpv", "The mpv package on Arch. Version 0.41 tested.", "Verified"],
    ["Display", "Wayland for in-window video. On X11 the UI runs and video plays in an external mpv window (NOCTURNE_VIDEO=external forces that anywhere).", "Wayland and X11 fallback verified"],
    ["Graphics", "A Vulkan driver for the UI, OpenGL/EGL for video.", "Other GPU setups not yet tested"],
    ["Keyring", "A Secret Service provider (gnome-keyring, KWallet or KeePassXC) to remember your sign-in.", "Required to stay signed in"],
    ["Server", "A Jellyfin server, developed against 12.x.", "Older servers untested"],
    ["Distribution", "Packages are tested on Arch Linux only. The tarball binary is built on a rolling distribution, so it needs a glibc at least as new.", "Debian, Ubuntu, Fedora, openSUSE untested"],
  ];
  const docs = [["Features", "FEATURES"], ["Architecture", "ARCHITECTURE"], ["Writes", "WRITES"], ["Packaging", "PACKAGING"], ["QA report", "QA_REPORT"], ["Roadmap", "ROADMAP"]];
</script>

<section id="install" class="section rule" aria-labelledby="install-h">
  <div class="wrap">
    <header class="reveal text-center">
      <span class="role">{RELEASED ? `Version ${VERSION}` : "Install"}</span>
      <h2 id="install-h" class="scene-title">{RELEASED ? "Now showing on Linux" : "Coming soon to Linux"}</h2>
      {#if !RELEASED}<p class="lede mx-auto mt-6">Install commands and download links appear here with the first release.</p>{/if}
    </header>

    <div class="stagger mt-[clamp(56px,7vw,96px)] grid gap-y-10 lg:grid-cols-3 lg:divide-x lg:divide-line">
      <div class="lg:pr-10">
        <h3 class="role text-ink">Arch Linux</h3>
        <p class="m-0 mt-3 text-ink-2">A PKGBUILD that builds from the source archive of the release tag.</p>
        {#if RELEASED}<pre class={pre}><code>git clone {REPO}.git
cd nocturne/packaging
makepkg -si</code></pre>{/if}
      </div>
      <div class="lg:px-10">
        <h3 class="role text-ink">Tarball</h3>
        <p class="m-0 mt-3 text-ink-2">A generic x86_64 build with an install script. Installs to <code>/usr/local</code>, and the same script uninstalls.</p>
        {#if RELEASED}<pre class={pre}><code>tar xf nocturne-{VERSION}-x86_64-linux.tar.gz
cd nocturne-{VERSION}-x86_64-linux
sudo ./install.sh</code></pre>
          <p class="mt-4 text-sm text-ink-2">Get it from <a href={RELEASES} rel="noopener">the latest release</a>.</p>{/if}
      </div>
      <div class="lg:pl-10">
        <h3 class="role text-ink">From source</h3>
        <p class="m-0 mt-3 text-ink-2">Stable Rust 1.89 or newer and the libmpv development files. A clean build takes several minutes.</p>
        {#if RELEASED}<pre class={pre}><code>git clone {REPO}.git
cd nocturne
cargo build --release --locked
./target/release/nocturne</code></pre>{/if}
      </div>
    </div>

    <div class="reveal mt-[clamp(72px,9vw,128px)]">
      <h3 class="role text-center">Technical specifications</h3>
      <table class="mt-8 w-full border-collapse text-left text-[0.95rem]">
        <caption class="sr-only">Requirements and compatibility</caption>
        <thead class="sr-only"><tr><th scope="col">Needs</th><th scope="col">Detail</th><th scope="col">Status</th></tr></thead>
        <tbody>
          {#each specs as [k, d, st]}
            <tr class="border-t border-line max-md:grid max-md:gap-1 max-md:py-5">
              <th scope="row" class="text-[0.72rem] font-medium tracking-[0.26em] text-ink-3 uppercase md:w-44 md:py-5 md:pr-6 md:align-top">{k}</th>
              <td class="text-ink md:py-5 md:pr-6 md:align-top">{d}</td>
              <td class="text-ink-2 md:w-64 md:py-5 md:align-top">{st}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if RELEASED}
      <p class="reveal mt-12 flex flex-wrap justify-center gap-x-7 gap-y-3">
        <span class="role">Documentation</span>
        {#each docs as [name, f]}<a class="text-sm no-underline" href="{REPO}/blob/main/docs/{f}.md" rel="noopener">{name}</a>{/each}
      </p>
    {/if}
  </div>
</section>
