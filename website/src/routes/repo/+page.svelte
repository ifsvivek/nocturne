<script>
  import Seo from "#lib/components/Seo.svelte";
  import { RELEASED, SITE } from "#lib/site.js";
  // The signing key and the pacman.conf block, exactly as the repository is published (website/static/repo/ at deploy).
  const KEY_URL = `${SITE}/repo/ARCH-GPG-KEY`;
  const KEY_ID = "5204B42AF768DE210D4979E397FA02EFCF1A4E3B";
  const FINGERPRINT = "5204 B42A F768 DE21 0D49 79E3 97FA 02EF CF1A 4E3B";
  const steps = [
    ["Import the signing key", `curl -fsSL ${KEY_URL} | sudo pacman-key --add -\nsudo pacman-key --lsign-key ${KEY_ID}`],
    ["Add the repository to /etc/pacman.conf", `sudo tee -a /etc/pacman.conf << 'EOF'\n\n[nocturne]\nSigLevel = Required TrustedOnly\nServer = ${SITE}/repo/arch/$arch\nEOF`],
    ["Refresh the package databases", "sudo pacman -Sy"],
    ["Install NOCTURNE", "sudo pacman -S nocturne"],
  ];
  const later = [["Debian and Ubuntu", ".deb"], ["Fedora", ".rpm"]];
</script>

<Seo title="Package repository: NOCTURNE" description="Install NOCTURNE from its signed pacman repository on Arch Linux, CachyOS, EndeavourOS or Manjaro, and get updates with the rest of your system." path="/repo" />
<div class="wrap max-w-[880px] pt-[clamp(140px,18vh,200px)] pb-[clamp(64px,9vw,120px)]">
  <span class="role">Install</span>
  <h1 class="scene-title">Package repository</h1>

  {#if RELEASED}
    <p class="lede mt-6">Signed packages for pacman. Add the repository once, and NOCTURNE updates with the rest of your system.</p>

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

      <aside class="mt-10 rounded-[3px] border-l-2 border-accent bg-surface px-6 py-5 ring-1 ring-line ring-inset" aria-label="Signing key">
        <p class="role m-0">Signing key</p>
        <p class="m-0 mt-3 font-mono text-[0.95rem] tracking-[0.04em] text-ink">{FINGERPRINT}</p>
        <p class="m-0 mt-2 text-sm text-ink-2">ed25519, valid until 2029-11-15. Key: <a class="break-all" href={KEY_URL}>{KEY_URL}</a></p>
      </aside>
    </section>

    <section class="rule mt-16 pt-10" aria-labelledby="later">
      <h2 id="later" class="h2">Other formats</h2>
      <dl class="m-0 mt-8 grid gap-y-4">
        {#each later as [name, ext]}
          <div class="grid gap-x-6 sm:grid-cols-[14rem_minmax(0,1fr)]"><dt class="text-ink">{name} <span class="text-ink-3">({ext})</span></dt><dd class="m-0 text-ink-2">Planned with 0.4.</dd></div>
        {/each}
      </dl>
    </section>

    <p class="mt-16 text-sm text-ink-2">Rather not add a repository? The PKGBUILD, the tarball and the source build are under <a href="/#install">Install</a>.</p>
  {:else}
    <p class="lede mt-6">The package repository opens with the first release.</p>
  {/if}
</div>
