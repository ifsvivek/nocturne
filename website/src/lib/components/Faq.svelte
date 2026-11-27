<script>
  import Next from "./Next.svelte";
  import { shipped } from "#lib/site.js";
  // 0.5.2 answers replace the 0.5 ones only once VERSION reaches 0.5.2 (no mark ahead of time).
  const v052 = shipped("0.5.2");
  const v060 = shipped("0.6.0");
  const v070 = shipped("0.7.0");
  // [question, answer, version that ships it (marked until VERSION reaches it)]
  const flatpak = shipped("0.6.0") ? "Flatpak is not available yet" : "Flatpak is being evaluated";
  const items = [
    ["Do I need a Jellyfin server?", "No. NOCTURNE works with local folders alone, with one or several Jellyfin servers, or with both. The first run asks which, and Settings, Sources changes it later.", "0.3.0"],
    ["Can I use several servers?", "Yes, each with its own account. Their libraries join one catalog, a source filter shows one at a time, and when one server is offline the others keep working.", "0.3.0"],
    ["What if the same film is on two sources?", "It shows once. Copies are merged only when they share an IMDb, TMDB or TVDB id, and the others are listed as alternate sources on the detail page. Titles that merely look alike stay separate.", "0.3.0"],
    ["Does it change my files?", "Never. Local folders are read in place: nothing is renamed, moved, modified or deleted. The index, resume points and your edits live in NOCTURNE's own data folder.", "0.3.0"],
    ["Is this an official Jellyfin app?", "No. NOCTURNE is an independent, unofficial project. It is not affiliated with, endorsed by or sponsored by the Jellyfin project, and uses the name only to say which server it works with."],
    ["Which systems does it run on?", "Linux: signed repositories for Arch, Debian 13 and Ubuntu 24.04 or newer, and Fedora, and a tarball. Windows (x64) and macOS (Apple Silicon) have preview builds on the GitHub release, unsigned, tried on a real PC and Mac, " + (v052 ? "with video inside the app window in the Windows preview and in mpv's own window in the macOS preview" : "with video in mpv's own window") + "; there is an experimental AppImage on the GitHub release, and " + flatpak + ". Linux needs a Wayland session: X11 sessions are not supported.", "0.4.0"],
    ["What needs permission to change my servers?", "Playback progress sync, watched and favourite flags, your own password, your home sections on a server and every admin action. All of them sit behind one setting, Allow NOCTURNE to change data on the server, which is off by default."],
    ["Where are my credentials stored?", "Server sign-in tokens go into the system keyring (Secret Service), never into a file. Settings and remembered servers (URLs only) are plain files under your config directory. Without a running Secret Service, NOCTURNE cannot remember a sign-in."],
    ["Does it send data anywhere?", "Only to the servers you add and to the online services you turn on (details, subtitles, Trakt)" + (v052 ? ", the addresses a theme you add loads its imports, fonts and pictures from" : "") + ", plus one daily request to this site asking for the newest version, which Settings, Updates turns off. There is no telemetry, usage statistics or crash reporting.", "0.4.0"],
    ...(v052 ? [
      ["Can I change how it looks?", "Yes. Pick NOCTURNE or Glass, start from Spotlight, Marquee, Feed or Classic, or build your own in the Theme Studio by pointing at the app; it writes real CSS. Themes lay out Home, the header, libraries, search, the detail page and the player's controls with flex, grid and positioning, globally or per server."],
      ["Can I use my Jellyfin theme?", "Mostly, yes. Paste its CSS, @import line or address in Settings, Themes; its imports, fonts and pictures are fetched and the preview stays until you keep or revert it. For the closest match, let that source use its server's UI in Settings, Sources: NOCTURNE then builds the web client's own page structure, so most themes look as they do in the browser. What a theme does with JavaScript, backdrop blur, masks and blur filters still differ, and Settings, Themes lists everything that did not apply."],
      ["How does it update?", "Installed from a repository, with your system's updates: NOCTURNE shows the pacman, apt or dnf command when a new version is out, and where PackageKit runs (most desktops), Update now updates only NOCTURNE after your desktop's usual password prompt. A per-user install updates itself in the app after checking the download's signature."],
    ] : [
      ["Can I change how it looks?", "Yes, with a CSS theme in Settings, Themes: colours, a font, card shape and size, carousel or grid rows, the hero and header and the order of Home's sections, globally or per server. NOCTURNE draws it natively, so it styles NOCTURNE's own parts; a Jellyfin web theme applies only partly.", "0.5.0"],
      ["How does it update?", "Installed from a repository, with your system's updates; NOCTURNE shows the pacman, apt or dnf command when a new version is out. A per-user install updates itself in the app after checking the download's signature.", "0.5.0"],
    ]),
    ...(v060 ? [
      ["Can I request movies?", "Yes, if you run a Seerr server (Jellyseerr and Overseerr work too). Sign in to it in Settings, turn on Allow requests, and a title that is not in your library gets a Request button. The request goes to the Seerr server, not to a media server, and what happens next is up to Seerr. 4K requests are not offered yet."],
      ["Can I use VLC?", "Yes. If VLC is installed, Settings offers it as the player, next to the in-window player and an mpv window. NOCTURNE controls VLC over a local, password-protected interface, so your place, progress sync, Trakt and Stop keep working. VLC's own menus handle subtitles and audio."],
    ] : []),
    ...(v070 ? [
      ["Can my phone or TV app use NOCTURNE?", "It may. Switch on the gateway in Settings, Gateway and NOCTURNE answers as a Jellyfin server of its own, with all your servers and local folders as one library. Add that address in a Jellyfin app and sign in as nocturne or with Quick Connect. By default it listens on this computer only; the local network is opt-in. The project has not yet tried the official phone and TV apps, so expect rough edges."],
      ["Can NOCTURNE send requests to Radarr or Sonarr?", "Yes, if you connect your own in Settings, Requests and turn on adding requests there. Each request asks first, goes to the service you chose and shows its status afterwards. Without Seerr, requests can also stay in a list on your computer that marks a title Available once a source has it."],
    ] : []),
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
