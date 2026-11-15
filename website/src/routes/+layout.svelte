<script>
  import "../app.css";
  import font from "../lib/fonts/geist-latin.woff2?url";
  import Header from "#lib/components/Header.svelte";
  import Footer from "#lib/components/Footer.svelte";
  let { children } = $props();
</script>

<svelte:head>
  <link rel="preload" href={font} as="font" type="font/woff2" crossorigin="anonymous">
</svelte:head>

<a class="fixed left-3 -top-16 z-[100] rounded-full bg-ink px-4 py-2 font-semibold text-frame no-underline focus:top-3" href="#main">Skip to content</a>
<Header />
<main id="main">{@render children()}</main>
<Footer />
