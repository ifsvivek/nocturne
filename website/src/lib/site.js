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
const shots = import.meta.glob("/src/lib/screens/*.webp", { eager: true, query: "?url", import: "default" });
export const SCREENS = Object.entries(shots)
  .sort(([a], [b]) => a.localeCompare(b))
  .map(([path, src]) => {
    const { width, height } = webpSize(readFileSync(process.cwd() + path));
    const name = path.split("/").pop().replace(/\.webp$/, "").replace(/^\d+[-_]?/, "").replace(/[-_]+/g, " ").trim();
    return { src, width, height, alt: `Screenshot: ${name}` };
  });
