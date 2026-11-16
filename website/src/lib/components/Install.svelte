<script>
  import { RELEASED, REPO, RELEASES, VERSION } from "#lib/site.js";
  import Next from "./Next.svelte";
  // [needs, detail, status, version that ships the detail (marked until VERSION reaches it)]
  const specs = [
    ["Platform", "Linux, x86_64, with packages for Arch Linux only. .deb and .rpm packages are planned; an AppImage is under evaluation. macOS and Windows are planned, not available.", "Linux only today"],
    ["libmpv", "Comes with the package: the Arch package depends on mpv, and the tarball installer installs it with your distribution's package manager. NOCTURNE also starts without it and tells you what to install.", "mpv 0.41 tested; 0.42's certificate check needs no change", "0.3.0"],
    ["Display", "Wayland for in-window video. On X11 the UI runs and video plays in an external mpv window (NOCTURNE_VIDEO=external forces that anywhere).", "Wayland and X11 fallback verified"],
    ["Graphics", "A Vulkan driver for the UI, OpenGL/EGL for video.", "Other GPU setups not yet tested"],
    ["Media", "Local folders you can read, a Jellyfin server, several servers, or any mix. None of them is required on its own.", "Jellyfin developed against 12.x", "0.3.0"],
    ["Keyring", "A Secret Service provider (gnome-keyring, KWallet or KeePassXC) to remember server sign-ins.", "Required to stay signed in to a server"],
    ["Distribution", "Packages are tested on Arch Linux only. The tarball binary is built on current Arch, so it needs a current glibc.", "Debian, Ubuntu, Fedora, openSUSE untested"],
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

    <div class="stagger mt-[clamp(56px,7vw,96px)] grid gap-y-10 *:min-w-0 lg:grid-cols-3 lg:divide-x lg:divide-line">
      <div class="lg:pr-10">
        <h3 class="role text-ink">Arch Linux</h3>
        {#if RELEASED}
          <p class="m-0 mt-3 text-ink-2">A signed pacman repository: add it once, and NOCTURNE updates with the rest of your system. <a href="/repo">Set up the repository</a>.</p>
          <pre class="code"><code>sudo pacman -S nocturne</code></pre>
          <p class="m-0 mt-6 text-sm text-ink-2">Or build the PKGBUILD from the v{VERSION} release, which pulls in mpv for playback:</p>
          <pre class="code"><code>git clone --branch v{VERSION} --depth 1 {REPO}.git
cd nocturne/packaging
makepkg -si</code></pre>
        {:else}
          <p class="m-0 mt-3 text-ink-2">A signed pacman repository and a PKGBUILD that builds from the release tag's source archive and pulls in mpv for playback.</p>
        {/if}
      </div>
      <div class="lg:px-10">
        <h3 class="role text-ink">Tarball</h3>
        <p class="m-0 mt-3 text-ink-2">A generic x86_64 build with an install script. Installs to <code>/usr/local</code>, and the same script uninstalls. It also installs the runtime packages, mpv included. <Next /></p>
        {#if RELEASED}<pre class="code"><code>tar xf nocturne-{VERSION}-x86_64-linux.tar.gz
cd nocturne-{VERSION}-x86_64-linux
sudo ./install.sh</code></pre>
          <p class="mt-4 text-sm text-ink-2">Get it from <a href={RELEASES} rel="noopener">the latest release</a>.</p>{/if}
      </div>
      <div class="lg:pl-10">
        <h3 class="role text-ink">From source</h3>
        <p class="m-0 mt-3 text-ink-2">Stable Rust 1.89 or newer, and mpv installed to play video. A clean build takes several minutes.</p>
        {#if RELEASED}<pre class="code"><code>git clone --branch v{VERSION} --depth 1 {REPO}.git
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
          {#each specs as [k, d, st, v]}
            <tr class="border-t border-line max-md:grid max-md:gap-1 max-md:py-5">
              <th scope="row" class="text-[0.72rem] font-medium tracking-[0.26em] text-ink-3 uppercase md:w-44 md:py-5 md:pr-6 md:align-top">{k}</th>
              <td class="text-ink md:py-5 md:pr-6 md:align-top">{d}{#if v} <Next {v} />{/if}</td>
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
