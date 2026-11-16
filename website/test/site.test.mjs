// Builds the site (RELEASED false and true) with vite and checks the emitted files.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { before, test } from "node:test";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ALLOWED = new Set(["nocturne.ifsvivek.in", "jellyfin.org", "www.gpui.rs", "iced.rs", "github.com", "mpv.io", "www.rust-lang.org", "www.w3.org", "www.sitemaps.org"]);

function build(released, version) {
  const out = join(mkdtempSync(join(tmpdir(), "nocturne-site-")), "site");
  const r = spawnSync(process.execPath, [join(ROOT, "node_modules/vite/bin/vite.js"), "build"], {
    cwd: ROOT, encoding: "utf8", env: { ...process.env, SITE_OUT: out, NOCTURNE_RELEASED: String(released), ...(version ? { NOCTURNE_VERSION: version } : {}) },
  });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  return out;
}
const files = (d) => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? files(join(d, f)) : [join(d, f)]));
const read = (dir, f) => readFileSync(join(dir, f), "utf8");
let off, on, prev;
// prev: the same site while VERSION was 0.2.0, when every new-in-0.3 claim must carry its "Coming in 0.3" mark.
before(() => { off = build(false); on = build(true); prev = build(true, "0.2.0"); });

test("RELEASED=false: no repo or release links, coming soon", () => {
  for (const f of ["index.html", "privacy.html", "404.html"]) {
    const h = read(off, f);
    assert.doesNotMatch(h, /github\.com\/ifsvivek/i, f);
    assert.doesNotMatch(h, /releases/i, f);
  }
  assert.match(read(off, "index.html"), /Coming soon/);
  // Full product content shows while unreleased; only links and download buttons are gated.
  const h = read(off, "index.html");
  for (const s of ["Scene 1", "Behind the scenes", "Your files are never touched. Nothing on a server changes until you allow it.", "One library", "alternate sources", "Do I need a Jellyfin server?", "Coming soon to Linux", "Technical specifications", "Questions", "srcset=", "Scene 4", "Who's watching?", "Use as default profile", "Default profile", "Scene 6"]) assert.ok(h.includes(s), s);
  assert.doesNotMatch(h, /makepkg|install\.sh|cargo build/);
});

test("RELEASED=true: download, repo and version", () => {
  const h = read(on, "index.html");
  assert.ok(h.includes("https://github.com/ifsvivek/nocturne/releases/latest"));
  assert.ok(h.includes('href="https://github.com/ifsvivek/nocturne"'));
  assert.ok(h.includes(JSON.parse(readFileSync(join(ROOT, "site.config.json"), "utf8")).VERSION), "version from site.config.json");
  assert.ok(h.includes("makepkg -si") && h.includes("install.sh") && h.includes("cargo build --release --locked"));
  assert.ok(h.includes("https://github.com/ifsvivek/nocturne/blob/main/docs/WRITES.md"));
  assert.doesNotMatch(read(off, "index.html"), /Documentation/);
});

test("0.3 copy is marked while VERSION is older, and unmarked once it ships", () => {
  const h = read(prev, "index.html"), n = read(on, "index.html");
  // Each new-in-0.3 claim carries its mark: hero, scene, details, playback items, safety facts, specs, FAQ.
  assert.ok((h.match(/Coming in 0\.3/g) || []).length >= 12, "marks in the 0.2.0 build");
  assert.ok(h.includes("GPUI") && !h.includes(">Iced<"), "credits name the toolkit of the downloadable version");
  assert.doesNotMatch(n, /Coming in 0\.3|GPUI/);
  assert.ok(n.includes(">Iced<") && n.includes("Version 0.3.0"));
  assert.doesNotMatch(read(on, "privacy.html"), /Coming in/);
  assert.match(read(prev, "privacy.html"), /Coming in 0\.3/);
});

test("content hygiene: no em dashes, no GPU model or compositor advice, every img has alt and size", () => {
  for (const d of [off, on, prev]) for (const f of files(d).filter((f) => f.endsWith(".html"))) {
    const h = readFileSync(f, "utf8");
    assert.doesNotMatch(h, /\u2014|\u2013/, f);
    assert.doesNotMatch(h, /nvidia|nvdec|hyprland|geforce|radeon|\bRTX\b/i, f);
    for (const m of h.matchAll(/<img\b[^>]*>/g)) { assert.match(m[0], /\balt=/); assert.match(m[0], /width=/); assert.match(m[0], /height=/); }
  }
  const h = read(off, "index.html");
  assert.equal((h.match(/<h1\b/g) || []).length, 1);
  assert.ok(h.includes('href="#main"'));
  for (const f of ["manifest.webmanifest", "sitemap.xml", "robots.txt", "og.jpg"]) assert.ok(statSync(join(off, f)).size > 0, f);
});

