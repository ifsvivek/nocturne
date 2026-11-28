import { h, clear, table, KINDS, kindLabel, act, confirm, dialog, field, select } from "../ui.js";
import { getOr, post, put, del } from "../api.js";
import { ROUTES, modules, moduleCard } from "../modules.js";

async function editDialog(lib) {
  const isNew = !lib;
  lib = lib || { name: "", kind: "movie", mode: "managed", root: "", folder_template: "", file_template: "" };
  const name = h("input", { type: "text", name: "name", required: true, value: lib.name });
  const kind = select("kind", KINDS.map(([k, many]) => [k, many]), lib.kind);
  const root = h("input", { type: "text", name: "root", required: true, value: lib.root, placeholder: "/srv/media/movies", pattern: "/.*" });
  const folder = h("input", { type: "text", name: "folder_template", value: lib.folder_template || "", placeholder: "{name} ({year})" });
  const file = h("input", { type: "text", name: "file_template", value: lib.file_template || "", placeholder: "{name} ({year}) {quality}" });
  const managed = h("input", { type: "radio", name: "mode", value: "managed", checked: lib.mode === "managed" });
  const existing = h("input", { type: "radio", name: "mode", value: "existing", checked: lib.mode === "existing" });
  return dialog(isNew ? "Add library folder" : `Edit ${lib.name}`, [h("div.form-grid",
    field("Name", name), field("Kind", kind),
    h("div.full", field("Folder", root, "Absolute path on the server")),
    h("fieldset.full.field",
      h("legend", h("span.muted", "Where finished downloads go")),
      h("label.inline-check", managed, "A folder NOCTURNE manages (created if missing, added as a local source)"),
      h("label.inline-check", existing, "My existing folder (imports are added under the templates; other files are never touched)")),
    field("Folder template", folder, "Empty = default for the kind"),
    field("File template", file, "Empty = default for the kind"))],
  [{ label: "Cancel", value: "cancel" }, { label: "Save", value: "save", primary: true }], {
    onSubmit: async () => {
      const body = {
        ...lib, name: name.value.trim(), kind: kind.value, root: root.value.trim(),
        mode: existing.checked ? "existing" : "managed",
        folder_template: folder.value.trim(), file_template: file.value.trim(),
      };
      if (isNew) await post("/library/libraries", body);
      else await put(`/library/libraries/${lib.id}`, body);
    },
  });
}

export async function render(view) {
  view.title("Storage");
  const add = h("button.btn.primary", { type: "button" }, "Add library folder");
  view.actions.append(add);
  add.addEventListener("click", async () => { if (await editDialog(null)) load(); });

  async function load() {
    const [libs, config, status] = await Promise.all([getOr("/library/libraries", null), getOr("/config", null), getOr("/status", null)]);
    const rows = (libs || []).map((l) => {
      const edit = h("button.btn.small", { type: "button" }, "Edit");
      edit.addEventListener("click", async () => { if (await editDialog(l)) load(); });
      const rm = h("button.btn.small.danger", { type: "button" }, "Forget");
      rm.addEventListener("click", async () => {
        if (!(await confirm("Forget library folder", `Forget “${l.name}”? Its files are not touched. Folders in use cannot be forgotten.`, "Forget"))) return;
        await act(rm, () => del(`/library/libraries/${l.id}`), "Library folder forgotten");
        load();
      });
      return h("tr",
        h("td", l.name),
        h("td", kindLabel(l.kind)),
        h("td.mono.wrap-any", l.root),
        h("td.hide-narrow", h("span.badge", { class: l.mode === "managed" ? "accent" : "" }, l.mode === "managed" ? "Managed" : "Existing")),
        h("td.actions", edit, " ", rm));
    });
    const dl = config?.downloads;
    const disks = Array.isArray(status?.disks) ? status.disks : [];
    const mounts = await moduleCard(await modules(), "storage", "Network mounts and remote agents", ROUTES.storage.list);
    clear(view.body,
      h("section.stack",
        h("h3", "Library folders"),
        libs === null
          ? h("div.empty", "The library module is switched off. Turn it on in Settings.")
          : table(["Name", "Kind", "Folder", ["Mode", "hide-narrow"], ""], rows, "No library folders yet. Add one per kind of media.")),
      h("p"),
      h("div.cards",
        h("div.card", h("h3", "Downloads"),
          dl ? h("dl.kv",
            h("dt", "Folder"), h("dd.mono", dl.dir || "default (data folder)"),
            h("dt", "Seed until"), h("dd", `ratio ${dl.seed_ratio} or ${dl.seed_hours} h`),
            h("dt", "Module"), h("dd", dl.enabled ? "On" : "Off")) : h("p.muted", "Not available.")),
        h("div.card", h("h3", "Disks"),
          disks.length ? h("dl.kv", disks.map((d) => [h("dt.mono", d.path), h("dd", `${Math.round((d.free / d.total) * 100)} % free`)]))
            : h("p.muted", "This server does not report disk space yet.")),
        mounts));
  }
  await load();
}
