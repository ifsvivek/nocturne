<script>
  import { VERSION, shipped } from "#lib/site.js";
  // From docs/ROADMAP.md and FEATURES.md: what 0.4 does not do, and where it is still rough. Revisit at every release.
  const missing = [
    "An AppImage: not shipped, too large and at odds with the system's libmpv. Flatpak is being evaluated",
    "Servers other than Jellyfin",
    "Book reading and audiobook playback",
    "Admin: HTTPS and proxy editing, access schedules",
    "Live TV, DVR, DLNA, casting and gapless music, not planned",
  ];
  const rough = [
    "HDR in the pop-out window needs an HDR display and mpv 0.42 or newer; it has been tried on one Linux HDR setup so far. In the window, HDR is tone-mapped to SDR.",
    "Windows (x64) and macOS (Apple Silicon) are previews on the GitHub release: built and smoke-tested on GitHub's machines, not yet tried on a real PC or Mac, and unsigned, so the system warns on first start. Video plays in mpv's own window there.",
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
    <header class="reveal"><h2 id="limits-h" class="h2">Not in this cut</h2><p class="lede mt-4">{shipped("0.4.0") ? `Early software, version ${VERSION}.` : "Version 0.4 is being finished."} Here is the honest list.</p></header>
    <div class="mt-12 grid gap-12 md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] md:gap-16">
      <div><h3 class="role">Not implemented</h3><ul class="tick stagger mt-5">{#each missing as m}<li>{m}</li>{/each}</ul></div>
      <div><h3 class="role">Rough edges</h3><ul class="tick stagger mt-5">{#each rough as m}<li>{m}</li>{/each}</ul></div>
    </div>
  </div>
</section>
