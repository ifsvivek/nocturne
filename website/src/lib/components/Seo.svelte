<script>
  import { SITE } from "#lib/site.js";
  let { title, description, path = "/", robots = "index, follow" } = $props();
  const canonical = SITE + path;
  const og = SITE + "/og.jpg";
</script>

<svelte:head>
  <title>{title}</title>
  <meta name="description" content={description}>
  <meta name="robots" content={robots}>
  <link rel="canonical" href={canonical}>
  <link rel="icon" href="/icon.svg" type="image/svg+xml">
  <link rel="icon" href="/favicon-48.png" type="image/png" sizes="48x48">
  <link rel="apple-touch-icon" href="/icon-256.png">
  <link rel="manifest" href="/manifest.webmanifest">
  <meta property="og:type" content="website">
  <meta property="og:site_name" content="NOCTURNE">
  <meta property="og:title" content={title}>
  <meta property="og:description" content={description}>
  <meta property="og:url" content={canonical}>
  <meta property="og:image" content={og}>
  <meta property="og:image:width" content="1200">
  <meta property="og:image:height" content="630">
  <meta property="og:image:alt" content="NOCTURNE, a native media center for Linux: the home screen and the in-window player. Your media. Your night.">
  <meta name="twitter:card" content="summary_large_image">
  <meta name="twitter:title" content={title}>
  <meta name="twitter:description" content={description}>
  <meta name="twitter:image" content={og}>
</svelte:head>
