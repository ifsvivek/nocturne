// Builds the site twice (RELEASED false/true) with vite, packs each into a Worker and exercises it in-process.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { before, describe, test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { pack } from "../scripts/build.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ALLOWED_HOSTS = new Set([
  "nocturne.ifsvivek.in", "jellyfin.org", "www.gpui.rs", "github.com", "mpv.io", "www.rust-lang.org",
  "www.w3.org", "www.sitemaps.org", // xmlns / sitemap namespaces, never requested
]);
const VOID = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);

function build(released) {
  const dir = mkdtempSync(join(tmpdir(), "nocturne-site-"));
  const site = join(dir, "site");
  const r = spawnSync(process.execPath, [join(ROOT, "node_modules/vite/bin/vite.js"), "build"], {
    cwd: ROOT,
    env: { ...process.env, SITE_OUT: site, NOCTURNE_RELEASED: String(released) },
    encoding: "utf8",
  });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const out = pack({ siteDir: site, outDir: join(dir, "dist") });
  return { ...out, site, dir };
}
const load = async (b) => (await import(pathToFileURL(join(b.outDir, "worker.js")).href + "?" + Math.random())).default;
const get = (w, path, init = {}) => w.fetch(new Request("https://nocturne.ifsvivek.in" + path, init));
const text = async (res) => {
  assert.equal(res.headers.get("content-encoding"), null, "worker must never set Content-Encoding");
  return Buffer.from(await res.arrayBuffer()).toString("utf8");
};
const hosts = (s) => [...s.matchAll(/https?:\/\/([^\s"'<>)\\/]+)/g)].map((m) => m[1]);

function wellFormed(html, name) {
  const body = html.replace(/<!--[\s\S]*?-->/g, "").replace(/<(style)\b[\s\S]*?<\/\1>/g, "<style></style>");
  const stack = [];
  for (const m of body.matchAll(/<(\/?)([a-zA-Z][\w:-]*)\b[^>]*?(\/?)>/g)) {
    const [, close, tag, self] = m;
    const t = tag.toLowerCase();
    if (self || VOID.has(t)) continue;
    if (close) assert.equal(stack.pop(), t, `${name}: unbalanced </${t}>`);
    else stack.push(t);
  }
  assert.deepEqual(stack, [], `${name}: unclosed ${stack}`);
}

let off, on, wOff, wOn;
before(async () => {
  off = build(false);
  on = build(true);
  wOff = await load(off);
  wOn = await load(on);
});

describe("worker responses", () => {
  test("pages and static files", async () => {
    for (const [p, type] of [["/", "text/html"], ["/privacy", "text/html"], ["/robots.txt", "text/plain"], ["/sitemap.xml", "application/xml"], ["/og.png", "image/png"], ["/icon.svg", "image/svg+xml"]]) {
      const r = await get(wOff, p);
      assert.equal(r.status, 200, p);
      assert.ok(r.headers.get("content-type").startsWith(type), p);
      assert.ok(r.headers.get("etag"), p);
    }
  });
  test("404, 405, redirects, HEAD", async () => {
    const nf = await get(wOff, "/definitely-not-here");
    assert.equal(nf.status, 404);
    assert.match(await text(nf), /Lost in the dark/);
    assert.equal((await get(wOff, "/404")).status, 404);
    assert.equal((await get(wOff, "/_app/nope.js")).status, 404);
    assert.equal((await get(wOff, "/", { method: "POST" })).status, 405);
    for (const [from, to] of [["/index.html", "/"], ["/privacy/", "/privacy"]]) {
      const r = await get(wOff, from);
      assert.equal(r.status, 301);
      assert.equal(r.headers.get("location"), to);
    }
    const h = await get(wOff, "/", { method: "HEAD" });
    assert.equal(h.status, 200);
    assert.equal((await h.arrayBuffer()).byteLength, 0);
    assert.equal(h.headers.get("etag"), (await get(wOff, "/")).headers.get("etag"));
    assert.equal((await get(wOff, "/nope", { method: "HEAD" })).status, 404);
  });
  test("always plain bodies whatever Accept-Encoding says; ETag/304; cache headers", async () => {
    const base = await get(wOff, "/");
    const id = await text(base);
    assert.ok(id.startsWith("<!doctype html>"));
    for (const ae of ["identity", "gzip", "br", "gzip, deflate, br", "br;q=1.0, gzip;q=0.8"]) {
      const r = await get(wOff, "/", { headers: { "accept-encoding": ae } });
      assert.equal(await text(r), id, ae);
      assert.equal(r.headers.get("etag"), base.headers.get("etag"));
      assert.equal(r.headers.get("vary"), null);
    }
    assert.match(base.headers.get("etag"), /^"[0-9a-f]{12}"$/);
    assert.match(base.headers.get("cache-control"), /max-age=300, must-revalidate/);
    assert.equal((await get(wOff, "/", { headers: { "if-none-match": base.headers.get("etag") } })).status, 304);
    // binaries are real bytes: PNG signature, exact length
    const png = Buffer.from(await (await get(wOff, "/og.png", { headers: { "accept-encoding": "br" } })).arrayBuffer());
    assert.equal(png.subarray(1, 4).toString(), "PNG");
    assert.equal(png.length, readFileSync(join(off.site, "og.png")).length);
  });
  test("security headers and a CSP that matches the inline style", async () => {
    const r = await get(wOff, "/");
    const csp = r.headers.get("content-security-policy");
    assert.match(csp, /default-src 'none'/);
    assert.match(csp, /frame-ancestors 'none'/);
    assert.doesNotMatch(csp, /unsafe-eval|unsafe-inline|\*/);
    assert.doesNotMatch(csp, /script-src/); // no JS ships; default-src 'none' blocks scripts
    for (const h of ["x-content-type-options", "referrer-policy", "permissions-policy", "strict-transport-security"]) assert.ok(r.headers.get(h), h);
    const html = await text(r);
    const styles = [...html.matchAll(/<style\b[^>]*>([\s\S]*?)<\/style>/g)];
    assert.ok(styles.length >= 1, "css is inlined");
    for (const [, css] of styles) assert.ok(csp.includes(`'sha256-${createHash("sha256").update(css).digest("base64")}'`));
    assert.doesNotMatch(html, /<script\b/i);
    assert.doesNotMatch(html, /\sstyle=/i);
    assert.doesNotMatch(html, /<link[^>]+stylesheet/i);
  });
});

describe("content", () => {
  test("served page is the SvelteKit prerender output", async () => {
    const built = readFileSync(join(off.site, "index.html"), "utf8");
    const served = await text(await get(wOff, "/"));
    const style = built.replace(/\/\*![\s\S]*?\*\//g, "").match(/<style\b[^>]*>[\s\S]*?<\/style>/)[0];
    assert.ok(served.includes(style));
    assert.ok(served.includes("<title>NOCTURNE: a native cinematic media client for Linux</title>"));
    assert.ok(Object.keys(off.table).every((p) => !p.endsWith(".js")), "no JavaScript assets");
  });
  test("RELEASED=false hides every repo/download reference", async () => {
    for (const p of ["/", "/privacy", "/nope"]) {
      const html = await text(await get(wOff, p));
      assert.doesNotMatch(html, /github\.com\/ifsvivek/i, p);
      assert.doesNotMatch(html, /releases/i, p);
      assert.match(html, /Coming soon/i);
    }
    assert.match(await text(await get(wOff, "/")), /Screenshots arrive with the first release/);
  });
  test("RELEASED=true shows download, repo and version", async () => {
    const html = await text(await get(wOn, "/"));
    assert.ok(html.includes("https://github.com/ifsvivek/nocturne/releases/latest"));
    assert.ok(html.includes('href="https://github.com/ifsvivek/nocturne"'));
    assert.match(html, /v0\.1\.0/);
    assert.doesNotMatch(html, /Coming soon/i);
  });
  test("required copy and meta", async () => {
    const html = await text(await get(wOff, "/"));
    for (const s of [
      'lang="en"', 'rel="canonical" href="https://nocturne.ifsvivek.in/"', "og:image", "twitter:card", 'name="color-scheme"',
      "YOUR MEDIA. YOUR NIGHT.", "A native cinematic media experience for Linux.",
      "not affiliated with or endorsed by Jellyfin", "GPL-3.0-or-later", "Copyright (C) 2026 ifsvivek",
      "https://jellyfin.org", "https://www.gpui.rs", "https://github.com/zed-industries/zed", "https://mpv.io", "https://www.rust-lang.org",
    ]) assert.ok(html.includes(s), s);
    assert.doesNotMatch(html.replace(/<style[\s\S]*?<\/style>/g, ""), /\b(Windows|macOS|MacOS|Mac OS)\b/);
    const privacy = await text(await get(wOff, "/privacy"));
    assert.match(privacy, /no cookies/);
    assert.match(privacy, /keyring/);
  });
  test("HTML is well formed", async () => {
    for (const p of ["/", "/privacy", "/404"]) wellFormed(await text(await get(wOff, p)), p);
  });
});

describe("hygiene", () => {
  test("no external origins beyond the allowed upstream links", async () => {
    for (const b of [off, on]) {
      const w = b === off ? wOff : wOn;
      for (const p of ["/", "/privacy", "/404", "/sitemap.xml", "/robots.txt", "/icon.svg"]) {
        const body = await text(await get(w, p));
        for (const h of hosts(body)) assert.ok(ALLOWED_HOSTS.has(h), `${p}: ${h}`);
        if (b === off) assert.ok(!hosts(body).includes("github.com") || /zed-industries\/zed/.test(body), p);
      }
    }
    const html = await text(await get(wOff, "/"));
    // every subresource reference is same-origin
    for (const m of html.matchAll(/<(?:img|link|script|source)\b[^>]*?(?:src|href)="([^"]+)"/g)) {
      if (/rel="(canonical)"/.test(m[0])) continue;
      assert.ok(!/^(https?:)?\/\//.test(m[1]) || m[1].startsWith("https://nocturne.ifsvivek.in/"), m[1]);
    }
    assert.doesNotMatch(html, /url\(\s*["']?https?:/i);
  });
  test("no personal data patterns", async () => {
    const bad = /\/home\/|\/Users\/|[A-Za-z]:\\\\|[\w.+-]+@[\w-]+\.[a-z]{2,}|(?:api[_-]?key|secret|token|password)\s*[=:]\s*\S{8,}|\b[a-f0-9]{32,}\b|\.local\b/i;
    for (const p of ["/", "/privacy", "/404"]) assert.doesNotMatch(await text(await get(wOn, p)), bad, p);
    assert.doesNotMatch(on.worker, /\/home\/|account_id|api[_-]?token/i);
  });
  test("size budget", async () => {
    assert.ok(off.bytes < 2.5 * 1024 * 1024, `worker ${off.bytes}`);
    assert.ok(off.sizes["/"].gzip <= 25 * 1024, `home gzip ${off.sizes["/"].gzip}`);
    assert.ok(off.sizes["/og.png"].raw <= 100 * 1024);
  });
  test("metadata.json", () => {
    const m = JSON.parse(readFileSync(join(off.outDir, "metadata.json"), "utf8"));
    assert.equal(m.name, "nocturne-website");
    assert.equal(m.main_module, "worker.js");
    assert.match(m.compatibility_date, /^\d{4}-\d{2}-\d{2}$/);
  });
  test("workerd (only if installed)", { skip: spawnSync("which", ["workerd"]).status !== 0 }, () => {});
});
