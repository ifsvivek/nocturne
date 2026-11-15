<script>
  import { VERSION } from "#lib/site.js";
  const missing = ["Live TV, SyncPlay, DLNA and casting", "Gapless music playback", "Playable books and audiobooks", "Some admin editing"];
  const rough = [
    "Photo viewer and book pages are plain detail pages. Music and other media types have less depth than movies and shows.",
    "Multi-season real libraries, HDR and the photo viewer have not been seen on real-server data yet.",
    "GPUI is pre-1.0 and the video embedding depends on its internals, so it is version-pinned.",
  ];
</script>

<section id="limits" class="section rule" aria-labelledby="limits-h">
  <div class="wrap">
    <header class="reveal"><h2 id="limits-h" class="h2">Not in this cut</h2><p class="lede mt-4">Early software, version {VERSION}. Here is the honest list.</p></header>
    <div class="mt-12 grid gap-12 md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] md:gap-16">
      <div><h3 class="role">Not implemented</h3><ul class="tick stagger mt-5">{#each missing as m}<li>{m}</li>{/each}</ul></div>
      <div><h3 class="role">Rough edges</h3><ul class="tick stagger mt-5">{#each rough as m}<li>{m}</li>{/each}</ul></div>
    </div>
  </div>
</section>
