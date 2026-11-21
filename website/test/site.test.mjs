// Builds the site (RELEASED false and true) with vite and checks the emitted files.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { before, test } from "node:test";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { gzipSync } from "node:zlib";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ALLOWED = new Set(["nocturne.ifsvivek.in", "jellyfin.org", "www.gpui.rs", "iced.rs", "www.themoviedb.org", "www.tvmaze.com", "mirrors.rpmfusion.org", "github.com", "mpv.io", "www.rust-lang.org", "www.w3.org", "www.sitemaps.org"]);

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
// Site files only: repo/ holds the generated, signed package repositories (checksums, upstream XML namespaces), not site copy.
const own = (d) => files(d).filter((f) => !f.startsWith(join(d, "repo/")));
let off, on, prev, v03, v04, v051, v052, v060;
// prev: the same site while VERSION was 0.2.0, when every new-in-0.3 claim must carry its "Coming in 0.3" mark.
// v03 / v04: the page just before and just after 0.4.0 ships, for the 0.4 marks.
before(() => { off = build(false); on = build(true); prev = build(true, "0.2.0"); v03 = build(true, "0.3.0"); v04 = build(true, "0.4.0"); v051 = build(true, "0.5.1"); v052 = build(true, "0.5.2"); v060 = build(true, "0.6.0"); });

test("RELEASED=false: no repo or release links, coming soon", () => {
  for (const f of ["index.html", "privacy.html", "404.html", "repo.html"]) {
    const h = read(off, f);
    assert.doesNotMatch(h, /github\.com\/ifsvivek/i, f);
    assert.doesNotMatch(h, /releases/i, f);
  }
  assert.match(read(off, "index.html"), /Coming soon/);
  // Full product content shows while unreleased; only links and download buttons are gated.
  const h = read(off, "index.html");
  for (const s of ["Scene 1", "Behind the scenes", "Your files are never touched. Nothing on a server changes until you allow it.", "One library", "alternate sources", "Do I need a Jellyfin server?", "Coming soon to Linux", "Technical specifications", "Questions", "srcset=", "Scene 4", "Who's watching?", "Use as default profile", "Default profile", "Scene 6"]) assert.ok(h.includes(s), s);
  assert.doesNotMatch(h, /makepkg|install\.sh|cargo build/);
  assert.doesNotMatch(read(off, "repo.html"), /pacman-key|pacman -S|5204/, "repository steps only once released");
});

