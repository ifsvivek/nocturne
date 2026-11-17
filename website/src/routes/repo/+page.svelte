<script>
  import Seo from "#lib/components/Seo.svelte";
  import { RELEASED, SITE, DEB, RPM, asset } from "#lib/site.js";
  import Next from "#lib/components/Next.svelte";
  // The signing key and the pacman.conf block, exactly as the repository is published (website/static/repo/ at deploy).
  const KEY_URL = `${SITE}/repo/ARCH-GPG-KEY`;
  const KEY_ID = "5204B42AF768DE210D4979E397FA02EFCF1A4E3B";
  const FINGERPRINT = "5204 B42A F768 DE21 0D49 79E3 97FA 02EF CF1A 4E3B";
  const steps = [
    ["Import the signing key", `curl -fsSL ${KEY_URL} | sudo pacman-key --add -\nsudo pacman-key --lsign-key ${KEY_ID}`],
    // printf, not a heredoc: fish (CachyOS's default shell) has no heredocs, and this line works in bash, zsh and fish.
    ["Add the repository to /etc/pacman.conf", `printf '%s\\n' '' '[nocturne]' 'SigLevel = Required TrustedOnly' 'Server = ${SITE}/repo/arch/$arch' | sudo tee -a /etc/pacman.conf`],
    ["Install NOCTURNE (also refreshes the package databases)", "sudo pacman -Syu nocturne"],
  ];
  // .deb and .rpm: single files from the release page, no repository (yet), so updates mean installing the next file.
  const other = [
    { id: "deb", name: "Debian and Ubuntu", who: "Debian 13, Ubuntu 24.04 and newer. Debian 12 and Ubuntu 22.04 are too old for this build.",
      steps: [["Download the package", `curl -LO ${asset(DEB)}`], ["Install it with its dependencies", `sudo apt install ./${DEB}`]],
      note: "Ubuntu 24.04 ships libmpv 0.37, which in-window video cannot use yet, so video plays in an external mpv window there." },
    { id: "rpm", name: "Fedora", who: "The Fedora release it was built on (45) and newer. mpv-libs comes from RPM Fusion, so that is enabled first.",
      steps: [["Enable RPM Fusion free (skip if it is already on)", "sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm"], ["Download the package", `curl -LO ${asset(RPM)}`], ["Install it with its dependencies", `sudo dnf install ./${RPM}`]],
      note: "" },
  ];
</script>

<Seo title="Package repository: NOCTURNE" description="Install NOCTURNE from its signed pacman repository on Arch Linux, CachyOS, EndeavourOS or Manjaro, or from the .deb and .rpm packages for Debian, Ubuntu and Fedora." path="/repo" />
<div class="wrap max-w-[880px] pt-[clamp(140px,18vh,200px)] pb-[clamp(64px,9vw,120px)]">
  <span class="role">Install</span>
  <h1 class="scene-title">Package repository</h1>

  {#if RELEASED}
    <p class="lede mt-6">Signed packages for pacman: add the repository once, and NOCTURNE updates with the rest of your system. Debian, Ubuntu and Fedora get a .deb and an .rpm further down.</p>

    <section class="rule mt-14 pt-10" aria-labelledby="pacman">
      <h2 id="pacman" class="h2">Pacman</h2>
      <p class="m-0 mt-3 text-ink-2">Arch Linux and derivatives such as CachyOS, EndeavourOS and Manjaro.</p>
      <ol class="steps mt-10">
        {#each steps as [title, cmd], i}
          <li>
            <h3 class="m-0 text-[1.02rem] font-semibold text-ink">{title}</h3>
            <pre class="code"><code>{cmd}</code></pre>
            {#if i === steps.length - 1}<p class="m-0 mt-4 text-sm text-ink-2">Updates then come with <code>sudo pacman -Syu</code>.</p>{/if}
          </li>
        {/each}
      </ol>

      <div class="mt-10 grid gap-y-4 text-sm text-ink-2" aria-label="Troubleshooting">
        <p class="m-0"><span class="text-ink">Works in bash, zsh and fish.</span> Paste the commands as they are into any of them.</p>
        <p class="m-0"><span class="text-ink">"target not found: nocturne"</span> means the <code>[nocturne]</code> section is not in <code>/etc/pacman.conf</code>: check with <code>grep -A2 '^\[nocturne\]' /etc/pacman.conf</code> and repeat step 2.</p>
        <p class="m-0"><span class="text-ink">"third-party key signatures using the SHA1 algorithm are rejected"</span> is a harmless note from gpg about Arch's own keyring. Keep <code>SigLevel = Required TrustedOnly</code>; never lower it to make NOCTURNE install.</p>
      </div>

      <aside class="mt-10 rounded-[3px] border-l-2 border-accent bg-surface px-6 py-5 ring-1 ring-line ring-inset" aria-label="Signing key">
        <p class="role m-0">Signing key</p>
        <p class="m-0 mt-3 font-mono text-[0.95rem] tracking-[0.04em] text-ink">{FINGERPRINT}</p>
        <p class="m-0 mt-2 text-sm text-ink-2">ed25519, valid until 2029-11-15. Key: <a class="break-all" href={KEY_URL}>{KEY_URL}</a></p>
      </aside>
    </section>

    {#each other as f}
      <section class="rule mt-16 pt-10" aria-labelledby={f.id}>
        <h2 id={f.id} class="h2">{f.name}</h2>
        <p class="m-0 mt-3 text-ink-2">{f.who} <Next v="0.4.0" /></p>
        <ol class="steps mt-10">
          {#each f.steps as [title, cmd]}<li><h3 class="m-0 text-[1.02rem] font-semibold text-ink">{title}</h3><pre class="code"><code>{cmd}</code></pre></li>{/each}
        </ol>
        <p class="m-0 mt-6 text-sm text-ink-2">{f.note ? `${f.note} ` : ""}There is no repository for this format yet: to update, install the next release's file the same way.</p>
      </section>
    {/each}

    <p class="mt-16 text-sm text-ink-2">macOS and Windows are planned, not available. The PKGBUILD, the tarball and the source build are under <a href="/#install">Install</a>.</p>
  {:else}
    <p class="lede mt-6">The package repository opens with the first release.</p>
  {/if}
</div>
