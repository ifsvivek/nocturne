<script>
  import { VERSION, shipped } from "#lib/site.js";
  // From docs/ROADMAP.md and FEATURES.md: what 0.5.x does not do, and where it is still rough. Revisit at every release.
  const missing = [
    shipped("0.6.0")
      ? "A supported AppImage: the GitHub release has only an experimental one (about 107 MB, it bundles its own libraries; use the repositories where you can). Flatpak is not available: it was evaluated, and would not offer VLC from inside its sandbox"
      : "A supported AppImage: the GitHub release has only an experimental one (about 107 MB, it bundles its own libraries; use the repositories where you can). Flatpak is being evaluated",
    ...(shipped("0.6.0") ? [
      "Other external players: VLC and mpv only for now; MPC-HC for the Windows preview and IINA for the macOS preview are not supported yet",
      "Requests for 4K versions, and Seerr's server, profile and folder choices",
    ] : []),
    "X11 sessions: from 0.5, Linux needs Wayland",
    "Servers other than Jellyfin",
    "Book reading and audiobook playback",
    "Admin: HTTPS and proxy editing, access schedules",
    "Live TV, DVR, DLNA, casting and gapless music (planned for a later release)",
  ];
  // 0.5.2 rows replace the 0.5 ones they outdate only once VERSION reaches 0.5.2.
  const rough = [
    "HDR in the pop-out window needs an HDR display and mpv 0.42 or newer; it has been tried on a Linux, a Windows and a macOS HDR setup. In the window, HDR is tone-mapped to SDR.",
    ...(shipped("0.5.2") ? [
      "Windows (x64) and macOS (Apple Silicon) are previews on the GitHub release: built on GitHub's machines and tried by the maintainer on a real PC and Mac, but unsigned, so the system warns on first start. The Windows preview brings its own fonts, no longer freezes when playback starts and plays video inside the app window, with the picture making room for the controls bar rather than under it (not yet tried on a real PC); the macOS preview still plays it in mpv's own window, and now updates itself in the app, as the Windows preview does.",
      "Themes are drawn natively, not by a browser: what a theme does with JavaScript does not run, backdrop blur becomes a translucent fill, and masks, conic gradients and blur, saturate or contrast filters are not drawn. Settings, Admin and setup keep their own screens.",
      "Your server's own look needs the server's web client; without it the source shows NOCTURNE's layout and Settings, Sources says why. Parts the web client builds with JavaScript are drawn but not wired: the item page's track choices only show, and Play all and Shuffle in the library toolbar do nothing. Live TV and the audio and book continue rows are not shown.",
      "In-app updates have been tested against a local server, not yet from a published release, and the update of the macOS preview has not run on a Mac yet. Update now needs PackageKit; without it the banner shows the command.",
    ] : [
      "Windows (x64) and macOS (Apple Silicon) are previews on the GitHub release: built on GitHub's machines and tried by the maintainer on a real PC and Mac, but unsigned, so the system warns on first start. Video plays in mpv's own window there.",
      "Themes: letter spacing is read but not drawn, and the hover zoom has not been tried with a real pointer yet. A Jellyfin theme from the web has not been tried against the app.",
      "Server UI has not been tried with several servers at once or with a server's real branding CSS. Live TV and the audio and book continue rows are not shown.",
      "In-app updates have been tested against a local server, not yet from a published release. In-app update on macOS is planned; the macOS preview shows Download instead.",
    ]),
    ...(shipped("0.6.0") ? [
      "Seerr requests, the Calendar and the player choice were tried against throwaway servers and a demo library, not against a long-running setup.",
    ] : []),
    "Couch mode has not been tried with a real gamepad or on a TV. Remote control was tried through the server, not yet from a phone app.",
    "The .deb and .rpm install from the apt and dnf repositories in clean containers, not yet on a full desktop install. On Ubuntu 24.04, video plays in an external mpv window.",
    "Copies without an IMDb, TMDB or TVDB id are not merged, so the same film can show twice rather than one being hidden by mistake.",
    "The local index is held in memory, which is fine for tens of thousands of files. Larger libraries are untested.",
    "Screen readers get little from the interface yet.",
    "The photo viewer and music have only been tried on a generated demo library.",
  ];
</script>

<section id="limits" class="section rule" aria-labelledby="limits-h">
  <div class="wrap">
    <header class="reveal"><h2 id="limits-h" class="h2">Not in this cut</h2><p class="lede mt-4">{shipped("0.5.0") ? `Early software, version ${VERSION}.` : "Version 0.5 is being finished."} Here is the honest list.</p></header>
    <div class="mt-12 grid gap-12 md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] md:gap-16">
      <div><h3 class="role">Not implemented</h3><ul class="tick stagger mt-5">{#each missing as m}<li>{m}</li>{/each}</ul></div>
      <div><h3 class="role">Rough edges</h3><ul class="tick stagger mt-5">{#each rough as m}<li>{m}</li>{/each}</ul></div>
    </div>
  </div>
</section>
