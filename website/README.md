# NOCTURNE website

Static site for `nocturne.ifsvivek.in`: SvelteKit (Svelte 5) prerendered with `adapter-static`, Tailwind CSS v4, packed into a single Cloudflare Worker. No JavaScript ships to the browser (`csr = false` everywhere), no third-party requests, system fonts only.

## Structure

- `src/routes/` pages: `/`, `/privacy`, `/404` (all `prerender`, `csr = false`).
- `src/lib/components/` Svelte sections (Hero, BrandVisual, HowItWorks, Features, Safety, Screens, Install, Limitations, Header, Footer, StatusChip, Seo).
- `src/app.css` Tailwind v4 `@theme` tokens mirroring `crates/app/src/theme.rs`.
- `static/` favicon (from `packaging/`), `og.png` (rendered from `brand/og.svg`), `robots.txt`, `sitemap.xml`.
- `scripts/build.mjs` embeds the `vite build` output into `dist/worker.js` (+ `dist/metadata.json`): plain (identity) bodies only, never `Content-Encoding` (Cloudflare compresses text at the edge), strong content-hash ETags, 304, HEAD, 404, `/index.html` -> `/` redirect, security headers, and a per-page CSP whose `style-src` is the sha256 of the inlined stylesheet (no `unsafe-inline`, no script source).
- `test/site.test.mjs` builds both release variants and tests the worker in-process.

## RELEASED flag

`site.config.json`:

```json
{ "RELEASED": false, "VERSION": "0.1.0" }
```

It is read at build time. `false` shows "Coming soon" and emits no link to the GitHub repo or releases. `true` shows the Download button, repo link, version chip and install commands. `NOCTURNE_RELEASED=true|false` overrides the file for a single build.

## Screenshots

Drop WebP files in `src/lib/screens/` (for example `01-home.webp`, sorted by name; the file name becomes the alt text). Width/height are read from the file so no layout shift. Until then the gallery shows a "Screenshots arrive with the first release" placeholder. Use sanitized demo-library captures only.

## Commands

```sh
npm ci
npm run build   # vite build, then dist/worker.js + dist/metadata.json
npm test        # node --test (builds twice, so it takes a few seconds)
```

## Deployment

Done by the maintainer through the Cloudflare API using `dist/worker.js` and `dist/metadata.json`. No account IDs or tokens live in this repository.

## Performance

Local Lighthouse 13 run against the built worker served on 127.0.0.1 (headless Chromium):

| Profile | Performance | Accessibility | Best Practices | SEO | LCP | CLS | TBT |
|---|---|---|---|---|---|---|---|
| Mobile (emulated) | 100 | 100 | 100 | 100 | 1.0 s | 0 | 0 ms |
| Desktop | 100 | 100 | 100 | 100 | 0.3 s | 0 | 0 ms |

Locally the worker serves the plain body, so the home page transfers 54.7 kB of HTML (CSS inlined) plus a 0.3 kB SVG logo, 0 kB JavaScript; the mobile run above is therefore slower than production. With edge compression that is roughly 13.3 kB gzip or 11.3 kB brotli (measured with node:zlib at build time, never served). `og.png` is 49 kB and only fetched by link previews. `dist/worker.js` is about 216 kB.
