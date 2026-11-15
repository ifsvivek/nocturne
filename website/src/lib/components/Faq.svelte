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

<section id="faq" class="border-t border-line py-[clamp(64px,9vw,136px)]" aria-labelledby="faq-h">
  <div class="wrap grid gap-10 min-[900px]:grid-cols-[4fr_8fr] min-[900px]:gap-16">
    <h2 id="faq-h" class="h2 reveal min-[900px]:sticky min-[900px]:top-28 min-[900px]:self-start">Questions</h2>
    <div class="reveal border-t border-line">
      {#each items as [q, a]}
        <details class="faq"><summary>{q}</summary><div><p>{a}</p></div></details>
      {/each}
    </div>
  </div>
</section>
