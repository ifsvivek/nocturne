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
// .deb and .rpm assets on the GitHub release (names from scripts/package.sh: confirm them at each release; fc45 is the
// Fedora release the .rpm is built on). Before 0.4.0 ships they name 0.4.0 and the copy around them is marked.
const PKG_VER = shipped("0.4.0") ? VERSION : "0.4.0";
export const DEB = `nocturne_${PKG_VER}_amd64.deb`;
export const RPM = `nocturne-${PKG_VER}-1.fc45.x86_64.rpm`;
export const asset = (file) => `${REPO}/releases/download/v${PKG_VER}/${file}`;
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
  home: "NOCTURNE home screen: a hero banner, the All sources filter, and Continue watching with a source mark on every card",
  library: "A movie library grid with sort, filters, an A-Z rail and a source mark on every poster: Local, Living room or 2 sources",
  "series-episodes": "A series page with season tabs and an episode card grid",
  playback: "In-window playback with the player controls drawn over the video",
  loading: "The loading overlay: the film's backdrop, its title half filled with light and a thin line showing the buffer",
  settings: "Settings: Sources, Run setup again, the switch that allows changes to the server (off), then playback options",
  "settings-dropdown": "Settings with the audio language drop-down open, the current choice ticked",
  "picker-crop": "The Who's watching? profile picker: two round profile tiles, an Add profile tile and a Use as default profile box",
  "account-menu": "The account menu with Settings, Sources, Admin, Watch together, Add another account, Switch server and Sign out",
  "admin-dashboard": "The administrator dashboard of one server, view-only while changes are off",
  "search-crop": "The search palette: the same film listed once per source, Local, Living room and Study",
  "settings-crop": "The setting Allow NOCTURNE to change data on the server, switched off and described as off by default",
  "video-crop": "The video picture on its own, without the player controls",
  "controls-crop": "The player control bar: seek, play, chapters, audio, subtitles and volume",
  "alternates-crop": "Versions and sources: one film on three sources, Living room, Local and Study, with quality, and Always prefer this source",
  "first-run": "Setup, step 2 of 5: Where is your media? Local folders, A Jellyfin server, or Both",
  sources: "Settings, Sources: This computer with one folder, and the servers Living room and Study, all online",
  "local-folders": "Local folders: one indexed folder, counts of movies, shows and episodes, and one unmatched file to place by hand",
};
// Image props for one screenshot: src (largest), srcset, intrinsic size and alt, so layout never shifts.
export function shot(key) {
  const v = (bySize[key] ?? []).sort((a, b) => a.width - b.width);
  if (!v.length) throw new Error(`missing screenshot: ${key}`);
  const big = v[v.length - 1];
  return { src: big.url, srcset: v.map((x) => `${x.url} ${x.width}w`).join(", "), width: big.width, height: big.height, alt: ALT[key] ?? key };
}
