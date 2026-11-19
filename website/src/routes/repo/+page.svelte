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
  // apt and dnf: the same key, exactly as the root README's Install section documents them.
  const KEYRING = "/usr/share/keyrings/nocturne.gpg";
  const RPM_REPO = `${SITE}/repo/rpm/nocturne.repo`;
  const debSteps = [
    ["Trust the signing key", `curl -fsSL ${SITE}/repo/NOCTURNE-GPG-KEY.gpg | sudo tee ${KEYRING} >/dev/null`],
    ["Add the repository", `echo 'deb [signed-by=${KEYRING}] ${SITE}/repo/deb stable main' | sudo tee /etc/apt/sources.list.d/nocturne.list`],
    ["Install NOCTURNE", "sudo apt update && sudo apt install nocturne"],
  ];
  const rpmSteps = [
    ["Enable RPM Fusion free for mpv-libs (skip if it is already on)", "sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm"],
    ["Add the repository", `sudo dnf config-manager addrepo --from-repofile=${RPM_REPO}`],
    ["Install NOCTURNE", "sudo dnf install nocturne"],
  ];
</script>

{#snippet list(items, update)}
  <ol class="steps mt-10">
    {#each items as [title, cmd], i}
      <li>
        <h3 class="m-0 text-[1.02rem] font-semibold text-ink">{title}</h3>
        <pre class="code"><code>{cmd}</code></pre>
        {#if i === items.length - 1}<p class="m-0 mt-4 text-sm text-ink-2">Updates then come with <code>{update}</code>.</p>{/if}
      </li>
    {/each}
  </ol>
{/snippet}

<Seo title="Package repository: NOCTURNE" description="Install NOCTURNE from its signed package repositories: pacman for Arch Linux, CachyOS, EndeavourOS and Manjaro, apt for Debian and Ubuntu, dnf for Fedora." path="/repo" />
<div class="wrap max-w-[880px] pt-[clamp(140px,18vh,200px)] pb-[clamp(64px,9vw,120px)]">
  <span class="role">Install</span>
  <h1 class="scene-title">Package repository</h1>

  {#if RELEASED}
    <p class="lede mt-6">Signed packages for pacman, apt and dnf: add the repository once, and NOCTURNE updates with the rest of your system.</p>

    <section class="rule mt-14 pt-10" aria-labelledby="pacman">
      <h2 id="pacman" class="h2">Pacman</h2>
      <p class="m-0 mt-3 text-ink-2">Arch Linux and derivatives such as CachyOS, EndeavourOS and Manjaro.</p>
      {@render list(steps, "sudo pacman -Syu")}

      <div class="mt-10 grid gap-y-4 text-sm text-ink-2" aria-label="Troubleshooting">
        <p class="m-0"><span class="text-ink">Works in bash, zsh and fish.</span> Paste the commands as they are into any of them.</p>
        <p class="m-0"><span class="text-ink">"target not found: nocturne"</span> means the <code>[nocturne]</code> section is not in <code>/etc/pacman.conf</code>: check with <code>grep -A2 '^\[nocturne\]' /etc/pacman.conf</code> and repeat step 2.</p>
        <p class="m-0"><span class="text-ink">"third-party key signatures using the SHA1 algorithm are rejected"</span> is a harmless note from gpg about Arch's own keyring. Keep <code>SigLevel = Required TrustedOnly</code>; never lower it to make NOCTURNE install.</p>
      </div>
    </section>

    <section class="rule mt-16 pt-10" aria-labelledby="deb">
      <h2 id="deb" class="h2">Debian and Ubuntu</h2>
      <p class="m-0 mt-3 text-ink-2">Debian 13, Ubuntu 24.04 and newer, with apt. Debian 12 and Ubuntu 22.04 are too old for this build. <Next v="0.4.0" /></p>
      {@render list(debSteps, "sudo apt upgrade")}
      <div class="mt-10 grid gap-y-4 text-sm text-ink-2" aria-label="Notes for apt">
        <p class="m-0"><span class="text-ink">Check the key</span> with <code>gpg --show-keys {KEYRING}</code>: the fingerprint must match the one below.</p>
        <p class="m-0"><span class="text-ink">Ubuntu 24.04</span> ships libmpv 0.37, which in-window video cannot use yet, so video plays in an external mpv window there.</p>
        <p class="m-0"><span class="text-ink">One file instead:</span> download <a class="break-all" href={asset(DEB)}>{DEB}</a> and run <code>sudo apt install ./{DEB}</code>; updates then mean installing the next release's file.</p>
      </div>
    </section>

    <section class="rule mt-16 pt-10" aria-labelledby="rpm">
      <h2 id="rpm" class="h2">Fedora</h2>
      <p class="m-0 mt-3 text-ink-2">The Fedora release it was built on (45) and newer, with dnf. mpv-libs comes from RPM Fusion, so that is enabled first. <Next v="0.4.0" /></p>
      {@render list(rpmSteps, "sudo dnf upgrade")}
      <div class="mt-10 grid gap-y-4 text-sm text-ink-2" aria-label="Notes for dnf">
        <p class="m-0"><span class="text-ink">dnf asks once to import the signing key.</span> Compare its fingerprint with the one below, then answer <code>y</code>.</p>
        <p class="m-0"><span class="text-ink">On dnf4</span>, which has no <code>addrepo</code>: use <code class="break-all">sudo dnf config-manager --add-repo {RPM_REPO}</code> (package <code>dnf-plugins-core</code>), or save the file to <code>/etc/yum.repos.d/</code> yourself.</p>
        <p class="m-0"><span class="text-ink">One file instead:</span> download <a class="break-all" href={asset(RPM)}>{RPM}</a> and run <code>sudo dnf install ./{RPM}</code>; updates then mean installing the next release's file.</p>
      </div>
    </section>

    <aside class="mt-16 rounded-[3px] border-l-2 border-accent bg-surface px-6 py-5 ring-1 ring-line ring-inset" aria-label="Signing key">
      <p class="role m-0">Signing key</p>
      <p class="m-0 mt-3 font-mono text-[0.95rem] tracking-[0.04em] text-ink">{FINGERPRINT}</p>
      <p class="m-0 mt-2 text-sm text-ink-2">ed25519, valid until 2029-11-15. One key signs all three repositories. pacman: <a class="break-all" href={KEY_URL}>{KEY_URL}</a>; apt: <a class="break-all" href="{SITE}/repo/NOCTURNE-GPG-KEY.gpg">{SITE}/repo/NOCTURNE-GPG-KEY.gpg</a>; dnf: <a class="break-all" href="{SITE}/repo/NOCTURNE-GPG-KEY">{SITE}/repo/NOCTURNE-GPG-KEY</a>.</p>
    </aside>

    <p class="mt-16 text-sm text-ink-2">macOS and Windows are planned, not available. The PKGBUILD, the tarball and the source build are under <a href="/#install">Install</a>.</p>
  {:else}
    <p class="lede mt-6">The package repository opens with the first release.</p>
  {/if}
</div>
