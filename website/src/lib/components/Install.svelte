<script>
  import { RELEASED, REPO, RELEASES, VERSION } from "#lib/site.js";
  const pre = "m-0 overflow-x-auto rounded-xl border border-line bg-frame px-4 py-3.5 text-[0.84rem] leading-[1.7]";
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

<section id="install" class="border-t border-line py-[clamp(64px,9vw,136px)]" aria-labelledby="install-h">
  <div class="wrap">
    <div class="reveal max-w-[60ch]">
      <p class="label">Install</p>
      <h2 id="install-h" class="h2 mt-3">Requirements and install</h2>
      {#if !RELEASED}
        <p class="lede mt-4">An Arch Linux PKGBUILD and a generic tarball with an install script are being prepared, along with building from source. Install commands and download links appear here with the first release.</p>
      {/if}
    </div>

    {#if RELEASED}
      <div class="mt-10 grid gap-4 min-[900px]:grid-cols-3">
        <div class="reveal panel">
          <h3 class="h3 text-lg">Arch Linux</h3>
          <pre class={pre}><code>git clone {REPO}.git
cd nocturne/packaging
makepkg -si</code></pre>
          <p class="text-sm text-ink-2">Builds from the source archive of the release tag.</p>
        </div>
        <div class="reveal panel">
          <h3 class="h3 text-lg">Tarball {VERSION}</h3>
          <pre class={pre}><code>tar xf nocturne-{VERSION}-x86_64-linux.tar.gz
cd nocturne-{VERSION}-x86_64-linux
sudo ./install.sh</code></pre>
          <p class="text-sm text-ink-2">Get it from <a href={RELEASES} rel="noopener">the latest release</a>. Installs to <code>/usr/local</code>; <code>sudo ./install.sh uninstall</code> removes it.</p>
        </div>
        <div class="reveal panel">
          <h3 class="h3 text-lg">From source</h3>
          <pre class={pre}><code>git clone {REPO}.git
cd nocturne
cargo build --release --locked
./target/release/nocturne</code></pre>
          <p class="text-sm text-ink-2">Needs stable Rust 1.89+ and <code>pkg-config --modversion mpv</code> to succeed. A clean build takes several minutes.</p>
        </div>
      </div>
    {/if}

    <div class="reveal mt-12 overflow-hidden rounded-2xl border border-line bg-surface">
      <table class="w-full border-collapse text-left text-[0.94rem]">
        <caption class="sr-only">Requirements and compatibility</caption>
        <thead class="max-md:sr-only">
          <tr class="border-b border-line text-ink-3"><th scope="col" class="px-6 py-3.5 font-mono text-[0.72rem] font-medium uppercase tracking-[0.12em]">Needs</th><th scope="col" class="px-6 py-3.5 font-mono text-[0.72rem] font-medium uppercase tracking-[0.12em]">Detail</th><th scope="col" class="px-6 py-3.5 font-mono text-[0.72rem] font-medium uppercase tracking-[0.12em]">Status</th></tr>
        </thead>
        <tbody>
          {#each rows as [k, d, st], i}
            <tr class="max-md:grid max-md:gap-1 max-md:px-5 max-md:py-4 {i ? 'border-t border-line' : ''}">
              <th scope="row" class="whitespace-nowrap font-medium text-ink md:w-36 md:px-6 md:py-4 md:align-top">{k}</th>
              <td class="text-ink-2 md:px-6 md:py-4 md:align-top">{d}</td>
              <td class="text-accent-ink md:w-64 md:px-6 md:py-4 md:align-top">{st}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if RELEASED}
      <div class="reveal mt-10 flex flex-wrap items-center gap-x-6 gap-y-3">
        <span class="label">Documentation</span>
        {#each docs as [name, f]}<a class="link-arrow" href="{REPO}/blob/main/docs/{f}.md" rel="noopener">{name}</a>{/each}
      </div>
    {/if}
  </div>
</section>
