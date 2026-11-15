// Builds the site (RELEASED false and true) with vite and checks the emitted files.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { before, test } from "node:test";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ALLOWED = new Set(["nocturne.ifsvivek.in", "jellyfin.org", "www.gpui.rs", "github.com", "mpv.io", "www.rust-lang.org", "www.w3.org", "www.sitemaps.org"]);

function build(released) {
  const out = join(mkdtempSync(join(tmpdir(), "nocturne-site-")), "site");
  const r = spawnSync(process.execPath, [join(ROOT, "node_modules/vite/bin/vite.js"), "build"], {
    cwd: ROOT, encoding: "utf8", env: { ...process.env, SITE_OUT: out, NOCTURNE_RELEASED: String(released) },
  });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  return out;
}
const files = (d) => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? files(join(d, f)) : [join(d, f)]));
const read = (dir, f) => readFileSync(join(dir, f), "utf8");
let off, on;
before(() => { off = build(false); on = build(true); });

test("RELEASED=false: no repo or release links, coming soon", () => {
  for (const f of ["index.html", "privacy.html", "404.html"]) {
    const h = read(off, f);
    assert.doesNotMatch(h, /github\.com\/ifsvivek/i, f);
    assert.doesNotMatch(h, /releases/i, f);
  }
  assert.match(read(off, "index.html"), /Coming soon/);
  // Full product content shows while unreleased; only links and download buttons are gated.
  const h = read(off, "index.html");
  for (const s of ["A tour of the app", "Read-only until you say otherwise", "Requirements and install", "Questions", "srcset="]) assert.ok(h.includes(s), s);
  assert.doesNotMatch(h, /makepkg|install\.sh|cargo build/);
});

test("RELEASED=true: download, repo and version", () => {
  const h = read(on, "index.html");
  assert.ok(h.includes("https://github.com/ifsvivek/nocturne/releases/latest"));
  assert.ok(h.includes('href="https://github.com/ifsvivek/nocturne"'));
  assert.match(h, /v0\.1\.0|0\.1\.0/);
  assert.ok(h.includes("makepkg -si") && h.includes("install.sh") && h.includes("cargo build --release --locked"));
  assert.ok(h.includes("https://github.com/ifsvivek/nocturne/blob/main/docs/WRITES.md"));
  assert.doesNotMatch(read(off, "index.html"), /Documentation/);
});

test("content hygiene: no em dashes, no GPU model or compositor advice, every img has alt and size", () => {
  for (const d of [off, on]) for (const f of files(d).filter((f) => f.endsWith(".html"))) {
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
  for (const d of [off, on]) for (const f of files(d).filter((f) => f.endsWith(".html"))) {
    assert.doesNotMatch(readFileSync(f, "utf8"), /<script\b|<style\b|\sstyle=/i, f);
  }
  assert.doesNotMatch(h, /\b(Windows|macOS)\b/);
});

test("no external origins except upstream credit links", () => {
  for (const d of [off, on]) for (const f of files(d).filter((f) => /\.(html|css|svg|xml|txt)$/.test(f))) {
    for (const m of readFileSync(f, "utf8").replace(/\/\*![\s\S]*?\*\//g, "").matchAll(/https?:\/\/([^\s"'<>)\\/]+)/g)) assert.ok(ALLOWED.has(m[1]), `${f}: ${m[1]}`);
  }
});

test("no personal data patterns", () => {
  const bad = /\/home\/|\/Users\/|[\w.+-]+@[\w-]+\.[a-z]{2,}|(?:api[_-]?key|secret|token|password)\s*[=:]\s*\S{8,}|\b[a-f0-9]{32,}\b|account_id/i;
  for (const f of files(on).filter((f) => /\.(html|css|svg|xml|txt)$/.test(f))) assert.doesNotMatch(readFileSync(f, "utf8"), bad, f);
});

test("_headers carries the strict CSP and no-transform", () => {
  const h = read(off, "_headers");
  assert.match(h, /Content-Security-Policy: default-src 'none'; style-src 'self'; manifest-src 'self'; img-src 'self' data:/);
  assert.match(h, /frame-ancestors 'none'/);
  assert.doesNotMatch(h, /unsafe-eval|unsafe-inline/);
  for (const k of ["Strict-Transport-Security", "Referrer-Policy: no-referrer", "X-Content-Type-Options: nosniff", "Permissions-Policy", "no-transform"]) assert.ok(h.includes(k), k);
});

test("size budget", () => {
  const css = files(off).filter((f) => f.endsWith(".css"));
  assert.equal(css.length, 1, "one stylesheet");
  const total = [join(off, "index.html"), ...css].reduce((n, f) => n + gzipSync(readFileSync(f)).length, 0);
  assert.ok(total <= 60 * 1024, `home html+css gzip ${total}`);
  assert.ok(statSync(join(off, "og.jpg")).size <= 100 * 1024);
  // Hero requests: eager images only; everything else lazy.
  const imgs = [...read(off, "index.html").matchAll(/<img\b[^>]*>/g)].map((m) => m[0]);
  assert.ok(imgs.filter((i) => /loading="eager"/.test(i) && /srcset/.test(i)).length === 2, "two hero images eager");
});
