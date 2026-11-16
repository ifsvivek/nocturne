<script>
  import { shipped } from "#lib/site.js";
  // v: the version that ships the feature described next to this mark. Renders nothing once VERSION reaches it.
  let { v = "0.3.0", class: cls = "" } = $props();
</script>

{#if !shipped(v)}<span class="soon {cls}">Coming in {v.replace(/\.0$/, "")}</span>{/if}
