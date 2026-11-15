import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const out = process.env.SITE_OUT || "build";

export default defineConfig({
  plugins: [
    tailwindcss(),
    sveltekit({
      // Every page is prerendered with csr=false: zero JavaScript ships, so the CSP has no script source.
      adapter: adapter({ pages: out, assets: out, strict: true }),
      paths: { relative: false },
      // One inlined stylesheet, no render-blocking request; the worker adds its hash to the CSP.
      inlineStyleThreshold: Infinity,
    }),
  ],
  build: { assetsInlineLimit: 0 },
});
