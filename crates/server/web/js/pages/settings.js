import { h, clear, field, select, sw, act, table, when, confirm, toast } from "../ui.js";
import { get, put, del } from "../api.js";
import { moduleBadge } from "../modules.js";

const MODULES = {
  library: ["Library manager", "Monitors titles, searches indexers, grabs and imports."],
  downloads: ["Downloads", "The built-in torrent engine."],
  requests: ["Requests", "Lets people ask for titles; approved requests become monitored titles."],
  gateway: ["Gateway", "One Jellyfin-compatible address over every source."],
  tunnel: ["Remote access", "A Cloudflare tunnel to the gateway."],
  discovery: ["Discovery", "Finds Jellyfin and NOCTURNE servers on this network."],
  sources: ["Sources", "Other Jellyfin and NOCTURNE servers this server reads from."],
  storage: ["Storage", "Network mounts and remote agents."],
};

function accountSection() {
  const current = h("input", { type: "password", name: "current", autocomplete: "current-password", required: true });
  const next = h("input", { type: "password", name: "new", autocomplete: "new-password", required: true, minlength: 8 });
  const again = h("input", { type: "password", name: "again", autocomplete: "new-password", required: true, minlength: 8 });
  const save = h("button.btn", { type: "submit" }, "Change password");
  const form = h("form.form-grid", { on: { submit: (ev) => {
    ev.preventDefault();
    if (next.value !== again.value) { toast("The new passwords do not match.", true); return; }
    act(save, async () => {
      await put("/auth/password", { current: current.value, new: next.value });
      form.reset();
    }, "Password changed: every other session was signed out.");
  } } }, field("Current password", current), field("New password", next, "At least 8 characters"), field("Repeat", again), h("div.field.full", h("div.row", save)));

  const sessions = h("div");
  async function loadSessions() {
    const list = await get("/auth/sessions");
    clear(sessions, table(["Client", ["Signed in", "hide-narrow"], "Last used", ""], list.map((s) => {
      const rm = h("button.btn.small.danger", { type: "button", disabled: s.current }, s.current ? "This session" : "Sign out");
      rm.addEventListener("click", async () => {
        if (!(await confirm("Sign out session", `Sign out ${s.client || "this session"}?`, "Sign out"))) return;
        await act(rm, () => del(`/auth/sessions/${encodeURIComponent(s.id)}`), "Signed out");
        loadSessions();
      });
      return h("tr", h("td.wrap-any", s.client || "unknown"), h("td.muted.hide-narrow", when(s.created)), h("td.muted", when(s.last_used)), h("td.actions", rm));
    }), "No sessions."));
  }
  loadSessions().catch((e) => clear(sessions, h("p.danger-text", e.message)));
  return h("section.card.stack", h("h3", "Account"), form, h("h3", "Signed-in sessions"), sessions);
}

export async function render(view) {
  view.title("Settings");
  const [config, status] = await Promise.all([get("/config"), get("/status")]);
  const mods = new Map(status.modules.map((m) => [m.name, m]));
  const save = h("button.btn.primary", { type: "submit", form: "settings-form" }, "Save");
  view.actions.append(save);

  // Every section with `enabled` is a module, known or not.
  const names = [...new Set([...Object.keys(MODULES), ...Object.keys(config).filter((k) => config[k] && typeof config[k] === "object" && "enabled" in config[k])])]
    .filter((k) => k in config || mods.has(k));
  const switches = new Map();
  const moduleRows = names.map((name) => {
    const [label, desc] = MODULES[name] || [name, ""];
    const s = sw(`module-${name}`, !!config[name]?.enabled);
    switches.set(name, s.querySelector("input"));
    const m = mods.get(name);
    return h("li.row",
      h("div.spacer", h("div", h("strong", label), " ", moduleBadge(m)), desc ? h("div.muted", desc) : null,
        m?.error ? h("div.danger-text", m.error) : null),
      s);
  });

  const http = config.http || {};
  const binding = select("binding", [["local", "This computer only"], ["lan", "Home network (HTTP)"], ["remote", "Internet (HTTPS, your certificate)"]], http.binding || "local");
  const port = h("input", { type: "number", min: "1", max: "65535", name: "port", value: http.port ?? 8100 });
  const cert = h("input", { type: "text", name: "cert_path", value: http.cert_path || "" });
  const key = h("input", { type: "text", name: "key_path", value: http.key_path || "" });
  const proxy = h("input", { type: "text", name: "trusted_proxy", value: http.trusted_proxy || "", placeholder: "127.0.0.1" });

  const dl = config.downloads || {};
  const dir = h("input", { type: "text", name: "dir", value: dl.dir || "", placeholder: "default: the server's data folder" });
  const ratio = h("input", { type: "number", min: "0", step: "0.1", name: "seed_ratio", value: dl.seed_ratio ?? 1 });
  const hours = h("input", { type: "number", min: "0", name: "seed_hours", value: dl.seed_hours ?? 168 });
  const auto = sw("auto_approve", config.requests?.auto_approve !== false, "Approve requests automatically");

  const form = h("form#settings-form.stack", { on: { submit: (ev) => {
    ev.preventDefault();
    const next = structuredClone(config);
    for (const [name, input] of switches) next[name] = { ...(next[name] || {}), enabled: input.checked };
    next.http = { ...http, binding: binding.value, port: Number(port.value) || 8100, cert_path: cert.value.trim() || null, key_path: key.value.trim() || null, trusted_proxy: proxy.value.trim() || null };
    next.downloads = { ...next.downloads, dir: dir.value.trim() || null, seed_ratio: Number(ratio.value), seed_hours: Number(hours.value) };
    next.requests = { ...next.requests, auto_approve: auto.querySelector("input").checked };
    act(save, async () => {
      await put("/config", next);
      view.navigate("/settings", true);
    }, "Settings saved");
  } } },
  h("section.card", h("h3", "Modules"), h("ul.plain.stack", moduleRows)),
  h("section.card.stack", h("h3", "Web and API address"),
    h("div.form-grid", field("Reachable from", binding), field("Port", port), field("Certificate (PEM)", cert, "Needed for Internet"), field("Private key (PEM)", key), field("Trusted proxy", proxy, "X-Forwarded-For is believed only from this address")),
    h("p.note", "Address changes take effect when the server restarts.")),
  h("section.card.stack", h("h3", "Downloads"),
    h("div.form-grid", h("div.full", field("Download folder", dir)), field("Seed until ratio", ratio), field("or hours", hours))),
  h("section.card.stack", h("h3", "Requests"), auto));

  clear(view.body, form, h("p"), accountSection(),
    h("p.faint", `NOCTURNE server ${status.version}`));
}
