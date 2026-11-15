<script>
  import { shot } from "#lib/site.js";
  // name: screenshot key; sizes: the <img> sizes attribute; fit: extra classes to crop (object-cover) in fixed-height tiles.
  let { name, sizes = "100vw", eager = false, hover = true, fit = "", class: cls = "" } = $props();
  const s = shot(name);
</script>

<figure class="frame {hover ? 'frame-hover' : ''} {cls}">
  <img class={fit} src={s.src} srcset={s.srcset} {sizes} alt={s.alt} width={s.width} height={s.height}
    loading={eager ? "eager" : "lazy"} fetchpriority={eager ? "high" : "auto"} decoding={eager ? "sync" : "async"}>
</figure>
