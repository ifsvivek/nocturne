import { h, clear, table, KINDS, kindLabel, act, confirm, dialog, field, select, sw } from "../ui.js";
import { get, post, put, del } from "../api.js";

async function editDialog(def) {
  const isNew = !def;
  def = def || { name: "", protocol: "torrent", url: "", categories: [], kinds: ["movie", "series"], enabled: true, priority: 25 };
  const name = h("input", { type: "text", name: "name", required: true, value: def.name });
  const protocol = select("protocol", [["torrent", "Torznab (torrent)"], ["usenet", "Newznab (Usenet)"]], def.protocol);
  const url = h("input", { type: "url", name: "url", required: true, value: def.url, placeholder: "http://127.0.0.1:9696/1/" });
  const key = h("input", { type: "password", name: "api_key", autocomplete: "off", required: isNew, placeholder: isNew ? "" : "Stored; leave empty to keep it" });
  const cats = h("input", { type: "text", name: "categories", value: (def.categories || []).join(", "), placeholder: "2000, 5000 (empty = standard for each kind)" });
  const prio = h("input", { type: "number", name: "priority", min: "1", max: "50", value: def.priority ?? 25 });
  const enabled = sw("enabled", def.enabled);
  const kinds = KINDS.map(([k, many]) => h("label", h("input", { type: "checkbox", name: "kind", value: k, checked: def.kinds.includes(k) }), many));
  return dialog(isNew ? "Add indexer" : `Edit ${def.name}`, [
    h("p.muted", "A Torznab or Newznab feed you run or subscribe to, for example from Prowlarr or Jackett. NOCTURNE ships no site definitions."),
    h("div.form-grid",
      field("Name", name), field("Protocol", protocol),
      h("div.full", field("Feed URL", url)),
      field("API key", key, "Kept in the system keyring, never shown again"),
      field("Categories", cats), field("Priority", prio, "Lower wins among equal releases"),
      h("div.field", h("span", "Enabled"), enabled),
      h("div.field.full", h("span", "Search for"), h("div.checks", kinds))),
  ], [{ label: "Cancel", value: "cancel" }, { label: "Save", value: "save", primary: true }], {
    onSubmit: async () => {
      const body = {
        def: {
          ...def,
          name: name.value.trim(),
          protocol: protocol.value,
          url: url.value.trim(),
          categories: cats.value.split(/[\s,]+/).filter(Boolean).map(Number).filter(Number.isInteger),
          kinds: kinds.map((l) => l.querySelector("input")).filter((i) => i.checked).map((i) => i.value),
          enabled: enabled.querySelector("input").checked,
          priority: Number(prio.value) || 25,
        },
        api_key: key.value === "" ? (isNew ? "" : null) : key.value,
      };
      if (isNew) await post("/library/indexers", body);
      else await put(`/library/indexers/${def.id}`, body);
    },
  });
}

function capsView(c) {
  const modes = [["search", "Search"], ["movie", "Movies"], ["tv", "TV"], ["music", "Music"], ["book", "Books"]];
  return [
    h("dl.kv", modes.map(([k, l]) => [h("dt", l), h("dd", c[k] ? c[k].join(", ") : h("span.faint", "not offered"))]),
      h("dt", "Page limit"), h("dd", c.limit_max ?? "–")),
    h("h3", "Categories"),
    h("p.muted.mono", (c.categories || []).map((x) => `${x.id} ${x.name}`).join(" · ") || "none"),
  ];
}

export async function render(view) {
  view.title("Indexers");
  const add = h("button.btn.primary", { type: "button" }, "Add indexer");
  view.actions.append(add);
  add.addEventListener("click", async () => { if (await editDialog(null)) load(); });

  async function load() {
    const list = await get("/library/indexers");
    list.sort((a, b) => (a.priority - b.priority) || a.name.localeCompare(b.name));
    const rows = list.map((d) => {
      const test = h("button.btn.small", { type: "button" }, "Test");
      test.addEventListener("click", async () => {
        const caps = await act(test, () => post(`/library/indexers/${d.id}/test`));
        if (caps) dialog(`${d.name}: capabilities`, capsView(caps), [{ label: "Close", value: "cancel" }]);
      });
      const edit = h("button.btn.small", { type: "button" }, "Edit");
      edit.addEventListener("click", async () => { if (await editDialog(d)) load(); });
      const rm = h("button.btn.small.danger", { type: "button" }, "Delete");
      rm.addEventListener("click", async () => {
        if (!(await confirm("Delete indexer", `Delete “${d.name}” and its stored key?`, "Delete"))) return;
        await act(rm, () => del(`/library/indexers/${d.id}`), "Indexer deleted");
        load();
      });
      return h("tr",
        h("td", h("div", d.name), h("div.faint.wrap-any.hide-narrow", d.url)),
        h("td", h("span.badge", d.protocol === "usenet" ? "Newznab" : "Torznab")),
        h("td.hide-narrow", d.kinds.map(kindLabel).join(", ")),
        h("td.num.hide-narrow", d.priority),
        h("td", d.enabled ? h("span.badge.accent", "On") : h("span.badge", "Off")),
        h("td.actions", test, " ", edit, " ", rm));
    });
    clear(view.body,
      list.some((d) => d.protocol === "usenet") ? h("p.note", "Usenet results are shown but can only be grabbed through a Usenet download client.") : null,
      table(["Name", "Type", ["Kinds", "hide-narrow"], ["Priority", "num hide-narrow"], "Enabled", ""], rows,
        "No indexers yet. Add the Torznab or Newznab feed of Prowlarr, Jackett or a site you use."));
  }
  await load();
}
