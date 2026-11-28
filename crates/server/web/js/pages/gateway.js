import { h, clear, act, duration, confirm, table } from "../ui.js";
import { post, onEvent } from "../api.js";
import { ROUTES, modules, moduleCard, generic } from "../modules.js";

const BINDING = { local: "This computer only", lan: "Home network (HTTP)", remote: "Internet (HTTPS)" };
export const SOURCE_STATE = { ready: ["Ready", "accent"], signed_out: ["Signed out", "warn"], keyring_locked: ["Keyring locked", "warn"], local: ["Local folders", ""] };

/** The gateway's status (`nocturne_core::gateway::Status`); anything else falls back to the generic view. */
function gatewayView(g) {
  if (!g || typeof g !== "object" || !("address" in g)) return generic(g);
  return h("div.stack",
    g.error ? h("div.error-box", { role: "alert" }, g.error) : null,
    h("p", h("span.secret", g.address)),
    h("dl.kv",
      h("dt", "Reachable from"), h("dd", BINDING[g.binding] || g.binding),
      h("dt", "Encryption"), h("dd", g.tls ? "HTTPS" : "none (HTTP)"),
      h("dt", "Listening"), h("dd.mono", (g.listening || []).join(", ") || "–"),
      h("dt", "User"), h("dd", g.user_name),
      h("dt", "Signed-in devices"), h("dd", g.devices),
      h("dt", "Uptime"), h("dd", duration(g.uptime_secs))),
    g.password_unseen ? h("p.note", "The first-start password has not been revealed yet: open Settings > Gateway in the desktop app.") : null,
    (g.pending_codes || []).length ? h("div", h("h3", "Quick Connect codes waiting"),
      h("ul.plain", g.pending_codes.map((c) => h("li", h("span.mono", c.code), " · ", c.device)))) : null,
    Array.isArray(g.sources) ? h("div.stack", h("h3", "Sources"), table(["Source", "State"], g.sources.map((s) => {
      const [l, c] = SOURCE_STATE[s.state] || [s.state, ""];
      return h("tr", h("td", s.label), h("td", h("span.badge", { class: c }, l)));
    }), "No sources.")) : null);
}

function tunnelView(t) {
  if (!t || typeof t !== "object" || !("connected" in t)) return generic(t);
  return h("dl.kv",
    h("dt", "Cloudflare tunnel"), h("dd", t.connected ? h("span.ok-text", "Connected") : h("span.warn-text", t.state || "Not connected")),
    t.hostname ? [h("dt", "Public name"), h("dd.mono", t.hostname)] : null,
    t.error ? [h("dt", "Error"), h("dd.danger-text", t.error)] : null);
}

export async function render(view) {
  view.title("Gateway & remote");
  const restart = h("button.btn", { type: "button" }, "Restart gateway");
  view.actions.append(restart);
  restart.addEventListener("click", async () => {
    if (!(await confirm("Restart the gateway", "Connected clients reconnect after a moment.", "Restart", false))) return;
    await act(restart, () => post(ROUTES.gateway.restart), "Gateway restarting");
    setTimeout(() => load().catch(() => {}), 1500);
  });

  async function load() {
    const mods = await modules();
    restart.disabled = !mods.get("gateway")?.enabled;
    const [gw, tunnel] = await Promise.all([
      moduleCard(mods, "gateway", "Gateway", ROUTES.gateway.status, gatewayView),
      moduleCard(mods, "tunnel", "Remote access (Cloudflare tunnel)", ROUTES.tunnel.status, tunnelView),
    ]);
    clear(view.body,
      h("p.muted", "The gateway gives every Jellyfin client one address over all your sources. Remote access publishes it outside your network."),
      h("div.cards", gw, tunnel));
  }
  await load();
  const refresh = (m) => { if (!m || ["gateway", "tunnel"].includes(m.name || m.module)) load().catch(() => {}); };
  const offs = [onEvent("health", refresh), onEvent("module", refresh)];
  return () => offs.forEach((f) => f());
}
