<script>
  import Seo from "#lib/components/Seo.svelte";
  import Next from "#lib/components/Next.svelte";
  import { shipped } from "#lib/site.js";
</script>

<Seo title="Privacy: NOCTURNE" description="NOCTURNE's site sets no cookies and loads no third-party resources. The app talks only to the servers and services you add, plus a daily update check." path="/privacy" />
<div class="wrap max-w-[760px] pt-[clamp(140px,18vh,200px)] pb-[clamp(64px,9vw,120px)]">
  <h1 class="scene-title">Privacy</h1>
  <p class="mt-5 text-lg text-ink-2">Short version: this site tracks nothing, and the app only talks to the servers and services you add, plus a daily update check you can turn off.</p>

  <h2 class="role mb-5 mt-14">This website</h2>
  <ul class="tick">
    <li>It sets no cookies, uses no local storage and runs no analytics.</li>
    <li>It loads no third-party resources: no scripts, embeds or tracking pixels. The one font is served from this domain, like every other file.</li>
    <li>Like any web host, the infrastructure serving it (Cloudflare) may process request metadata such as IP address to deliver pages and defend against abuse. We do not read or store that for any other purpose.</li>
  </ul>

  <h2 class="role mb-5 mt-14">The NOCTURNE app</h2>
  <ul class="tick">
    <li>It connects only to the Jellyfin servers you add, the online services you turn on{#if shipped("0.5.2")}, the addresses a theme you add loads its files from,{/if} and the update check below. It sends no telemetry, usage statistics or crash reports anywhere.</li>
    <li>Online services are off until you turn them on: details for local files (TMDB with your own key, TVmaze, MusicBrainz), subtitle search (OpenSubtitles, your own key), Trakt and remote control from other Jellyfin apps. For details only the title, year and ids are sent, never file names or paths; keys and sign-ins stay in the system keyring. <Next v="0.4.0" />
      <span class="mt-2 block text-sm text-ink-3">This product uses the TMDB API but is not endorsed or certified by TMDB. TV data from TVmaze.com, licensed CC BY-SA 4.0. Trakt is a trademark of Trakt, Inc. NOCTURNE uses the Trakt API but is not affiliated with or endorsed by Trakt.</span></li>
    <li>Once a day it asks this site for a small file naming the newest version. The request carries only the app's version; like any request, it shows the host your IP address. Settings, Updates turns the check off. An update you install is downloaded from the project's GitHub release. <Next v="0.5.0" /></li>
    {#if shipped("0.5.2")}<li>A theme can name files on the web (other stylesheets, fonts, pictures). NOCTURNE fetches them over https with no cookies or sign-in, so the host sees your IP address, keeps them in its cache and fetches them again only when you ask. A server's own look is read from that server. Settings, Themes has one switch that turns all theme downloads off.</li>{/if}
    <li>Local folders are read in place. NOCTURNE never renames, moves, modifies or deletes your files; its index, resume points and your edits stay in its own data folder on your disk. <Next /></li>
    <li>Each server sign-in token is stored in the system keyring (Secret Service). It is not written to a plaintext file.</li>
    <li>Changes to your server are off by default. A single setting, <em>Allow NOCTURNE to change data on the server</em>, must be turned on by you before the app will sync playback progress, mark items watched or favourite, change your password, or perform any admin action. A change goes only to the server the item came from.</li>
    <li>Artwork is cached on your own disk to speed up browsing; nothing is uploaded.</li>
  </ul>

  <p class="mt-10 text-sm text-ink-2">NOCTURNE is an independent project, not affiliated with or endorsed by Jellyfin. <a href="/">Back to the home page</a>.</p>
</div>
