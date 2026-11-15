<script>
  import { RELEASED, VERSION } from "#lib/site.js";
  let { class: cls = "" } = $props();
</script>

<p class="chip {cls}"><i></i>{#if RELEASED}v{VERSION} &middot; Linux{:else}Coming soon &middot; Linux{/if}</p>
