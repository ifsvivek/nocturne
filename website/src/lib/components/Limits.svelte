<script>
  const missing = ["Live TV, SyncPlay, DLNA and casting", "Gapless music playback", "Playable books and audiobooks", "Some admin editing"];
  const rough = [
    "Photo viewer and book pages are plain detail pages. Music and other media types have less depth than movies and shows.",
    "Multi-season real libraries, HDR and the photo viewer have not been seen on real-server data yet.",
    "GPUI is pre-1.0 and the video embedding depends on its internals, so it is version-pinned.",
  ];
</script>

<section id="limits" class="border-t border-line py-[clamp(56px,8vw,112px)] [content-visibility:auto] [contain-intrinsic-size:auto_420px]" aria-labelledby="limits-h">
  <div class="wrap">
    <h2 id="limits-h" class="h2 reveal">What it does not do yet</h2>
    <p class="lede reveal mt-4">Early software, version 0.1.0. Here is the honest list.</p>
    <div class="mt-10 grid gap-10 min-[900px]:grid-cols-2 min-[900px]:gap-16">
      <div class="reveal"><h3 class="mb-4 text-lg font-medium">Not implemented</h3><ul class="tick">{#each missing as m}<li>{m}</li>{/each}</ul></div>
      <div class="reveal"><h3 class="mb-4 text-lg font-medium">Rough edges</h3><ul class="tick">{#each rough as m}<li>{m}</li>{/each}</ul></div>
    </div>
  </div>
</section>
