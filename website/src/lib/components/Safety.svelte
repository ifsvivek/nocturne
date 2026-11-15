<script>
  import Section from "./Section.svelte";
</script>

<Section id="safety" no="03" tag="SAFETY" title="Read-only until you say otherwise">
  <div class="grid max-w-[70ch] gap-5">
    <div class="flex flex-wrap items-center gap-3.5 rounded-xl border border-line-strong bg-surface px-[18px] py-4" aria-hidden="true">
      <span class="label min-w-[220px] flex-1 text-ink-2">ALLOW NOCTURNE TO CHANGE DATA ON THE SERVER</span>
      <span class="relative h-[22px] w-10 rounded-full border border-line-strong bg-frame"><i class="absolute left-[3px] top-[3px] size-3.5 rounded-full bg-ink-3"></i></span>
      <span class="font-mono text-[0.72rem] tracking-[0.12em] text-ink-2">OFF &middot; DEFAULT</span>
    </div>
    <p class="m-0 text-ink-2">Your server holds your personal library, so NOCTURNE starts as a read-only client. One master setting, off by default, gates every change it could make: playback progress sync, watched and favourite flags, your own password, and all admin actions. Controls that need it stay visible but dimmed, with a tooltip that says why.</p>
    <ul class="tick">
      <li>Browsing and playback use a GET-only client; a test in the repository fails if a write call appears.</li>
      <li>Admin writes live in a separate crate and need a confirmation token minted only by the confirm dialog.</li>
      <li>Signing in and out is separate: any client must create its own session to authenticate.</li>
    </ul>
  </div>
</Section>
