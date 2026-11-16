<script>
  import Seo from "#lib/components/Seo.svelte";
  import { shipped } from "#lib/site.js";
  import Hero from "#lib/components/Hero.svelte";
  import Tour from "#lib/components/Tour.svelte";
  import How from "#lib/components/How.svelte";
  import Safety from "#lib/components/Safety.svelte";
  import Install from "#lib/components/Install.svelte";
  import Faq from "#lib/components/Faq.svelte";
  import Limits from "#lib/components/Limits.svelte";
</script>

<Seo title="NOCTURNE: a native cinematic media center for Linux" description={shipped("0.3.0")
  ? "NOCTURNE is a native media center for Linux: local folders and several Jellyfin servers in one cinematic, searchable library. In-window GPU video through libmpv. Never changes your files; read-only toward servers by default."
  : "NOCTURNE is a native, cinematic media center for Linux with in-window GPU video through libmpv. Version 0.3, in development, brings local folders and several Jellyfin servers into one library. Read-only by default."} />
<Hero />
<Tour />
<How />
<Safety />
<Install />
<Faq />
<Limits />