test("package repository page: exact pacman, apt and dnf steps, signing key, other formats planned", () => {
  const h = read(on, "repo.html").replace(/&#39;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
  for (const s of [
    "curl -fsSL https://nocturne.ifsvivek.in/repo/ARCH-GPG-KEY | sudo pacman-key --add -",
    "sudo pacman-key --lsign-key 5204B42AF768DE210D4979E397FA02EFCF1A4E3B",
    "printf '%s\\n' '' '[nocturne]' 'SigLevel = Required TrustedOnly' 'Server = https://nocturne.ifsvivek.in/repo/arch/$arch' | sudo tee -a /etc/pacman.conf",
    "sudo pacman -Syu nocturne", "sudo pacman -Syu",
    "5204 B42A F768 DE21 0D49 79E3 97FA 02EF CF1A 4E3B", "ed25519, valid until 2029-11-15",
    "CachyOS, EndeavourOS and Manjaro", 'rel="canonical" href="https://nocturne.ifsvivek.in/repo"', "Windows (x64) and macOS (Apple Silicon) are previews on the",
    // apt and dnf, exactly as in the root README's Install section.
    "curl -fsSL https://nocturne.ifsvivek.in/repo/NOCTURNE-GPG-KEY.gpg | sudo tee /usr/share/keyrings/nocturne.gpg >/dev/null",
    "echo 'deb [signed-by=/usr/share/keyrings/nocturne.gpg] https://nocturne.ifsvivek.in/repo/deb stable main' | sudo tee /etc/apt/sources.list.d/nocturne.list",
    "sudo apt update && sudo apt install nocturne", "gpg --show-keys /usr/share/keyrings/nocturne.gpg",
    "sudo dnf config-manager addrepo --from-repofile=https://nocturne.ifsvivek.in/repo/rpm/nocturne.repo",
    "sudo dnf install nocturne", "sudo dnf config-manager --add-repo https://nocturne.ifsvivek.in/repo/rpm/nocturne.repo", "dnf-plugins-core",
    "https://nocturne.ifsvivek.in/repo/NOCTURNE-GPG-KEY",
  ]) assert.ok(h.includes(s), s);
  assert.equal((h.match(/5204 B42A F768 DE21 0D49 79E3 97FA 02EF CF1A 4E3B/g) || []).length, 1, "one fingerprint for all three repositories");
  // One-file alternative: download from the release, install with the distribution's tool; the file names follow the version.
  for (const [d, ver] of [[v03, "0.4.0"], [v04, "0.4.0"], [v051, "0.5.1"]]) {
    const t = read(d, "repo.html").replace(/&#39;/g, "'").replace(/&amp;/g, "&");
    const rel = `https://github.com/ifsvivek/nocturne/releases/download/v${ver}`;
    for (const x of [`href="${rel}/nocturne_${ver}_amd64.deb"`, `sudo apt install ./nocturne_${ver}_amd64.deb`,
      "sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm",
      `href="${rel}/nocturne-${ver}-1.fc45.x86_64.rpm"`, `sudo dnf install ./nocturne-${ver}-1.fc45.x86_64.rpm`, "Debian 13, Ubuntu 24.04 and newer"]) assert.ok(t.includes(x), x);
  }
  assert.doesNotMatch(h, /\.exe|\.msi|\.dmg/, "no Windows or macOS build offered");
  assert.ok(read(on, "index.html").includes('href="/repo"'), "Install and footer link the page");
  assert.ok(read(on, "sitemap.xml").includes("https://nocturne.ifsvivek.in/repo"));
});

test("RELEASED=true: download, repo and version", () => {
  const h = read(on, "index.html");
  assert.ok(h.includes("https://github.com/ifsvivek/nocturne/releases/latest"), "tarball link in Install");
  assert.match(h, /href="\/repo"[^>]*>Download</, "Download opens the package repository page");
  assert.doesNotMatch(h, /releases\/latest"[^>]*>Download</, "no Download button points at GitHub");
  assert.ok(h.includes('href="https://github.com/ifsvivek/nocturne"'));
  assert.ok(h.includes(JSON.parse(readFileSync(join(ROOT, "site.config.json"), "utf8")).VERSION), "version from site.config.json");
  assert.ok(h.includes("makepkg -si") && h.includes("install.sh") && h.includes("cargo build --release --locked"));
  assert.ok(h.includes("https://github.com/ifsvivek/nocturne/blob/main/docs/WRITES.md"));
  assert.doesNotMatch(read(off, "index.html"), /Documentation/);
});

test("0.3 copy is marked while VERSION is older, and unmarked once it ships", () => {
  const h = read(prev, "index.html"), n = read(v03, "index.html");
  // Each new-in-0.3 claim carries its mark: hero, scene, details, playback items, safety facts, specs, FAQ.
  assert.ok((h.match(/Coming in 0\.3/g) || []).length >= 12, "marks in the 0.2.0 build");
  assert.ok(h.includes("GPUI") && !h.includes(">Iced<"), "credits name the toolkit of the downloadable version");
  assert.doesNotMatch(n, /Coming in 0\.3|GPUI/);
  assert.ok(n.includes(">Iced<") && n.includes("Version 0.3.0"));
  assert.doesNotMatch(read(v03, "privacy.html"), /Coming in 0\.3/);
  assert.match(read(prev, "privacy.html"), /Coming in 0\.3/);
});

test("0.4 copy is marked before 0.4.0 and unmarked from it; credits wherever a service is named", () => {
  const h = read(v03, "index.html"), n = read(v04, "index.html");
  assert.ok((h.match(/Coming in 0\.4/g) || []).length >= 15, "marks in the 0.3.0 build");
  for (const s of ["Living room", "Across services", "HDR pop-out window", "mpv 0.42 or newer", "Smart collections", "second source", "Included in".toLowerCase()]) assert.ok(h.toLowerCase().includes(s.toLowerCase()), s);
  for (const f of ["index.html", "repo.html", "privacy.html"]) assert.doesNotMatch(read(v04, f), /Coming in 0\.4/, f);
  assert.ok(n.includes("Version 0.4.0"));
  for (const d of [v03, v04, on]) for (const f of files(d).filter((f) => f.endsWith(".html"))) {
    const t = readFileSync(f, "utf8").replace(/<head>[\s\S]*<\/head>/, "");
    if (/TMDB/.test(t)) assert.ok(t.includes("This product uses the TMDB API but is not endorsed or certified by TMDB."), `${f}: TMDB notice`);
    if (/TVmaze/.test(t)) assert.ok(t.includes("TV data from TVmaze.com, licensed CC BY-SA 4.0."), `${f}: TVmaze credit`);
    if (/Trakt/.test(t)) assert.ok(t.includes("Trakt is a trademark of Trakt, Inc. NOCTURNE uses the Trakt API but is not affiliated with or endorsed by Trakt."), `${f}: Trakt credit`);
  }
});

test("0.5 copy is marked before 0.5.0 and unmarked from it; Wayland only", () => {
  const h = read(v04, "index.html"), n = read(v051, "index.html");
  assert.ok((h.match(/Coming in 0\.5/g) || []).length >= 8, "marks in the 0.4.0 build");
  assert.match(read(v04, "privacy.html"), /Coming in 0\.5/);
  for (const f of ["index.html", "repo.html", "privacy.html"]) assert.doesNotMatch(read(v051, f), /Coming in/, f);
  assert.ok(n.includes("Version 0.5.1") && n.includes("Early software, version 0.5.1."));
  for (const s of ["Your look", "Marquee and Feed", "with no browser inside", "a Jellyfin web theme applies only partly", "Letter spacing is read but not drawn", "Server UI", "branding CSS",
    "Settings, Sources, Sections", "Every text field edits like any other", "Connect Trakt", "Download and install, then Restart to update", "PREFIX=~/.local", "ed25519 signature",
    "X11 sessions are not supported", "In-app update on macOS is planned", "hover zoom has not been tried with a real pointer", "Server UI has not been tried with several servers"]) assert.ok(n.includes(s), s);
  // Linux is Wayland-only from 0.5: X11 is named only as unsupported.
  for (const f of ["index.html", "repo.html", "privacy.html"]) for (const m of read(v051, f).matchAll(/X11/g)) assert.match(read(v051, f).slice(m.index - 40, m.index + 60), /not supported|needs Wayland/, f);
  assert.match(read(v051, "privacy.html"), /Once a day it asks this site/, "privacy names the update check");
});

test("0.5.2 copy appears only from 0.5.2, without a mark, and replaces the 0.5 copy it outdates", () => {
  const text = (d, f) => read(d, f).replace(/&#39;/g, "'").replace(/&amp;/g, "&");
  const NEW = {
    "index.html": ["real flex, grid and positioning", "Theme Studio", "NOCTURNE and Glass", "Spotlight, Marquee, Feed and Classic", "Paste a Jellyfin theme", "Fetched safely", "Your server's own look",
      "Most community Jellyfin themes look as they do in the browser", "One server's UI over another", "Ctrl+Shift+0", "backdrop blur becomes a translucent fill", "Can I use my Jellyfin theme?",
      "Update now", "PackageKit", "video inside the app window in the Windows preview", "no longer freezes when playback starts", "Play all and Shuffle in the library toolbar do nothing", "has not run on a Mac yet", "now updates itself in the app, as the Windows preview does"],
    "privacy.html": ["turns all theme downloads off", "the addresses a theme you add loads its files from"],
  };
  const OLD = ["a Jellyfin web theme applies only partly", "Letter spacing is read but not drawn", "In-app update on macOS is planned", "Server UI has not been tried with several servers",
    "Package installs never update themselves", "nothing in it runs or loads", "with video in mpv's own window;"];
  for (const [f, list] of Object.entries(NEW)) for (const s of list) {
    assert.ok(text(v052, f).includes(s), `0.5.2: ${s}`);
    assert.ok(!text(v051, f).includes(s), `0.5.1 must not show: ${s}`);
  }
  for (const s of OLD) { assert.ok(text(v051, "index.html").includes(s), `0.5.1: ${s}`); assert.ok(!text(v052, "index.html").includes(s), `0.5.2 drops: ${s}`); }
  for (const f of ["index.html", "repo.html", "privacy.html"]) assert.doesNotMatch(read(v052, f), /Coming in/, f);
  assert.ok(read(v052, "index.html").includes("Early software, version 0.5.2."));
});

test("0.6.0 copy appears only from 0.6.0, without a mark", () => {
  const text = (d, f) => read(d, f).replace(/&#39;/g, "'").replace(/&amp;/g, "&");
  const NEW = ["Calendar and requests", "Requests with Seerr", "Themes gallery", "Choose where video plays", "Can I request movies?", "Can I use VLC?",
    "Flatpak is not available", "MPC-HC for the Windows preview and IINA for the macOS preview are not supported yet", "Requests for 4K versions", "A request goes only to your Seerr server"];
  for (const s of NEW) {
    assert.ok(text(v060, "index.html").includes(s), `0.6.0: ${s}`);
    assert.ok(!text(v052, "index.html").includes(s), `0.5.2 must not show: ${s}`);
  }
  assert.ok(text(v052, "index.html").includes("Flatpak is being evaluated"));
  assert.ok(!text(v060, "index.html").includes("Flatpak is being evaluated"));
  assert.ok(!/Coming in/.test(read(v060, "index.html")));
  assert.ok(read(v060, "index.html").includes("Early software, version 0.6.0."));
});

test("content hygiene: no em dashes, no GPU model or compositor advice, every img has alt and size", () => {
  for (const d of [off, on, prev, v04, v052, v060]) for (const f of files(d).filter((f) => f.endsWith(".html"))) {
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
  for (const d of [off, on, prev, v04, v052, v060]) for (const f of files(d).filter((f) => f.endsWith(".html"))) {
    assert.doesNotMatch(readFileSync(f, "utf8"), /<script\b|<style\b|\sstyle=/i, f);
  }
  // Other platforms may only be named as previews or planned, never as finished (0.4 ships unsigned Windows and macOS previews on the GitHub release).
  for (const t of [h, read(on, "index.html"), read(on, "repo.html"), read(on, "privacy.html"), read(v04, "index.html"), read(v052, "index.html"), read(v052, "privacy.html")]) for (const m of t.matchAll(/\b(Windows|macOS)\b/g)) assert.match(t.slice(m.index, m.index + 160), /planned|preview/, `${m[0]} named without "preview" or "planned"`);
  // One self-hosted font, preloaded, and the stylesheet points at the same hashed file.
  const font = h.match(/<link rel="preload" href="([^"]+\.woff2)" as="font"/)?.[1];
  assert.ok(font && existsSync(join(off, font)), "preloaded font file");
  assert.ok(files(off).filter((f) => f.endsWith(".css") && !f.startsWith(join(off, "themes/"))).some((f) => readFileSync(f, "utf8").includes(font)), "css uses the preloaded font");
});

test("no external origins except upstream credit links", () => {
  for (const d of [off, on, prev, v04, v052, v060]) for (const f of own(d).filter((f) => /\.(html|css|svg|xml|txt)$/.test(f))) {
    for (const m of readFileSync(f, "utf8").replace(/\/\*![\s\S]*?\*\//g, "").matchAll(/https?:\/\/([^\s"'<>)\\/]+)/g)) assert.ok(ALLOWED.has(m[1]), `${f}: ${m[1]}`);
  }
});

test("no personal data patterns", () => {
  const bad = /\/home\/|\/Users\/|[\w.+-]+@[\w-]+\.[a-z]{2,}|(?:api[_-]?key|secret|token|password)\s*[=:]\s*\S{8,}|\b[a-f0-9]{32,}\b|account_id/i;
  // The repository signing key id is public by design.
  for (const f of own(on).filter((f) => /\.(html|css|svg|xml|txt)$/.test(f))) assert.doesNotMatch(readFileSync(f, "utf8").replaceAll("5204B42AF768DE210D4979E397FA02EFCF1A4E3B", ""), bad, f);
});

test("_headers carries the strict CSP and no-transform", () => {
  const h = read(off, "_headers");
  assert.match(h, /Content-Security-Policy: default-src 'none'; style-src 'self'; manifest-src 'self'; img-src 'self' data:; font-src 'self';/);
  assert.match(h, /frame-ancestors 'none'/);
  assert.doesNotMatch(h, /unsafe-eval|unsafe-inline/);
  for (const k of ["Strict-Transport-Security", "Referrer-Policy: no-referrer", "X-Content-Type-Options: nosniff", "Permissions-Policy", "no-transform"]) assert.ok(h.includes(k), k);
  // Repository files: each rule comes after the global block, detaches its Cache-Control and sets one value.
  const rule = (path) => h.match(new RegExp(`^${path.replace(/[.*]/g, (c) => "\\" + c)}\\n  ! Cache-Control\\n  Cache-Control: ([^\\n]+)`, "m"))?.[1];
  for (const p of ["/repo/ARCH-GPG-KEY", "/repo/*.db", "/repo/*.db.sig", "/repo/*.files", "/repo/*.files.sig"]) assert.equal(rule(p), "no-cache, no-transform", p);
  for (const p of ["/repo/*.pkg.tar.zst", "/repo/*.pkg.tar.zst.sig"]) assert.equal(rule(p), "public, max-age=31536000, immutable, no-transform", p);
  for (const p of ["/repo/NOCTURNE-GPG-KEY*", "/repo/deb/dists/*", "/repo/rpm/nocturne.repo"]) assert.equal(rule(p), "no-cache, no-transform", p);
  for (const p of ["/repo/deb/pool/*", "/repo/rpm/*.rpm"]) assert.equal(rule(p), "public, max-age=31536000, immutable, no-transform", p);
  assert.ok(h.lastIndexOf("/repo/rpm/repodata/repomd.xml*") > h.indexOf("/repo/rpm/repodata/*\n"), "repomd.xml (no-cache) overrides the immutable repodata rule");
  assert.ok(h.indexOf("/repo/") > h.indexOf("/*\n"), "repo rules follow the global rule");
});

test("size budget", () => {
  const css = files(off).filter((f) => f.endsWith(".css") && !f.startsWith(join(off, "themes/")));
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

test("themes gallery: the index is valid and every file is served and matches its checksum", () => {
  const idx = JSON.parse(read(on, "themes/index.json"));
  assert.equal(idx.version, 1);
  assert.ok(idx.themes.length >= 7);
  const ids = new Set();
  for (const t of idx.themes) {
    assert.match(t.id, /^[a-z0-9][a-z0-9-]{0,39}$/);
    assert.ok(!ids.has(t.id), `duplicate ${t.id}`); ids.add(t.id);
    for (const k of ["name", "description", "author", "licence", "min_version"]) assert.ok(t[k] && typeof t[k] === "string", `${t.id} ${k}`);
    assert.ok(["official", "community"].includes(t.kind), t.id);
    // Relative addresses on the site's own host: the app refuses anything else.
    assert.equal(t.css, `${t.id}/theme.css`); assert.equal(t.preview, `${t.id}/preview.webp`);
    const css = readFileSync(join(on, "themes", t.css));
    assert.equal(createHash("sha256").update(css).digest("hex"), t.sha256, t.id);
    assert.equal(css.length, t.size); assert.ok(css.length <= 256 * 1024);
    const prev = readFileSync(join(on, "themes", t.preview));
    assert.ok(prev.length < 150 * 1024, `${t.id} preview is ${prev.length} bytes`);
    assert.equal(prev.subarray(0, 4).toString(), "RIFF"); assert.equal(prev.subarray(8, 12).toString(), "WEBP");
  }
  assert.ok(idx.themes.filter((t) => t.kind === "official").length >= 7);
});
