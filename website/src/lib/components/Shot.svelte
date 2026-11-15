<script>
  import { shot } from "#lib/site.js";
  // name: screenshot key; sizes: the <img> sizes attribute; bleed: edge-to-edge scene instead of a framed still.
  let { name, sizes = "100vw", eager = false, bleed = false, class: cls = "", children } = $props();
  const s = shot(name);
</script>

<figure class="{bleed ? 'bleed' : 'still'} {cls}">
  <img src={s.src} srcset={s.srcset} {sizes} alt={s.alt} width={s.width} height={s.height}
    loading={eager ? "eager" : "lazy"} fetchpriority={eager ? "high" : "auto"} decoding="async">
  {@render children?.()}
</figure>
