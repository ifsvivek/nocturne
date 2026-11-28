// Modules whose routes are documented by their own workers as they land (gateway, tunnel, discovery, sources, storage).
// Their pages read the module state from /status and render whatever the module's status route returns; edit ROUTES when the contract fixes the paths.
import { h, table } from "./ui.js";
import { getOr, probe } from "./api.js";

export const ROUTES = {
  gateway: { status: "/gateway/status", restart: "/gateway/restart" },
  tunnel: { status: "/tunnel/status" },
  discovery: { list: "/discovery/servers" },
  sources: { list: "/sources" },
  storage: { list: "/storage/mounts" },
};

export async function modules() {
  const s = await getOr("/status", null);
  return new Map((s?.modules || []).map((m) => [m.name, m]));
}

/** Badge for a module: Off, Error, Running, Starting, or Not installed. */
export function moduleBadge(m) {
  if (!m) return h("span.badge", "Not in this server");
  if (!m.enabled) return h("span.badge", "Off");
  if (m.error) return h("span.badge.danger", "Error");
  return m.running ? h("span.badge.accent", "Running") : h("span.badge.warn", "Starting");
}

const SECRET = /token|secret|password|passphrase|api_?key|^key$/i;
const label = (k) => k.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase());

function scalar(k, v) {
  if (SECRET.test(k)) return h("span.faint", "hidden");
  if (v === null || v === undefined || v === "") return h("span.faint", "–");
  if (typeof v === "boolean") return v ? "Yes" : "No";
  return String(v);
}

/** Any JSON as definition lists and tables, secrets hidden. */
export function generic(data) {
  if (Array.isArray(data)) {
    if (!data.length) return h("p.muted", "None.");
    if (data.every((x) => x && typeof x === "object" && !Array.isArray(x))) {
      const keys = [...new Set(data.flatMap((x) => Object.keys(x)))].filter((k) => data.some((x) => typeof x[k] !== "object" || x[k] === null));
      return table(keys.map(label), data.map((x) => h("tr", keys.map((k) => h("td.wrap-any", scalar(k, x[k]))))));
    }
    return h("p", data.map((x) => scalar("", x)).join(", "));
  }
  if (data && typeof data === "object") {
    const flat = Object.entries(data).filter(([, v]) => v === null || typeof v !== "object");
    const nested = Object.entries(data).filter(([, v]) => v !== null && typeof v === "object");
    return h("div.stack",
      flat.length ? h("dl.kv", flat.map(([k, v]) => [h("dt", label(k)), h("dd", scalar(k, v))])) : null,
      nested.map(([k, v]) => h("section.stack", h("h3", label(k)), generic(v))));
  }
  return h("p", scalar("", data));
}

/** A card for one module: its state, then `render(data)` of `path` (or a note when the module is off or the route is not there yet). */
export async function moduleCard(mods, name, title, path, render = generic) {
  const m = mods.get(name);
  const head = h("div.row", h("h3", title), h("span.spacer"), moduleBadge(m));
  if (!m || !m.enabled) {
    return h("div.card", head, h("p.muted", m ? "Switched off. Turn it on in Settings." : "This server version has no such module."),
      m ? h("a", { href: "/settings", "data-link": true }, "Settings") : null);
  }
  const r = await probe(path);
  return h("div.card", head,
    m.error ? h("div.error-box", { role: "alert" }, m.error) : null,
    r.state === "ok" ? render(r.data)
      : r.state === "off" ? h("p.muted", "The module is not running.")
        : h("p.muted", "Running, but this server version does not report details yet."));
}
