# NOCTURNE website

Static site for `nocturne.ifsvivek.in`: SvelteKit (Svelte 5, `adapter-static`, every page prerendered with `csr = false`, so no JavaScript ships) and Tailwind CSS v4, served by a Cloudflare Worker with static assets. One self-hosted font, no third-party requests.

- `src/routes/` pages (`/`, `/repo`, `/privacy`, `/404`). `/repo` is the package repository page: the pacman steps (key import, the `pacman.conf` block, install) and the signing key's fingerprint, kept in one place at the top of `repo/+page.svelte` so a change to the key or the `SigLevel` line is one edit; the tests check the exact text. The repository files themselves are generated into the git-ignored `static/repo/` at deploy time, and `_headers` gives them their own `Cache-Control` (databases, signatures and the key `no-cache`; packages and their signatures immutable for a year), detaching the site-wide value and keeping `no-transform`. The home page is built as a film: Hero (the opening, letterbox bars around the app playing a film, then opening credits), Tour (seven scenes: One library, then Browse, Details, Playback, Profiles, Settings, Admin; One library is a procedural figure of the sources running into one slit of light, echoing the app's startup screen, the others a large still from the app each, copy set as a subtitle), How ("Behind the scenes": the player window as three real layers), Safety (a film rating card, "Read-only", your files never touched, plus the real setting), Install ("Coming soon to Linux", requirements as technical specifications), Faq, Limits ("Not in this cut") and Footer (end credits). `src/app.css` holds the tokens, credit typography, film grain and all motion.
- Look: true black, projector-white type, wide-tracked uppercase for titles and credits, the app's blue only for links, focus and the one marker on the OFF switch. Stills have a hairline edge and 3 px corners, no glows.
- Type: Geist variable (SIL OFL 1.1, licence in `src/lib/fonts/OFL.txt`), latin subset woff2 from the Fontsource package, `font-display: swap`, preloaded from `+layout.svelte`; Vite hashes it into `_app/immutable/`. Code uses the system monospace.
- Motion is CSS only and limited to five ideas: the opening screen opens and the title settles, the credits and scenes roll in on scroll, the One library figure assembles on scroll (an SVG mask wipes the source beams in, then the moonlight slit opens and the flare line runs out to "One library"), the playback scene's letterbox opens, and Behind the scenes loops on a 12 s timer (the player window folds flat, then splits into video, window and controls layers with labels; hover pauses it). A focus ring also locks onto the real OFF switch. Content is visible by default: scroll-driven parts sit inside `@supports (animation-timeline: view())` and everything is static under `prefers-reduced-motion`. Avoid `filter` in text keyframes (headless Chrome left blurred text unpainted), write `animation-*` longhands when there is no name in the shorthand (the minifier turns a nameless shorthand into `animation: none`), and keep 3D labels out of elements with `mix-blend-mode` (it flattens `preserve-3d`). Do not draw SVG lines with `stroke-dashoffset` when they use `vector-effect: non-scaling-stroke` and `pathLength`: Chrome computes the dashes in screen space and a second dash shows; the One library figure uses a mask wipe instead.
- `static/` favicon, `og.jpg`, `manifest.webmanifest`, `robots.txt`, `sitemap.xml`, and `_headers` (strict CSP, HSTS, Referrer-Policy, `no-transform` so Cloudflare does not rewrite HTML). The CSP adds `manifest-src 'self'` for the manifest and `font-src 'self'` for the font.
- `wrangler.jsonc` serves `build/` on the custom domain.

## Version marks

The page can describe the next version while `VERSION` still names the one you can download. With 0.3.0 released the 0.3 marks are retired; the mechanism stays for the next version. Every claim that was new in 0.3 carries a "Coming in 0.3" mark (`Next.svelte`, `<Next v="0.3.0" />`, or a version in the copy arrays of Tour, Safety, Install, Faq and How), and the interface credit names the toolkit of the downloadable version (`UI` in `site.js`: GPUI before 0.3.0, Iced from it). There is no extra config value: `shipped(v)` compares `VERSION` to the feature's version, so bumping `VERSION` at release removes every mark for it, and `NOCTURNE_VERSION=0.2.0 npm run build` shows the marked page. `npm test` builds that variant too and checks that the marks appear there and nowhere in the current build.

