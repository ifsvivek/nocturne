# NOCTURNE website

Static site for `nocturne.ifsvivek.in`: SvelteKit (Svelte 5, `adapter-static`, every page prerendered with `csr = false`, so no JavaScript ships) and Tailwind CSS v4, served by a Cloudflare Worker with static assets. System fonts only, no third-party requests.

- `src/routes/` pages (`/`, `/privacy`, `/404`); `src/lib/components/` sections (Hero, Tour, How, Safety, Install, Faq, Limits); `src/app.css` design tokens from `crates/app/src/theme.rs`, film grain and all motion.
- Motion is CSS only: hero load-in cascade, slow gradient drift, scroll-driven reveal and parallax (`animation-timeline`, content simply shows where unsupported), all disabled under `prefers-reduced-motion`.
- `static/` favicon, `og.jpg`, `manifest.webmanifest`, `robots.txt`, `sitemap.xml`, and `_headers` (strict CSP, HSTS, Referrer-Policy, `no-transform` so Cloudflare does not rewrite HTML). The CSP has one addition for the manifest: `manifest-src 'self'`.
- `wrangler.jsonc` serves `build/` on the custom domain.

## RELEASED flag

`site.config.json`: `{ "RELEASED": false, "VERSION": "0.1.0" }`, read at build time. `false` still shows all product content and screenshots, but with "Coming soon", no download buttons, no install commands and no link to or mention of the GitHub repo or releases. `true` adds Download, the repo link, version, install commands (Arch, tarball, source) and links to the docs in the repo. `NOCTURNE_RELEASED=true|false` overrides it for one build. Keep it `false` in the committed file until the first release.

## Screenshots and images

Demo-library WebP captures live in `src/lib/screens/NN-name.webp` (1920x1200); `NN-name` maps to the keys in `src/lib/site.js` (`shot("home")`, alt text there). `python3 scripts/make-assets.py` (needs Pillow) writes the 480/960/1440 px variants to `src/lib/screens/sized/` (used for `srcset`) and rebuilds `static/og.jpg` from the real screenshots. Re-run it after replacing a screenshot. Hero images load eagerly with `fetchpriority=high`; all others are lazy, and every `<img>` has width and height.

## Commands

```sh
npm ci
npm run build    # vite build -> build/
npm test         # node --test: builds both RELEASED variants and checks the output
npm run deploy   # build + wrangler deploy (maintainer only)
```

Deploy with `CLOUDFLARE_ACCOUNT_ID` and an API token in the environment; nothing account-specific is in this repo.

## Performance

No JavaScript, system fonts, no third-party requests. Home page: about 7 kB gzip HTML, about 8 kB gzip CSS, then images on demand (hero pair about 33 kB at desktop width; all eight screenshots about 200 kB across the page; OG image 36 kB). Re-measure with Lighthouse after deploy.