test("required copy, no JS, no inline style", () => {
  const h = read(off, "index.html");
  for (const s of ["YOUR MEDIA. YOUR NIGHT.", "not affiliated with or endorsed by Jellyfin", "GPL-3.0-or-later", "Copyright (C) 2026 ifsvivek", 'rel="canonical" href="https://nocturne.ifsvivek.in/"', "og:image", "og.jpg", "twitter:card"])
    assert.ok(h.includes(s), s);
  for (const d of [off, on, prev]) for (const f of files(d).filter((f) => f.endsWith(".html"))) {
    assert.doesNotMatch(readFileSync(f, "utf8"), /<script\b|<style\b|\sstyle=/i, f);
  }
  // Other platforms may only be named as planned, never as available.
  for (const m of h.matchAll(/\b(Windows|macOS)\b/g)) assert.match(h.slice(m.index, m.index + 160), /planned/, `${m[0]} named without "planned"`);
  // One self-hosted font, preloaded, and the stylesheet points at the same hashed file.
  const font = h.match(/<link rel="preload" href="([^"]+\.woff2)" as="font"/)?.[1];
  assert.ok(font && existsSync(join(off, font)), "preloaded font file");
  assert.ok(files(off).filter((f) => f.endsWith(".css")).some((f) => readFileSync(f, "utf8").includes(font)), "css uses the preloaded font");
});

test("no external origins except upstream credit links", () => {
  for (const d of [off, on, prev]) for (const f of files(d).filter((f) => /\.(html|css|svg|xml|txt)$/.test(f))) {
    for (const m of readFileSync(f, "utf8").replace(/\/\*![\s\S]*?\*\//g, "").matchAll(/https?:\/\/([^\s"'<>)\\/]+)/g)) assert.ok(ALLOWED.has(m[1]), `${f}: ${m[1]}`);
  }
});

test("no personal data patterns", () => {
  const bad = /\/home\/|\/Users\/|[\w.+-]+@[\w-]+\.[a-z]{2,}|(?:api[_-]?key|secret|token|password)\s*[=:]\s*\S{8,}|\b[a-f0-9]{32,}\b|account_id/i;
  for (const f of files(on).filter((f) => /\.(html|css|svg|xml|txt)$/.test(f))) assert.doesNotMatch(readFileSync(f, "utf8"), bad, f);
});

test("_headers carries the strict CSP and no-transform", () => {
  const h = read(off, "_headers");
  assert.match(h, /Content-Security-Policy: default-src 'none'; style-src 'self'; manifest-src 'self'; img-src 'self' data:; font-src 'self';/);
  assert.match(h, /frame-ancestors 'none'/);
  assert.doesNotMatch(h, /unsafe-eval|unsafe-inline/);
  for (const k of ["Strict-Transport-Security", "Referrer-Policy: no-referrer", "X-Content-Type-Options: nosniff", "Permissions-Policy", "no-transform"]) assert.ok(h.includes(k), k);
});

test("size budget", () => {
  const css = files(off).filter((f) => f.endsWith(".css"));
  assert.equal(css.length, 1, "one stylesheet");
  const total = [join(off, "index.html"), ...css].reduce((n, f) => n + gzipSync(readFileSync(f)).length, 0);
  assert.ok(total <= 60 * 1024, `home html+css gzip ${total}`);
  assert.ok(statSync(join(off, "og.jpg")).size <= 250 * 1024, "og.jpg stays under the 300 kB some scrapers allow");
  // Hero requests: eager images only; everything else lazy.
  const imgs = [...read(off, "index.html").matchAll(/<img\b[^>]*>/g)].map((m) => m[0]);
  assert.equal(imgs.filter((i) => /loading="eager"/.test(i)).length, 1, "only the opening still loads eagerly");
});

test("images: every srcset file exists, 16:9 stills at high resolution, no upscaling, sizes everywhere", () => {
  const h = read(off, "index.html");
  const imgs = [...h.matchAll(/<img\b[^>]*>/g)].map((m) => m[0]).filter((i) => i.includes("srcset="));
  assert.ok(imgs.length >= 14, `images ${imgs.length}`);
  let top = 0;
  for (const i of imgs) {
    const w = +i.match(/\bwidth="(\d+)"/)[1], ht = +i.match(/\bheight="(\d+)"/)[1];
    const set = [...i.match(/srcset="([^"]+)"/)[1].matchAll(/(\S+) (\d+)w/g)].map((m) => ({ url: m[1], w: +m[2] }));
    assert.ok(set.length >= 2 && /\bsizes="/.test(i), i);
    for (const c of set) {
      const f = join(off, c.url.replace(/^\//, ""));
      assert.ok(existsSync(f), f);
      const b = readFileSync(f);
      const pw = b.toString("latin1", 12, 16) === "VP8 " ? b.readUInt16LE(26) & 0x3fff : 0;
      assert.equal(pw, c.w, `${c.url} declared ${c.w}w`);
      assert.ok(c.w <= 3840);
      top = Math.max(top, c.w);
    }
    assert.equal(Math.max(...set.map((c) => c.w)), w, "width attribute is the largest variant");
    assert.ok(ht > 0);
  }
  assert.equal(top, 3840, "native 3840 variant is served for the full-bleed stills");
  // Full-bleed stills and the opening get 3840; the opening is the one fetchpriority=high image.
  assert.equal(imgs.filter((i) => /fetchpriority="high"/.test(i)).length, 1);
  assert.equal(imgs.filter((i) => /loading="lazy"/.test(i)).length, imgs.length - 1);
  for (const f of ["picker-crop", "settings-dropdown", "account-menu", "first-run", "sources", "local-folders", "loading", "alternates-crop"]) assert.ok(imgs.some((i) => new RegExp(`${f}-\\d+\\.`).test(i)), f);
});
