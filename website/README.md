# NOCTURNE website

Static site for `nocturne.ifsvivek.in`: SvelteKit (Svelte 5, `adapter-static`, every page prerendered with `csr = false`, so no JavaScript ships) and Tailwind CSS v4, served by a Cloudflare Worker with static assets. One self-hosted font, no third-party requests.

- `src/routes/` pages (`/`, `/privacy`, `/404`). The home page is built as a film: Hero (the opening, letterbox bars around the app playing a film, then opening credits), Tour (four scenes: a large still from the app each, copy set as a subtitle), How ("Behind the scenes": the player window as three real layers), Safety (a film rating card, "Read-only", plus the real setting), Install ("Coming soon to Linux", requirements as technical specifications), Faq, Limits ("Not in this cut") and Footer (end credits). `src/app.css` holds the tokens, credit typography, film grain and all motion.
- Look: true black, projector-white type, wide-tracked uppercase for titles and credits, the app's blue only for links, focus and the one marker on the OFF switch. Stills have a hairline edge and 3 px corners, no glows.
- Type: Geist variable (SIL OFL 1.1, licence in `src/lib/fonts/OFL.txt`), latin subset woff2 from the Fontsource package, `font-display: swap`, preloaded from `+layout.svelte`; Vite hashes it into `_app/immutable/`. Code uses the system monospace.
- Motion is CSS only and limited to four ideas: the opening screen opens and the title settles, the credits and scenes roll in on scroll, the playback scene's letterbox opens, and Behind the scenes loops on a 12 s timer (the player window folds flat, then splits into video, window and controls layers with labels; hover pauses it). A focus ring also locks onto the real OFF switch. Content is visible by default: scroll-driven parts sit inside `@supports (animation-timeline: view())` and everything is static under `prefers-reduced-motion`. Avoid `filter` in text keyframes (headless Chrome left blurred text unpainted), write `animation-*` longhands when there is no name in the shorthand (the minifier turns a nameless shorthand into `animation: none`), and keep 3D labels out of elements with `mix-blend-mode` (it flattens `preserve-3d`).
- `static/` favicon, `og.jpg`, `manifest.webmanifest`, `robots.txt`, `sitemap.xml`, and `_headers` (strict CSP, HSTS, Referrer-Policy, `no-transform` so Cloudflare does not rewrite HTML). The CSP adds `manifest-src 'self'` for the manifest and `font-src 'self'` for the font.
- `wrangler.jsonc` serves `build/` on the custom domain.

## RELEASED flag

`site.config.json`: `{ "RELEASED": false, "VERSION": "0.1.0" }`, read at build time. `false` still shows all product content and screenshots, but with "Coming soon", no download buttons, no install commands and no link to or mention of the GitHub repo or releases. `true` adds Download, the repo link, version, install commands (Arch, tarball, source) and links to the docs in the repo. `NOCTURNE_RELEASED=true|false` overrides it for one build. Keep it `false` in the committed file until the first release.

## Screenshots and images

Demo-library WebP captures live in `src/lib/screens/NN-name.webp` (1920x1200); `NN-name` maps to the keys in `src/lib/site.js` (`shot("home")`, alt text there). `python3 scripts/make-assets.py` (needs Pillow) writes the 480/960/1440 px variants to `src/lib/screens/sized/` (used for `srcset`), cuts the close-ups listed in `CROPS` (search palette, the server-changes setting, the player control bar, the admin dashboard) from the same captures, and rebuilds `static/og.jpg` from the real screenshots. Re-run it after replacing a screenshot. The opening still loads eagerly with `fetchpriority=high`; all others are lazy, and every `<img>` has width and height.

## Commands

```sh
npm ci
npm run build    # vite build -> build/
npm test         # node --test: builds both RELEASED variants and checks the output
npm run deploy   # build + wrangler deploy (maintainer only)
```

Deploy with `CLOUDFLARE_ACCOUNT_ID` and an API token in the environment; nothing account-specific is in this repo.

## Performance

No JavaScript, no third-party requests. Home page: about 8.5 kB gzip HTML, 8.6 kB gzip CSS, 29 kB font, then images on demand: at 1440 px wide the hero pair is 16 kB and the whole page 134 kB (403 kB on a 2x screen); a 390 px phone at 3x loads about 190 kB. Re-measure with Lighthouse after deploy.

Preview locally: `npm run build && python3 -m http.server -d build`, then screenshot with `google-chrome-stable --headless=new --force-prefers-reduced-motion --disable-features=LazyImageLoading --screenshot=out.png --window-size=1440,11000 http://127.0.0.1:8000/` (headless time can freeze the load-in animations, and lazy images do not load in one tall capture without that flag). To see motion, drive Chrome over its DevTools protocol (scroll, wait in real time, `Page.captureScreenshot`); anchors and `--virtual-time-budget` do not reliably advance it.
