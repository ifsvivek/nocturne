<script>
  import Section from "./Section.svelte";
  const items = [
    ["Linux only.", "Nothing else is built or supported."],
    ["Wayland first.", "In-window video needs Wayland; X11 uses an external mpv window."],
    ["Jellyfin only", "as a media source for now."],
    ["Not included:", "Live TV, SyncPlay, DLNA, casting and gapless music playback."],
    ["Basic pages:", "photo viewer and book pages are plain detail pages; music and other media types have less depth than movies and shows."],
    ["Admin gaps:", "backups, editable branding and networking, and some library and transcoding settings are not editable yet."],
    ["Still being verified live:", "several newer player features (resume prompt, A-B loop, audio device list, external subtitles) and the offline and re-sign-in handling."],
    ["Pinned UI toolkit:", "GPUI is pre-1.0 and the video embedding depends on its internals, so it is version-pinned."],
  ];
</script>

<Section id="limits" no="06" tag="LIMITATIONS" title="What it does not do yet" blurb="Early software. Here is the honest list.">
  <ul class="m-0 list-none overflow-hidden rounded-2xl border border-line bg-surface p-0">
    {#each items as [b, t], i}
      <li class="px-[22px] py-4 text-ink-2 {i ? 'border-t border-line' : ''}"><b class="font-medium text-ink">{b}</b> {t}</li>
    {/each}
  </ul>
</Section>
