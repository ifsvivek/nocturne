<script>
  import Section from "./Section.svelte";
  import { RELEASED, REPO, RELEASES, VERSION } from "#lib/site.js";
</script>

<Section id="install" no="05" tag="INSTALL" title="Requirements and install">
  <div class="grid gap-4 md:grid-cols-2">
    <div class="panel">
      <span class="label">REQUIRES</span>
      <ul class="tick">
        <li>Linux, x86_64</li>
        <li>libmpv (the <code>mpv</code> package on Arch; 0.41 tested)</li>
        <li>Wayland for in-window video; a Vulkan GPU for the UI, OpenGL/EGL for video</li>
        <li>A Jellyfin server (developed against 12.x)</li>
        <li>A Secret Service provider to keep your sign-in</li>
      </ul>
      <p class="text-sm text-ink-2">On X11 the UI runs and video falls back to an external mpv window. <code>NOCTURNE_VIDEO=external</code> forces that on any session.</p>
    </div>
    <div class="panel">
      <span class="label">VERIFIED vs NOT YET</span>
      <ul class="tick">
        <li><b>Verified:</b> Arch Linux, Hyprland on Wayland, NVIDIA GPU, mpv 0.41, Jellyfin 12.x</li>
        <li><b>Not yet verified:</b> GNOME and KDE sessions, AMD and Intel video decode, the X11 fallback on release builds, other distributions, older Jellyfin servers</li>
      </ul>
    </div>
  </div>
  {#if RELEASED}
    <div class="mt-4 grid gap-4 md:grid-cols-2">
      <div class="panel">
        <span class="label">ARCH / CACHYOS</span>
        <pre class="m-0 overflow-x-auto rounded-[10px] border border-line bg-frame px-4 py-3.5 text-[0.84rem] leading-[1.7]"><code>git clone {REPO}.git
cd nocturne/packaging
makepkg -si</code></pre>
      </div>
      <div class="panel">
        <span class="label">TARBALL v{VERSION}</span>
        <pre class="m-0 overflow-x-auto rounded-[10px] border border-line bg-frame px-4 py-3.5 text-[0.84rem] leading-[1.7]"><code>tar xf nocturne-*.tar.gz
cd nocturne-*/
./install.sh            # PREFIX=/usr/local</code></pre>
        <p class="text-sm text-ink-2">Get the archive from <a href={RELEASES} rel="noopener">the latest release</a>. <code>./install.sh uninstall</code> removes it.</p>
      </div>
    </div>
  {:else}
    <p class="mb-0 mt-4 text-ink-2">Packages for Arch/CachyOS (PKGBUILD) and a generic tarball with an install script are being prepared. Download links appear here with the first release.</p>
  {/if}
  <p class="mb-0 mt-4 text-sm text-ink-2">Hyprland and similar compositors that fade inactive windows will dim the video subsurface; exempt app id <code>nocturne</code> from window opacity.</p>
</Section>
