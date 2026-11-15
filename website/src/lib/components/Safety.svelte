<script>
  import Shot from "./Shot.svelte";
  const facts = [
    ["Read-only by default", "One setting, off by default, gates every change: playback progress, watched and favourite flags, your own password and all admin actions."],
    ["Admin needs more", "Admin actions also need an administrator account and a confirmation dialog."],
    ["Token in the keyring", "Your sign-in lives in the system keyring (Secret Service), never in a file."],
    ["No telemetry", "No analytics, no crash reports. The app talks only to the server you point it at."],
  ];
</script>

<section id="safety" class="relative overflow-hidden border-t border-line py-[clamp(64px,9vw,136px)]" aria-labelledby="safety-h">
  <div class="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(ellipse_50%_60%_at_85%_40%,rgb(75_85_224/0.16),transparent_70%)]" aria-hidden="true"></div>
  <div class="wrap grid items-center gap-12 min-[900px]:grid-cols-[6fr_6fr] min-[900px]:gap-16">
    <div class="reveal">
      <p class="label">Safety</p>
      <h2 id="safety-h" class="h2 mt-3">Read-only until you say otherwise</h2>
      <p class="lede mt-4">Your server holds your personal library, so NOCTURNE starts as a read-only client. Controls that need writes stay visible but dimmed, with a tooltip that says why.</p>
      <dl class="m-0 mt-8 grid gap-6">
        {#each facts as [t, d]}
          <div class="grid gap-1 border-l border-line-strong pl-5">
            <dt class="font-medium text-ink">{t}</dt>
            <dd class="m-0 max-w-[56ch] text-ink-2">{d}</dd>
          </div>
        {/each}
      </dl>
      <p class="mt-6 max-w-[56ch] text-sm text-ink-2">The browsing and playback crate is GET-only, and a test in the repository fails if a write call appears.</p>
    </div>
    <div class="reveal"><Shot name="settings" sizes="(min-width: 900px) 560px, 100vw" /></div>
  </div>
</section>
