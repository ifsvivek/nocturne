<script>
  import "../app.css";
  import Header from "#lib/components/Header.svelte";
  import Footer from "#lib/components/Footer.svelte";
  let { children } = $props();
</script>

<a class="fixed left-3 -top-16 z-[100] rounded-lg bg-accent px-3.5 py-2 font-semibold text-on-accent no-underline focus:top-3" href="#main">Skip to content</a>
<Header />
<main id="main">{@render children()}</main>
<Footer />
