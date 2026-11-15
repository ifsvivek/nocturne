<script>
  const items = [
    ["Is this an official Jellyfin app?", "No. NOCTURNE is an independent, unofficial client. It is not affiliated with, endorsed by or sponsored by the Jellyfin project, and uses the name only to say which server it works with."],
    ["Which systems does it run on?", "Linux only. Other operating systems are not supported. In-window video needs Wayland; on X11 the interface runs and video plays in an external mpv window."],
    ["What needs permission to change my server?", "Playback progress sync, watched and favourite flags, your own password and every admin action. All of them sit behind one setting, Allow NOCTURNE to change data on the server, which is off by default."],
    ["Where are my credentials stored?", "Your sign-in token goes into the system keyring (Secret Service), never into a file. Settings are in settings.toml and remembered servers (URLs only) in servers.toml, under your config directory. Without a running Secret Service, NOCTURNE cannot remember your sign-in."],
    ["Does it send data anywhere?", "Only to the Jellyfin server you configure. There is no telemetry, usage statistics or crash reporting."],
    ["Does it work with other media servers?", "Not yet. Jellyfin is the only supported source for now."],
  ];
</script>

<!-- Every answer is open: six short answers read faster in a grid than behind six clicks. -->
<section id="faq" class="section rule" aria-labelledby="faq-h">
  <div class="wrap grid gap-12 lg:grid-cols-[minmax(0,4fr)_minmax(0,8fr)] lg:gap-16">
    <h2 id="faq-h" class="h2 reveal lg:sticky lg:top-32 lg:self-start">Questions</h2>
    <dl class="m-0 grid gap-x-12 gap-y-12 md:grid-cols-2">
      {#each items as [q, a]}
        <div class="reveal">
          <dt class="text-[1.06rem] font-semibold leading-snug text-ink">{q}</dt>
          <dd class="m-0 mt-3 text-pretty text-ink-2">{a}</dd>
        </div>
      {/each}
    </dl>
  </div>
</section>
