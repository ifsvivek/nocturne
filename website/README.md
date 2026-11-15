# NOCTURNE website

Static site for `nocturne.ifsvivek.in`: SvelteKit (Svelte 5, `adapter-static`, every page prerendered with `csr = false`, so no JavaScript ships) and Tailwind CSS v4, served by a Cloudflare Worker with static assets. System fonts only, no third-party requests.

- `src/routes/` pages (`/`, `/privacy`, `/404`); `src/lib/components/` sections; `src/app.css` design tokens from `crates/app/src/theme.rs`.
- `static/` favicon, `og.png` (from `brand/og.svg`), `robots.txt`, `sitemap.xml`, and `_headers` (strict CSP, HSTS, Referrer-Policy, `no-transform` so Cloudflare does not rewrite HTML).
- `wrangler.jsonc` serves `build/` on the custom domain.

## RELEASED flag

`site.config.json`: `{ "RELEASED": false, "VERSION": "0.1.0" }`, read at build time. `false` shows "Coming soon" and emits no link to the GitHub repo or releases; `true` shows Download, repo link, version and install commands. `NOCTURNE_RELEASED=true|false` overrides it for one build.

## Screenshots

Put WebP files in `src/lib/screens/` (e.g. `01-home.webp`; name becomes the alt text). Until then a placeholder shows. Sanitized demo-library captures only.

## Commands

```sh
npm ci
npm run build    # vite build -> build/
npm test         # node --test: builds both RELEASED variants and checks the output
npm run deploy   # build + wrangler deploy (maintainer only)
```

Deploy with `CLOUDFLARE_ACCOUNT_ID` and an API token in the environment; nothing account-specific is in this repo.

## Performance

Local Lighthouse (before the move to Workers static assets, same pages): mobile and desktop 100 / 100 / 100 / 100, CLS 0, TBT 0 ms, no JavaScript. Home page is about 13 kB gzip of HTML plus about 7 kB gzip of CSS. Re-measure after deploy.
