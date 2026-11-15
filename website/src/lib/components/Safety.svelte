<script>
  import Shot from "./Shot.svelte";
  const facts = [
    ["Server writes", "One setting, off by default, gates every change: playback progress, watched and favourite flags, your own password and all admin actions."],
    ["Admin actions", "Also need an administrator account and a confirmation dialog."],
    ["Sign-in", "Your token lives in the system keyring (Secret Service), never in a file."],
    ["Telemetry", "None. No analytics, no crash reports. The app talks only to the server you point it at."],
  ];
</script>

<!-- Safety as a film rating card. -->
<section id="safety" class="section rule" aria-labelledby="safety-h">
  <div class="wrap">
    <div class="reveal mx-auto max-w-[760px]">
      <p class="role text-center">This application is rated</p>
      <div class="mt-6 border-2 border-ink p-1">
        <div class="grid grid-cols-[auto_minmax(0,1fr)] border border-ink">
          <span class="grid place-items-center border-r-2 border-ink px-[clamp(14px,3vw,30px)] text-[clamp(2.6rem,1.6rem+4vw,5rem)] leading-none font-bold tracking-[-0.03em]" aria-hidden="true">RO</span>
          <div class="p-[clamp(16px,3vw,28px)]">
            <h2 id="safety-h" class="m-0 text-[clamp(1.5rem,1.1rem+1.8vw,2.6rem)] leading-none font-bold tracking-[0.12em] uppercase">Read-only</h2>
            <p class="m-0 mt-3 text-balance text-[clamp(0.72rem,0.68rem+0.2vw,0.85rem)] font-medium tracking-[0.16em] text-ink-2 uppercase">Nothing on your server changes until you allow it</p>
          </div>
        </div>
        <p class="m-0 mt-1 border-t-2 border-ink py-2 text-center text-[0.68rem] font-medium tracking-[0.3em] uppercase">One switch. Off by default.</p>
      </div>
    </div>

    <dl class="stagger mx-auto mt-[clamp(56px,7vw,96px)] grid max-w-[1000px] gap-x-14 gap-y-10 md:grid-cols-2">
      {#each facts as [t, d]}
        <div class="rule pt-5"><dt class="role">{t}</dt><dd class="m-0 mt-3 text-ink-2">{d}</dd></div>
      {/each}
    </dl>

    <div class="reveal mx-auto mt-[clamp(56px,7vw,96px)] max-w-[1000px]">
      <Shot name="settings-crop" sizes="(min-width: 1040px) 1000px, 100vw">
        <span class="reticle" aria-hidden="true"></span>
      </Shot>
      <p class="mt-5 text-center text-sm text-ink-3">The browsing and playback crate only sends GET requests, and a test in the repository fails if a write call appears. The capture is from a demo library with the switch turned on; a fresh install has it off.</p>
    </div>
  </div>
</section>
