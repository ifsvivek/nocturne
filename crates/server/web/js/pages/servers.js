import { h, clear } from "../ui.js";
import { onEvent } from "../api.js";
import { ROUTES, modules, moduleCard } from "../modules.js";

// Other Jellyfin and NOCTURNE servers: the ones added as sources, and the ones discovery found on the network.
export async function render(view) {
  view.title("Servers");
  async function load() {
    const mods = await modules();
    const [sources, found] = await Promise.all([
      moduleCard(mods, "sources", "Sources", ROUTES.sources.list),
      moduleCard(mods, "discovery", "Found on this network", ROUTES.discovery.list),
    ]);
    clear(view.body,
      h("p.muted", "Jellyfin servers and other NOCTURNE servers this server reads from, and the ones discovery sees on your network."),
      h("div.stack", sources, found));
  }
  await load();
  const refresh = (m) => { if (m && ["sources", "discovery"].includes(m.name || m.module)) load().catch(() => {}); };
  const offs = [onEvent("health", refresh), onEvent("module", refresh)];
  return () => offs.forEach((f) => f());
}
