<script>
  let { id, no, tag, title, blurb = "", children } = $props();
</script>

<section class="border-t border-line py-[clamp(56px,8vw,104px)] [content-visibility:auto] [contain-intrinsic-size:auto_640px]" {id} aria-labelledby="{id}-h">
  <div class="wrap grid gap-8 min-[900px]:grid-cols-[260px_minmax(0,1fr)] min-[900px]:gap-16">
    <div class="reveal min-[900px]:sticky min-[900px]:top-[88px] min-[900px]:self-start">
      <span class="label">{no} / {tag}</span>
      <h2 id="{id}-h" class="mt-2.5 text-[clamp(1.5rem,2.6vw,2rem)] font-medium leading-tight tracking-tight">{title}</h2>
      {#if blurb}<p class="mt-3.5 text-[0.94rem] text-ink-2">{blurb}</p>{/if}
    </div>
    <div class="reveal min-w-0">{@render children()}</div>
  </div>
</section>
