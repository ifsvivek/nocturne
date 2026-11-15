<script>
  import { shot } from "#lib/site.js";
  // name: screenshot key; sizes: the <img> sizes attribute; fit: extra <img> classes (object-cover crops in fixed tiles).
  let { name, sizes = "100vw", eager = false, fit = "", class: cls = "", children } = $props();
  const s = shot(name);
</script>

<figure class="frame {cls}">
  <img class={fit} src={s.src} srcset={s.srcset} {sizes} alt={s.alt} width={s.width} height={s.height}
    loading={eager ? "eager" : "lazy"} fetchpriority={eager ? "high" : "auto"} decoding="async">
  {@render children?.()}
</figure>
