<script>
  import Next from "./Next.svelte";
  // [question, answer, version that ships it (marked until VERSION reaches it)]
  const items = [
    ["Do I need a Jellyfin server?", "No. NOCTURNE works with local folders alone, with one or several Jellyfin servers, or with both. The first run asks which, and Settings, Sources changes it later.", "0.3.0"],
    ["Can I use several servers?", "Yes, each with its own account. Their libraries join one catalog, a source filter shows one at a time, and when one server is offline the others keep working.", "0.3.0"],
    ["What if the same film is on two sources?", "It shows once. Copies are merged only when they share an IMDb, TMDB or TVDB id, and the others are listed as alternate sources on the detail page. Titles that merely look alike stay separate.", "0.3.0"],
    ["Does it change my files?", "Never. Local folders are read in place: nothing is renamed, moved, modified or deleted. The index, resume points and your edits live in NOCTURNE's own data folder.", "0.3.0"],
    ["Is this an official Jellyfin app?", "No. NOCTURNE is an independent, unofficial project. It is not affiliated with, endorsed by or sponsored by the Jellyfin project, and uses the name only to say which server it works with."],
    ["Which systems does it run on?", "Linux: signed repositories for Arch, Debian 13 and Ubuntu 24.04 or newer, and Fedora, and a tarball. Windows (x64) and macOS (Apple Silicon) have preview builds on the GitHub release, unsigned and not yet tried on real hardware, with video in mpv's own window; there is no AppImage, and Flatpak is being evaluated. Linux needs a Wayland session: X11 sessions are not supported.", "0.4.0"],
    ["What needs permission to change my servers?", "Playback progress sync, watched and favourite flags, your own password, your home sections on a server and every admin action. All of them sit behind one setting, Allow NOCTURNE to change data on the server, which is off by default."],
    ["Where are my credentials stored?", "Server sign-in tokens go into the system keyring (Secret Service), never into a file. Settings and remembered servers (URLs only) are plain files under your config directory. Without a running Secret Service, NOCTURNE cannot remember a sign-in."],
    ["Does it send data anywhere?", "Only to the servers you add and to the online services you turn on (details, subtitles, Trakt), plus one daily request to this site asking for the newest version, which Settings, Updates turns off. There is no telemetry, usage statistics or crash reporting.", "0.4.0"],
    ["Can I change how it looks?", "Yes, with a CSS theme in Settings, Themes: colours, a font, card shape and size, carousel or grid rows, the hero and header and the order of Home's sections, globally or per server. NOCTURNE draws it natively, so it styles NOCTURNE's own parts; a Jellyfin web theme applies only partly.", "0.5.0"],
    ["How does it update?", "Installed from a repository, with your system's updates; NOCTURNE shows the pacman, apt or dnf command when a new version is out. A per-user install updates itself in the app after checking the download's signature.", "0.5.0"],
    ["Does it work with other media servers?", "Not yet. Jellyfin is the first server type; local folders need no server at all."],
  ];
</script>

<!-- Every answer is open: short answers read faster in a grid than behind a click each. -->
<section id="faq" class="section rule" aria-labelledby="faq-h">
  <div class="wrap grid gap-12 lg:grid-cols-[minmax(0,4fr)_minmax(0,8fr)] lg:gap-16">
    <h2 id="faq-h" class="h2 reveal lg:sticky lg:top-16 lg:self-start">Questions</h2>
    <dl class="stagger m-0 grid gap-x-12 gap-y-12 md:grid-cols-2">
      {#each items as [q, a, v]}
        <div>
          <dt class="text-[1.04rem] font-semibold leading-snug text-ink">{q}</dt>
          <dd class="m-0 mt-3 text-pretty text-ink-2">{a}{#if v} <Next {v} />{/if}</dd>
        </div>
      {/each}
    </dl>
  </div>
</section>
