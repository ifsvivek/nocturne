<script>
  import { VERSION, shipped } from "#lib/site.js";
  // From docs/ROADMAP.md: what 0.3 does not do, and where it is still rough. Revisit at every release.
  const missing = [
    "macOS and Windows, planned after the Linux release",
    ".deb, .rpm and AppImage packages, planned next",
    "Servers other than Jellyfin",
    "Book reading and audiobook playback",
    "Two subtitle tracks at once, and a version picker",
    "Admin: HTTPS and proxy editing, access schedules",
    "Live TV, DVR, DLNA, casting and gapless music, not planned",
  ];
  const rough = [
    "Copies without an IMDb, TMDB or TVDB id are not merged, so the same film can show twice rather than one being hidden by mistake.",
    "The local index is held in memory, which is fine for tens of thousands of files. Larger libraries are untested.",
    "Screen readers get little from the interface yet, and HDR is tone-mapped to SDR.",
    "The photo viewer and music have only been tried on a generated demo library.",
  ];
</script>

<section id="limits" class="section rule" aria-labelledby="limits-h">
  <div class="wrap">
    <header class="reveal"><h2 id="limits-h" class="h2">Not in this cut</h2><p class="lede mt-4">{shipped("0.3.0") ? `Early software, version ${VERSION}.` : "Version 0.3 is in development."} Here is the honest list.</p></header>
    <div class="mt-12 grid gap-12 md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] md:gap-16">
      <div><h3 class="role">Not implemented</h3><ul class="tick stagger mt-5">{#each missing as m}<li>{m}</li>{/each}</ul></div>
      <div><h3 class="role">Rough edges</h3><ul class="tick stagger mt-5">{#each rough as m}<li>{m}</li>{/each}</ul></div>
    </div>
  </div>
</section>
