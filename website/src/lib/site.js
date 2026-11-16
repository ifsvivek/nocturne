import { readFileSync } from "node:fs";
import cfg from "../../site.config.json";

// Read at build time (prerender). RELEASED env overrides the file so tests can build both variants.
const env = typeof process !== "undefined" ? process.env.NOCTURNE_RELEASED : undefined;
export const RELEASED = env !== undefined ? env === "true" : cfg.RELEASED === true;
export const VERSION = String(process.env.NOCTURNE_VERSION ?? cfg.VERSION ?? "0.1.0");
// Copy for features that ship in a later version carries a "Coming in X" mark until VERSION reaches it, so the site
// can describe the next release without claiming it is the one you download. No extra config: bumping VERSION at
// release retires every mark for that version.
const num = (v) => v.split(".").map(Number);
export function shipped(v) {
  const [a, b] = [num(VERSION), num(v)];
  for (let i = 0; i < 3; i++) if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
  return true;
}
// The interface toolkit changed in 0.3 (GPUI -> Iced); credits name the one in the version you can download.
export const UI = shipped("0.3.0") ? ["Iced", "https://iced.rs"] : ["GPUI, from Zed", "https://www.gpui.rs"];
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
// Only the derived, size-suffixed files are bundled (scripts/make-assets.py); the 3840 captures stay out of the build.
const shots = import.meta.glob("/src/lib/screens/sized/*.webp", { eager: true, query: "?url", import: "default" });
// key ("home", "playback", "picker", ...) -> every width variant. NN-name-W.webp -> name.
const bySize = {};
for (const [path, url] of Object.entries(shots)) {
  const { width, height } = webpSize(readFileSync(process.cwd() + path));
  const key = path.split("/").pop().replace(/\.webp$/, "").replace(/^\d+-/, "").replace(/-\d+$/, "");
  (bySize[key] ??= []).push({ url, width, height });
}
const ALT = {
  home: "NOCTURNE home screen: a hero banner and a Continue watching row",
  library: "A movie library grid with sort, filters and an A-Z rail",
  "movie-detail": "A movie detail page with cast, chapters and media info",
  "series-episodes": "A series page with season tabs and an episode card grid",
  search: "Global search results",
  playback: "In-window playback with the player controls drawn over the video",
  settings: "Settings: the switch that allows changes to the server, then playback options as drop-downs and switches",
  "settings-dropdown": "Settings with the audio language drop-down open, the current choice ticked",
  "picker-crop": "The Who's watching? profile picker: three round profile tiles, an Add profile tile and a Use as default profile box",
  "account-menu": "The account menu with Settings, Switch to other accounts, Switch profile, Add another account and Sign out",
  "admin-dashboard": "The administrator dashboard inside the same app",
  "search-crop": "The search palette listing matching movies, shows and episodes",
  "settings-crop": "The setting Allow NOCTURNE to change data on the server, described as off by default",
  "video-crop": "The video picture on its own, without the player controls",
  "controls-crop": "The player control bar: seek, play, chapters, audio, subtitles and volume",
  // 0.3 captures (phase B recapture). Shown only once the files exist, see has().
  "first-run": "The first-run screen offering three choices: Local library, Connect to a server, or Both",
  sources: "Settings, Sources: one local folder and two Jellyfin servers, each with its status",
  "local-folders": "The local folders screen with indexing progress and an Unmatched list",
  startup: "The startup screen: a midnight-blue city canyon, a thin vertical slit of moonlight and a progress line",
  "alternates-crop": "A detail page's Alternate sources list: the same film on a local folder and a second server, with quality and availability",
  "filter-crop": "The source filter in the library toolbar: All, Local and each server",
};
// True once a capture for this key has been made (scripts/make-assets.py), so new scenes can wait for real stills.
export const has = (key) => key in bySize;
// Image props for one screenshot: src (largest), srcset, intrinsic size and alt, so layout never shifts.
export function shot(key) {
  const v = (bySize[key] ?? []).sort((a, b) => a.width - b.width);
  if (!v.length) throw new Error(`missing screenshot: ${key}`);
  const big = v[v.length - 1];
  return { src: big.url, srcset: v.map((x) => `${x.url} ${x.width}w`).join(", "), width: big.width, height: big.height, alt: ALT[key] ?? key };
}
