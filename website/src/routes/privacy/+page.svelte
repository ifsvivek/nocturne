<script>
  import Seo from "#lib/components/Seo.svelte";
</script>

<Seo title="Privacy: NOCTURNE" description="NOCTURNE's site sets no cookies and runs no analytics. The app talks only to the server you configure." path="/privacy" />
<div class="wrap min-h-[60vh] max-w-[720px] py-[clamp(48px,8vw,96px)]">
  <p class="label">LEGAL / PRIVACY</p>
  <h1 class="mt-3 text-[clamp(2rem,5vw,3rem)] font-normal leading-[1.1] tracking-tight">Privacy</h1>
  <p class="mt-5 text-lg text-ink-2">Short version: this site tracks nothing, and the app only talks to the server you point it at.</p>

  <h2 class="mb-3.5 mt-11 text-xl font-medium">This website</h2>
  <ul class="tick">
    <li>It sets no cookies and uses no local storage.</li>
    <li>It runs no analytics, no tracking pixels and no third-party scripts, fonts or embeds. Every file comes from this domain.</li>
    <li>Like any web host, the infrastructure serving it (Cloudflare) may process request metadata such as IP address to deliver pages and defend against abuse. We do not read or store that for any other purpose.</li>
  </ul>

  <h2 class="mb-3.5 mt-11 text-xl font-medium">The NOCTURNE app</h2>
  <ul class="tick">
    <li>It connects only to the Jellyfin server you configure. It sends no telemetry, usage statistics or crash reports anywhere.</li>
    <li>Your sign-in token is stored in the system keyring (Secret Service). It is not written to a plaintext file.</li>
    <li>Changes to your server are off by default. A single setting, <em>Allow NOCTURNE to change data on the server</em>, must be turned on by you before the app will sync playback progress, mark items watched or favourite, change your password, or perform any admin action.</li>
    <li>Artwork is cached on your own disk to speed up browsing; nothing is uploaded.</li>
  </ul>

  <p class="mt-10 text-sm text-ink-2">NOCTURNE is an independent project, not affiliated with or endorsed by Jellyfin. <a href="/">Back to the home page</a>.</p>
</div>
