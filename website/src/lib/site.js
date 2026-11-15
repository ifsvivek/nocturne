import { readFileSync } from "node:fs";
import cfg from "../../site.config.json";

// Read at build time (prerender). RELEASED env overrides the file so tests can build both variants.
const env = typeof process !== "undefined" ? process.env.NOCTURNE_RELEASED : undefined;
export const RELEASED = env !== undefined ? env === "true" : cfg.RELEASED === true;
export const VERSION = String(cfg.VERSION ?? "0.1.0");
export const SITE = "https://nocturne.ifsvivek.in";
export const REPO = "https://github.com/ifsvivek/nocturne";
export const RELEASES = `${REPO}/releases/latest`;

// Width/height from the WebP header (VP8, VP8L, VP8X) so every <img> reserves its space.
function webpSize(b) {
  const t = b.toString("latin1", 12, 16);
  if (t === "VP8X") return { width: 1 + b.readUIntLE(24, 3), height: 1 + b.readUIntLE(27, 3) };
  if (t === "VP8L") { const v = b.readUInt32LE(21); return { width: (v & 0x3fff) + 1, height: ((v >> 14) & 0x3fff) + 1 }; }
  return { width: b.readUInt16LE(26) & 0x3fff, height: b.readUInt16LE(28) & 0x3fff };
}
const shots = import.meta.glob("/src/lib/screens/**/*.webp", { eager: true, query: "?url", import: "default" });
// key ("home", "playback", ...) -> every width variant, smallest first. Originals are NN-name.webp, resized copies NN-name-W.webp.
const bySize = {};
for (const [path, url] of Object.entries(shots)) {
  const { width, height } = webpSize(readFileSync(process.cwd() + path));
  let key = path.split("/").pop().replace(/\.webp$/, "").replace(/^\d+-/, "");
  if (path.includes("/sized/")) key = key.replace(/-\d+$/, "");
  (bySize[key] ??= []).push({ url, width, height });
}
const ALT = {
  home: "NOCTURNE home screen: a hero banner and a Continue watching row",
  library: "A movie library grid with sort, filters and an A-Z rail",
  "movie-detail": "A movie detail page with cast, chapters and media info",
  "series-episodes": "A series page with season tabs and an episode card grid",
  search: "Global search results",
  playback: "In-window playback with the player controls drawn over the video",
  settings: "Settings, including the switch that allows changes to the server",
  "admin-dashboard": "The administrator dashboard inside the same app",
  "search-crop": "The search palette listing matching movies, shows and episodes",
  "settings-crop": "The setting Allow NOCTURNE to change data on the server, switched off",
  "video-crop": "The video picture on its own, without the player controls",
  "controls-crop": "The player control bar: seek, play, chapters, audio, subtitles and volume",
  "admin-crop": "Admin dashboard: server details, library counts, sessions and recent activity",
};
// Image props for one screenshot: src (largest), srcset, intrinsic size and alt, so layout never shifts.
export function shot(key) {
  const v = (bySize[key] ?? []).sort((a, b) => a.width - b.width);
  if (!v.length) throw new Error(`missing screenshot: ${key}`);
  const big = v[v.length - 1];
  return { src: big.url, srcset: v.map((x) => `${x.url} ${x.width}w`).join(", "), width: big.width, height: big.height, alt: ALT[key] ?? key };
}
