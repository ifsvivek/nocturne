import { h, clear, table, KINDS, kindLabel, when, act, confirm, dialog, field, select } from "../ui.js";
import { getOr, post, del, onEvent } from "../api.js";

const STATUS = { pending: ["Pending", "warn"], approved: ["Approved", "accent"], downloading: ["Downloading", "accent"], available: ["Available", "accent"], declined: ["Declined", "danger"] };

async function newRequest() {
  const kind = select("kind", KINDS.map(([k, , one]) => [k, one]), "movie");
  const name = h("input", { type: "text", name: "name", required: true });
  const year = h("input", { type: "number", name: "year", min: "1800", max: "2100" });
  const imdb = h("input", { type: "text", name: "imdb", placeholder: "tt0133093" });
  const tmdb = h("input", { type: "number", name: "tmdb" });
  const tvdb = h("input", { type: "number", name: "tvdb" });
  const seasons = h("input", { type: "text", name: "seasons", placeholder: "1, 2 (empty = all)" });
  return dialog("New request", [h("div.form-grid",
    field("Kind", kind), field("Name", name), field("Year", year),
    field("IMDb id", imdb), field("TMDB id", tmdb), field("TVDB id", tvdb),
    field("Seasons", seasons, "Shows only"))],
  [{ label: "Cancel", value: "cancel" }, { label: "Request", value: "ok", primary: true }], {
    onSubmit: async () => {
      const num = (v) => (v === "" ? null : Number(v));
      await post("/requests", {
        kind: kind.value, name: name.value.trim(), year: num(year.value),
        ids: { imdb: imdb.value.trim() || null, tmdb: num(tmdb.value), tvdb: num(tvdb.value) },
        seasons: seasons.value.split(/[\s,]+/).filter(Boolean).map(Number).filter((n) => Number.isInteger(n)),
      });
    },
  });
}

export async function render(view) {
  view.title("Requests");
  const add = h("button.btn.primary", { type: "button" }, "New request");
  view.actions.append(add);
  add.addEventListener("click", async () => { if (await newRequest()) load(); });

  async function load() {
    const list = await getOr("/requests", null);
    if (list === null) {
      clear(view.body, h("div.empty", "The requests module is switched off. Turn it on in Settings."));
      add.disabled = true;
      return;
    }
    list.sort((a, b) => b.created - a.created);
    const rows = list.map((r) => {
      const [label, cls] = STATUS[r.status] || [r.status, ""];
      const approve = r.status === "pending" ? h("button.btn.small.primary", { type: "button" }, "Approve") : null;
      if (approve) approve.addEventListener("click", () => act(approve, () => post(`/requests/${r.id}/approve`), "Approved").then(load));
      const cancel = h("button.btn.small.danger", { type: "button" }, "Cancel");
      cancel.addEventListener("click", async () => {
        if (!(await confirm("Cancel request", `Cancel the request for “${r.name}”? A title it created stays monitored.`, "Cancel request"))) return;
        await act(cancel, () => del(`/requests/${r.id}`), "Request cancelled");
        load();
      });
      return h("tr",
        h("td", r.title ? h("a", { href: `/library/${r.title}`, "data-link": true }, r.name) : r.name, r.year ? h("span.muted", ` (${r.year})`) : null,
          r.seasons && r.seasons.length ? h("div.faint", `Seasons ${r.seasons.join(", ")}`) : null),
        h("td", kindLabel(r.kind)),
        h("td", h("span.badge", { class: cls }, label)),
        h("td.muted.hide-narrow", r.requested_by),
        h("td.muted.hide-narrow.nowrap", when(r.created)),
        h("td.actions", approve, " ", cancel));
    });
    clear(view.body, table(["Title", "Kind", "Status", ["By", "hide-narrow"], ["When", "hide-narrow"], ""], rows, "No requests yet."));
  }
  await load();
  return onEvent("request", () => load().catch(() => {}));
}
