<script>
  import { VERSION } from "#lib/site.js";
  const missing = ["Live TV, SyncPlay, DLNA and casting", "Gapless music playback", "Playable books and audiobooks", "Some admin editing"];
  const rough = [
    "Photo viewer and book pages are plain detail pages. Music and other media types have less depth than movies and shows.",
    "Multi-season real libraries, HDR and the photo viewer have not been seen on real-server data yet.",
    "GPUI is pre-1.0 and the video embedding depends on its internals, so it is version-pinned.",
  ];
</script>

<section id="limits" class="pb-[clamp(88px,11vw,168px)]" aria-labelledby="limits-h">
  <div class="wrap">
    <div class="reveal relative isolate overflow-hidden rounded-[1.75rem] bg-[linear-gradient(160deg,rgb(75_85_224/0.16),rgb(12_14_22/0.6)_55%)] p-[clamp(28px,5vw,72px)] ring-1 ring-white/10 ring-inset">
      <h2 id="limits-h" class="h2">What it does not do yet</h2>
      <p class="lede mt-5">Early software, version {VERSION}. Here is the honest list.</p>
      <div class="mt-12 grid gap-12 md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] md:gap-16">
        <div><h3 class="text-[1.06rem] font-semibold">Not implemented</h3><ul class="tick stagger mt-5">{#each missing as m}<li>{m}</li>{/each}</ul></div>
        <div><h3 class="text-[1.06rem] font-semibold">Rough edges</h3><ul class="tick stagger mt-5">{#each rough as m}<li>{m}</li>{/each}</ul></div>
      </div>
    </div>
  </div>
</section>