## RELEASED flag

`site.config.json`: `{ "RELEASED": true, "VERSION": "0.3.0" }`, read at build time. `false` still shows all product content and screenshots, but with "Coming soon", no download buttons, no install commands and no link to or mention of the GitHub repo or releases. `true` adds Download, the repo link, version, install commands (Arch, tarball, source) and links to the docs in the repo. `NOCTURNE_RELEASED=true|false` overrides it for one build. `VERSION` names the release the download buttons and install commands point at (tag `v{VERSION}`).

## Screenshots and images

Full-size captures live in `src/lib/screens/*.webp`: 3840x2160 (16:9, scale-2 fullscreen), lossy WebP q97, `NN-name` mapping to the keys in `src/lib/site.js` (`shot("home")`, alt text there). They are sources only and are not bundled. `python3 scripts/make-assets.py` (needs Pillow; uses ImageMagick `magick` for sharp-YUV encoding when present) rebuilds `src/lib/screens/sized/NAME-W.webp` at 640/1280/1920/2560 px and the native 3840 (never upscaled), WebP q92 method 6. Widths are capped by slot: home and playback (full-bleed, up to 1680 css px) go to 3840, the framed stills to 2560, close-ups to what their slot needs. Close-ups (search palette, the server-changes setting with its OFF switch, the "Who's watching?" tiles, the player video and control bar, the Versions and sources picker) are cut from the captures with boxes given as fractions in `CROPS`, so they survive a resolution change; look at them after a recapture. The script also rebuilds `static/og.jpg` (1200x630, drawn at 2x, q90, about 75 kB so WhatsApp-style 300 kB limits are met). `picker.webp`, `05-search.webp` and `03-movie-detail.webp` are only used through their close-ups, so no full-frame variants are made. The source filter is the All sources button in the header; it shows in the home and library stills and has no close-up of its own.

The captures are from the 0.3 (Iced) build: `09-first-run` (setup, Where is your media?), `10-sources`, `11-local-folders` (One library) and `12-loading` (the video loading overlay, in Playback and the OG image) joined the earlier set.

AVIF was measured against WebP q92 (3840 px: playback 879 vs 934 kB, home 286 vs 329 kB, at equal or lower PSNR and softer UI text) and not adopted: under 15 percent smaller does not justify a `<picture>` fallback. The opening still loads eagerly with `fetchpriority=high`; all others are lazy, every `<img>` has width and height, and `sizes` in `Tour.svelte` mirror the real slot widths.

The settings capture shows the server-changes switch OFF, as on a fresh install, and the Safety close-up's reticle locks onto that switch (`.reticle` in `app.css` holds its position as fractions of the close-up; move it if the crop changes).

## Commands

```sh
npm ci
npm run build    # vite build -> build/
npm test         # node --test: builds both RELEASED variants and checks the output
npm run deploy   # build + wrangler deploy (maintainer only)
```

Deploy with `CLOUDFLARE_ACCOUNT_ID` and an API token in the environment; nothing account-specific is in this repo.

## Performance

No JavaScript, no third-party requests. Home page: about 8.5 kB gzip HTML, 8.6 kB gzip CSS, 29 kB font. Images measured in headless Chrome (fresh profile): initial load 382 kB at 1440 px or 1920 px wide at 1x (hero 1920 px variant, 284 kB), 1.2 MB at 1440 px 2x (hero 3840 px, 1.1 MB), 335 kB on a 390 px phone at 3x; scrolling the whole page loads 1.1 MB (1440 px, 1x), 2.9 MB (1440 px, 2x), 0.9 MB (390 px, 3x). No horizontal overflow from 320 to 1920 px. Re-measure with Lighthouse after deploy.

Preview locally: `npm run build && python3 -m http.server -d build`, then screenshot with `google-chrome-stable --headless=new --force-prefers-reduced-motion --disable-features=LazyImageLoading --screenshot=out.png --window-size=1440,11000 http://127.0.0.1:8000/` (headless time can freeze the load-in animations, and lazy images do not load in one tall capture without that flag). To see motion, drive Chrome over its DevTools protocol (scroll, wait in real time, `Page.captureScreenshot`); anchors and `--virtual-time-budget` do not reliably advance it.
