<script>
  import Shot from "./Shot.svelte";
  const facts = [
    ["Read-only by default", "One setting, off by default, gates every change: playback progress, watched and favourite flags, your own password and all admin actions."],
    ["Admin needs more", "Admin actions also need an administrator account and a confirmation dialog."],
    ["Token in the keyring", "Your sign-in lives in the system keyring (Secret Service), never in a file."],
    ["No telemetry", "No analytics, no crash reports. The app talks only to the server you point it at."],
  ];
</script>

<section id="safety" class="section relative isolate overflow-hidden rule" aria-labelledby="safety-h">
  <div class="pool left-[-10%] top-[10%] size-[520px] bg-accent/10" aria-hidden="true"></div>
  <div class="wrap">
    <div class="reveal max-w-[46rem]">
      <p class="label">Safety</p>
      <h2 id="safety-h" class="h2 mt-5">Read-only until you say otherwise.</h2>
      <p class="lede mt-5">Your server holds your personal library, so NOCTURNE starts as a read-only client. Controls that need writes stay visible but dimmed, with a tooltip that says why.</p>
    </div>

    <div class="reveal mt-[clamp(40px,5vw,64px)]">
      <Shot name="settings-crop" sizes="(min-width: 1260px) 1240px, 100vw">
        <span class="reticle" aria-hidden="true"></span>
        <span class="callout" aria-hidden="true">Off by default</span>
      </Shot>
    </div>

    <dl class="stagger m-0 mt-[clamp(40px,5vw,64px)] grid gap-x-12 gap-y-10 sm:grid-cols-2 lg:grid-cols-4">
      {#each facts as [t, d]}
        <div class="reveal rule grid content-start gap-2 pt-6">
          <dt class="font-semibold text-ink">{t}</dt>
          <dd class="m-0 text-ink-2">{d}</dd>
        </div>
      {/each}
    </dl>
    <p class="reveal mt-12 max-w-[64ch] text-sm text-ink-3">The browsing and playback crate only sends GET requests, and a test in the repository fails if a write call appears.</p>
  </div>
</section>
