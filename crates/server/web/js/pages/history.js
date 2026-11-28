import { h, clear, table, when, dateTime, qualityName, tabs, select, act, confirm } from "../ui.js";
import { get, getOr, del, onEvent } from "../api.js";

const EVENTS = {
  grabbed: ["Grabbed", "accent"],
  imported: ["Imported", "accent"],
  upgraded: ["Upgraded", "accent"],
  import_failed: ["Import failed", "danger"],
  download_failed: ["Download failed", "danger"],
  removed: ["Removed", ""],
};

export function historyRow(e, names, compact = false) {
  const [label, cls] = EVENTS[e.event] || [e.event, ""];
  const name = names.get(e.title) || `#${e.title}`;
  return h("tr",
    h("td.nowrap", h("time", { datetime: new Date(e.at * 1000).toISOString(), title: dateTime(e.at) }, when(e.at))),
    h("td", h("a", { href: `/library/${e.title}`, "data-link": true }, name)),
    h("td", h("span.badge", { class: cls }, label)),
    h("td.hide-narrow", qualityName(e.quality)),
    compact ? null : h("td.release-title.hide-narrow", e.release),
    compact ? null : h("td.muted.hide-narrow", e.indexer),
    compact ? null : h("td.muted", e.message || ""));
}

export async function render(view) {
  view.title("History");
  let tab = "history";
  let filter = "";
  const box = h("div");
  view.body.append(box);

  async function load() {
    const titles = await getOr("/library/titles", []);
    const names = new Map(titles.map((t) => [t.id, t.name]));
    const bar = tabs([["history", "History"], ["blocklist", "Blocklist"]], tab, (k) => { tab = k; load(); });
    if (tab === "history") {
      const list = await get("/library/history?limit=500");
      const pick = select("event", [["", "All events"], ...Object.entries(EVENTS).map(([k, v]) => [k, v[0]])], filter);
      pick.setAttribute("aria-label", "Filter by event");
      pick.addEventListener("change", () => { filter = pick.value; load(); });
      const rows = list.filter((e) => !filter || e.event === filter).map((e) => historyRow(e, names));
      clear(box, bar, h("div.toolbar", pick),
        table(["When", "Title", "Event", ["Quality", "hide-narrow"], ["Release", "hide-narrow"], ["Indexer", "hide-narrow"], "Message"], rows, "No history yet."));
    } else {
      const list = await get("/library/blocklist");
      const rows = list.map((b) => {
        const rm = h("button.btn.small", { type: "button" }, "Remove");
        rm.addEventListener("click", async () => {
          if (!(await confirm("Remove from blocklist", `“${b.release}” can be grabbed again.`, "Remove", false))) return;
          await act(rm, () => del(`/library/blocklist/${b.id}`), "Removed from the blocklist");
          load();
        });
        return h("tr",
          h("td.nowrap", when(b.at)),
          h("td", h("a", { href: `/library/${b.title}`, "data-link": true }, names.get(b.title) || `#${b.title}`)),
          h("td.release-title", b.release),
          h("td.muted.hide-narrow", b.reason),
          h("td.actions", rm));
      });
      clear(box, bar, table(["When", "Title", "Release", ["Reason", "hide-narrow"], ""], rows, "The blocklist is empty."));
    }
  }
  await load();
  return onEvent("history", () => { if (tab === "history") load().catch(() => {}); });
}
