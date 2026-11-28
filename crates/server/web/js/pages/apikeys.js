import { h, clear, table, when, act, confirm, dialog, field, toast } from "../ui.js";
import { get, post, del } from "../api.js";

export async function render(view) {
  view.title("API keys");
  const add = h("button.btn.primary", { type: "button" }, "New key");
  view.actions.append(add);
  add.addEventListener("click", async () => {
    const name = h("input", { type: "text", name: "name", required: true, placeholder: "Desktop app, phone, script…" });
    const created = await dialog("New API key", [field("Name", name)], [{ label: "Cancel", value: "cancel" }, { label: "Create", value: "ok", primary: true }], {
      onSubmit: () => post("/apikeys", { name: name.value.trim() }),
    });
    if (!created) return;
    const copy = h("button.btn", { type: "button" }, "Copy");
    copy.addEventListener("click", async () => {
      try { await navigator.clipboard.writeText(created.key); toast("Copied"); } catch { toast("Select the key and copy it.", true); }
    });
    await dialog("Your new key", [
      h("p", "Copy it now: it is shown only once. Send it as ", h("code", "X-Api-Key"), " or as a Bearer token."),
      h("div.secret", { tabindex: "0", "aria-label": "API key" }, created.key),
      h("p"), copy,
    ], [{ label: "Done", value: "cancel" }]);
    load();
  });

  async function load() {
    const list = await get("/apikeys");
    clear(view.body,
      h("p.muted", "Keys let apps and scripts use the API without your password. Revoke a key you no longer use."),
      table(["Name", ["Created", "hide-narrow"], "Last used", ""], list.map((k) => {
        const rm = h("button.btn.small.danger", { type: "button" }, "Revoke");
        rm.addEventListener("click", async () => {
          if (!(await confirm("Revoke key", `Revoke “${k.name}”? Anything using it stops working.`, "Revoke"))) return;
          await act(rm, () => del(`/apikeys/${encodeURIComponent(k.id)}`), "Key revoked");
          load();
        });
        return h("tr", h("td", k.name), h("td.muted.hide-narrow", when(k.created)), h("td.muted", when(k.last_used)), h("td.actions", rm));
      }), "No API keys yet."));
  }
  await load();
}
