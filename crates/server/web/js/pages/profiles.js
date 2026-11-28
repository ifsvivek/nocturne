import { h, clear, table, KINDS, kindLabel, act, confirm, dialog, field, select, sw, qualityName, qualitiesFor, lines } from "../ui.js";
import { get, post, put, del } from "../api.js";

async function editDialog(p) {
  const isNew = !p;
  p = p || { name: "", kind: "movie", allowed: ["web_dl1080", "bluray1080"], cutoff: "web_dl1080", upgrades: true, preferred: [], required: [], ignored: [], min_mb: 0, max_mb: 0, min_seeders: 1, languages: [] };
  const name = h("input", { type: "text", name: "name", required: true, value: p.name });
  const kind = select("kind", KINDS.map(([k, many]) => [k, many]), p.kind);
  const allowedBox = h("div.checks");
  const cutoff = h("select", { name: "cutoff" });
  const upgrades = sw("upgrades", p.upgrades !== false);
  const preferred = h("textarea", { name: "preferred", rows: "3", placeholder: "x265 = 10\n/\\bHDR\\b/ = 5\nHC = -100" },
    (p.preferred || []).map((x) => `${x.term} = ${x.score}`).join("\n"));
  const required = h("textarea", { name: "required", rows: "2" }, (p.required || []).join("\n"));
  const ignored = h("textarea", { name: "ignored", rows: "2" }, (p.ignored || []).join("\n"));
  const minMb = h("input", { type: "number", min: "0", name: "min_mb", value: p.min_mb || 0 });
  const maxMb = h("input", { type: "number", min: "0", name: "max_mb", value: p.max_mb || 0 });
  const seeders = h("input", { type: "number", min: "0", name: "min_seeders", value: p.min_seeders ?? 1 });
  const langs = h("input", { type: "text", name: "languages", value: (p.languages || []).join(", "), placeholder: "en, de (empty = any)" });

  const fill = () => {
    const qs = qualitiesFor(kind.value).filter((q) => q !== "unknown");
    const was = new Set(p.allowed);
    clear(allowedBox, qs.map((q) => h("label", h("input", { type: "checkbox", value: q, checked: was.has(q) }), qualityName(q))));
    clear(cutoff, qs.map((q) => h("option", { value: q, selected: q === p.cutoff }, qualityName(q))));
  };
  kind.addEventListener("change", () => { p = { ...p, allowed: [], cutoff: "" }; fill(); });
  fill();

  return dialog(isNew ? "Add profile" : `Edit ${p.name}`, [h("div.form-grid",
    field("Name", name), field("Kind", kind),
    h("div.field.full", h("span", "Allowed qualities (worst to best)"), allowedBox),
    field("Cutoff", cutoff, "Upgrades stop once a file of this quality is on disk"),
    h("div.field", h("span", "Upgrades"), upgrades),
    h("div.full", field("Preferred terms", preferred, "One per line: term = score. /regex/ allowed; negative scores discourage.")),
    field("Required terms", required, "One per line; at least one must match"),
    field("Ignored terms", ignored, "One per line; none may match"),
    field("Minimum size (MB per unit)", minMb, "0 = none"),
    field("Maximum size (MB per unit)", maxMb, "0 = none"),
    field("Minimum seeders", seeders),
    field("Languages", langs, "ISO 639-1 codes"))],
  [{ label: "Cancel", value: "cancel" }, { label: "Save", value: "save", primary: true }], {
    wide: true,
    onSubmit: async () => {
      const allowed = [...allowedBox.querySelectorAll("input:checked")].map((i) => i.value);
      if (!allowed.length) throw new Error("Allow at least one quality.");
      if (!allowed.includes(cutoff.value)) throw new Error("The cutoff must be one of the allowed qualities.");
      const pref = lines(preferred.value).map((l) => {
        const m = /^(.*?)\s*=\s*(-?\d+)$/.exec(l);
        if (!m) throw new Error(`“${l}” is not “term = score”.`);
        return { term: m[1], score: Number(m[2]) };
      });
      const body = {
        ...p, name: name.value.trim(), kind: kind.value, allowed, cutoff: cutoff.value,
        upgrades: upgrades.querySelector("input").checked, preferred: pref,
        required: lines(required.value), ignored: lines(ignored.value),
        min_mb: Number(minMb.value) || 0, max_mb: Number(maxMb.value) || 0, min_seeders: Number(seeders.value) || 0,
        languages: langs.value.split(/[\s,]+/).filter(Boolean),
      };
      if (isNew) await post("/library/profiles", body);
      else await put(`/library/profiles/${p.id}`, body);
    },
  });
}

export async function render(view) {
  view.title("Profiles");
  const add = h("button.btn.primary", { type: "button" }, "Add profile");
  view.actions.append(add);
  add.addEventListener("click", async () => { if (await editDialog(null)) load(); });

  async function load() {
    const list = await get("/library/profiles");
    const rows = list.map((p) => {
      const edit = h("button.btn.small", { type: "button" }, "Edit");
      edit.addEventListener("click", async () => { if (await editDialog(p)) load(); });
      const rm = h("button.btn.small.danger", { type: "button" }, "Delete");
      rm.addEventListener("click", async () => {
        if (!(await confirm("Delete profile", `Delete “${p.name}”? Profiles in use cannot be deleted.`, "Delete"))) return;
        await act(rm, () => del(`/library/profiles/${p.id}`), "Profile deleted");
        load();
      });
      return h("tr",
        h("td", p.name),
        h("td", kindLabel(p.kind)),
        h("td.hide-narrow", h("div.row", p.allowed.map((q) => h("span.badge", { class: q === p.cutoff ? "accent" : "" }, qualityName(q))))),
        h("td", qualityName(p.cutoff)),
        h("td.hide-narrow", p.upgrades ? "Yes" : "No"),
        h("td.actions", edit, " ", rm));
    });
    clear(view.body, table(["Name", "Kind", ["Allowed", "hide-narrow"], "Cutoff", ["Upgrades", "hide-narrow"], ""], rows,
      "No profiles yet. A profile says which qualities to grab and when to stop upgrading."));
  }
  await load();
}
