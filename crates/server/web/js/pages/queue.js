import { h, clear, table, tabs, bytes, rate, duration, act, dialog, field, toast, when } from "../ui.js";
import { get, getOr, post, put, del, onEvent } from "../api.js";

const GRAB = { downloading: ["Downloading", "accent"], seeding: ["Seeding", ""], import_blocked: ["Import blocked", "warn"], failed: ["Failed", "danger"], done: ["Done", ""] };
const TORRENT = { queued: "Queued", metadata: "Fetching metadata", checking: "Checking", downloading: "Downloading", seeding: "Seeding", paused: "Paused", error: "Error" };

async function removeDialog(item) {
  const block = h("input", { type: "checkbox", name: "blocklist" });
  const again = h("input", { type: "checkbox", name: "search" });
  const ok = await dialog("Remove download", [
    h("p", `Remove “${item.grab.release.title}” with its torrent and downloaded data?`),
    h("div.checks",
      h("label", block, "Blocklist this release"),
      h("label", again, "Search for another")),
  ], [{ label: "Cancel", value: "cancel" }, { label: "Remove", value: "yes", danger: true }]);
  if (ok !== "yes") return false;
  await del(`/library/queue/${item.grab.id}?blocklist=${block.checked}&search=${again.checked}`);
  toast("Removed");
  return true;
}

export async function render(view) {
  view.title("Queue");
  let tab = "queue";
  const box = h("div");
  view.body.append(box);
  // Rows keyed by grab id so live progress updates in place without losing focus.
  const live = new Map();

  function progressCells(s) {
    const pct = s && s.size ? s.done / s.size : 0;
    return {
      bar: h("progress", { max: "1", value: String(pct), "aria-label": "Progress" }),
      text: s ? `${Math.floor(pct * 100)} % · ${bytes(s.done)} of ${bytes(s.size)}` : "Waiting for the torrent",
      speed: s ? `${rate(s.down_rate)} ↓ ${rate(s.up_rate)} ↑` : "",
      eta: s && s.eta_secs !== null && s.eta_secs !== undefined ? duration(s.eta_secs) : "",
    };
  }

  function queueRow(item, reload) {
    const g = item.grab;
    const s = item.status;
    const [label, cls] = GRAB[g.state] || [g.state, ""];
    const p = progressCells(s);
    const pctText = h("div.faint", p.text);
    const speed = h("td.nowrap.hide-narrow.muted", p.speed);
    const eta = h("td.nowrap.muted", p.eta);
    const bar = p.bar;
    const btn = (text, fn, c = "") => {
      const b = h("button.btn.small", { type: "button", class: c }, text);
      b.addEventListener("click", () => act(b, fn).then(reload));
      return b;
    };
    const paused = s && s.state === "paused";
    const row = h("tr",
      h("td", h("a", { href: `/library/${g.title}`, "data-link": true }, item.title_name),
        h("div.release-title.faint", g.release.title),
        g.message ? h("div", { class: g.state === "failed" ? "danger-text" : "warn-text" }, g.message) : null),
      h("td", h("span.badge", { class: cls }, s && s.state === "paused" ? "Paused" : label)),
      h("td", bar, pctText),
      speed, eta,
      h("td.actions",
        g.state === "import_blocked" ? btn("Retry import", () => post(`/library/queue/${g.id}/retry`)) : null,
        g.state === "downloading" ? (paused ? btn("Resume", () => post(`/library/queue/${g.id}/resume`)) : btn("Pause", () => post(`/library/queue/${g.id}/pause`))) : null,
        btn("Remove", () => removeDialog(item), "danger")));
    live.set(g.id, { key: `${g.state}/${s ? s.state : ""}`, update: (st) => {
      const q = progressCells(st);
      bar.value = st && st.size ? st.done / st.size : 0;
      pctText.textContent = q.text;
      speed.textContent = q.speed;
      eta.textContent = q.eta;
    } });
    return row;
  }

  async function load() {
    live.clear();
    const bar = tabs([["queue", "Queue"], ["torrents", "Torrents"]], tab, (k) => { tab = k; load(); });
    if (tab === "queue") {
      const items = await get("/library/queue");
      clear(box, bar, table(["Title", "State", "Progress", ["Speed", "hide-narrow"], "ETA", ""], items.map((i) => queueRow(i, load)),
        "The queue is empty. Grabs show up here while they download, seed or wait for an import fix."));
    } else {
      const [list, config] = await Promise.all([getOr("/downloads", null), getOr("/config", null)]);
      if (list === null) {
        clear(box, bar, h("div.empty", "The downloads module is switched off. Turn it on in Settings."));
        return;
      }
      const lim = config?.downloads?.limits || {};
      const down = h("input", { type: "number", min: "0", name: "down", value: lim.down_bps ? Math.round(lim.down_bps / 1024) : "" });
      const up = h("input", { type: "number", min: "0", name: "up", value: lim.up_bps ? Math.round(lim.up_bps / 1024) : "" });
      const save = h("button.btn", { type: "submit" }, "Save limits");
      const form = h("form.row", { on: { submit: (ev) => {
        ev.preventDefault();
        const v = (x) => (x.value === "" || Number(x.value) === 0 ? null : Number(x.value) * 1024);
        act(save, () => put("/downloads/limits", { down_bps: v(down), up_bps: v(up) }), "Limits saved");
      } } }, field("Download limit (KB/s, empty = none)", down), field("Upload limit (KB/s)", up), save);
      const rows = list.map((t) => h("tr",
        h("td", h("div.wrap-any", t.name), h("div.faint", t.category), t.error ? h("div.danger-text", t.error) : null),
        h("td", h("span.badge", { class: t.state === "error" ? "danger" : t.state === "downloading" ? "accent" : "" }, TORRENT[t.state] || t.state)),
        h("td", h("progress", { max: String(t.size || 1), value: String(t.done), "aria-label": "Progress" }), h("div.faint", `${bytes(t.done)} of ${bytes(t.size)}`)),
        h("td.nowrap.hide-narrow.muted", `${rate(t.down_rate)} ↓ ${rate(t.up_rate)} ↑`),
        h("td.num.hide-narrow", t.peers),
        h("td.num.hide-narrow", t.size ? (t.uploaded / t.size).toFixed(2) : ""),
        h("td.muted.hide-narrow", when(t.added))));
      clear(box, bar, h("div.card", h("h3", "Transfer limits"), form), h("p"),
        table(["Name", "State", "Progress", ["Speed", "hide-narrow"], ["Peers", "num hide-narrow"], ["Ratio", "num hide-narrow"], ["Added", "hide-narrow"]], rows, "No torrents."));
    }
  }
  await load();
  return onEvent("queue", (items) => {
    if (tab !== "queue" || !Array.isArray(items)) return;
    const ids = new Set(items.map((i) => i.grab.id));
    const changed = (i) => !live.has(i.grab.id) || live.get(i.grab.id).key !== `${i.grab.state}/${i.status ? i.status.state : ""}`;
    if (ids.size !== live.size || items.some(changed)) { load().catch(() => {}); return; }
    for (const i of items) live.get(i.grab.id).update(i.status);
  });
}
