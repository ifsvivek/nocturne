import { h, clear, duration, rate, when, table, act, bytes } from "../ui.js";
import { get, getOr, post, onEvent } from "../api.js";
import { historyRow } from "./history.js";

const JOBS = { rss_sync: "RSS sync", missing_search: "Search missing", upgrade_search: "Search upgrades", refresh_metadata: "Refresh metadata" };
const MODULES = { library: "Library manager", downloads: "Downloads", requests: "Requests", gateway: "Gateway", tunnel: "Remote access", discovery: "Discovery", sources: "Sources", storage: "Storage" };

export async function render(view) {
  view.title("Dashboard");
  const refresh = h("button.btn", { type: "button" }, "Refresh");
  view.actions.append(refresh);

  async function load() {
    const [status, queue, hist, titles] = await Promise.all([
      get("/status"),
      getOr("/library/queue", []),
      getOr("/library/history?limit=8", []),
      getOr("/library/titles", []),
    ]);
    const names = new Map(titles.map((t) => [t.id, t.name]));
    const stat = (v, k) => h("div.stat", h("div.v", v), h("div.k", k));
    const modules = status.modules.map((m) => {
      const state = !m.enabled ? ["", "Off"] : m.error ? ["bad", "Error"] : m.running ? ["on", "Running"] : ["warn", "Starting"];
      return h("li.row",
        h("span.dot", { class: state[0], "aria-hidden": "true" }),
        h("span", MODULES[m.name] || m.name),
        h("span.spacer"),
        h("span", { class: m.error ? "danger-text" : "muted" }, m.error || state[1]));
    });
    const jobs = status.jobs.map((j) => {
      const run = h("button.btn.small", { type: "button", disabled: j.running }, j.running ? "Running" : "Run now");
      run.addEventListener("click", () => act(run, () => post(`/library/jobs/${j.job}/run`), `${JOBS[j.job] || j.job} started`));
      return h("tr",
        h("td", JOBS[j.job] || j.job),
        h("td.muted.hide-narrow", when(j.last_run)),
        h("td.muted", j.running ? "now" : when(j.next_run)),
        h("td.actions", run));
    });
    const downloading = queue.filter((q) => q.grab.state === "downloading").slice(0, 5).map((q) => {
      const s = q.status;
      const pct = s && s.size ? s.done / s.size : 0;
      return h("li.stack",
        h("div.row", h("a", { href: `/library/${q.grab.title}`, "data-link": true }, q.title_name), h("span.spacer"),
          h("span.muted.nowrap", s ? `${Math.floor(pct * 100)} % · ${rate(s.down_rate)}` : "waiting")),
        h("progress", { max: "1", value: String(pct), "aria-label": `${q.title_name} progress` }));
    });
    const disk = Array.isArray(status.disks) && status.disks.length
      ? h("div.card", h("h3", "Disk space"), h("ul.plain", status.disks.map((d) => h("li.stack",
        h("div.row", h("span.wrap-any", d.path), h("span.spacer"), h("span.muted", `${bytes(d.free)} free of ${bytes(d.total)}`)),
        h("progress", { max: String(d.total || 1), value: String((d.total || 0) - (d.free || 0)), "aria-label": `${d.path} used` })))))
      : null;

    clear(view.body,
      h("div.stats",
        stat(status.titles, "Titles"),
        stat(status.missing, "Missing"),
        stat(status.downloading, "Downloading"),
        stat(rate(status.down_rate), "Down"),
        stat(rate(status.up_rate), "Up"),
        stat(duration(status.uptime_secs), `Uptime · v${status.version}`)),
      h("div.cards",
        h("div.card", h("h3", "Health"), modules.length ? h("ul.plain", modules) : h("p.muted", "No modules in this server build.")),
        h("div.card", h("div.row", h("h3", "Downloading"), h("span.spacer"), h("a", { href: "/queue", "data-link": true }, "Queue")),
          downloading.length ? h("ul.plain", downloading) : h("p.muted", "Nothing is downloading.")),
        disk),
      h("div.card", h("h3", "Scheduled jobs"), table(["Job", ["Last run", "hide-narrow"], "Next run", ""], jobs, "No jobs.")),
      h("div.card",
        h("div.row", h("h3", "Recent activity"), h("span.spacer"), h("a", { href: "/history", "data-link": true }, "All history")),
        table(["When", "Title", "Event", ["Quality", "hide-narrow"]], hist.map((e) => historyRow(e, names, true)), "No activity yet.")));
  }

  refresh.addEventListener("click", () => act(refresh, load));
  await load();
  // Events only schedule a reload; queue progress arrives every 2 s, so reloads are at most every 5 s.
  let pending = null;
  let last = Date.now();
  const soon = () => {
    if (pending) return;
    pending = setTimeout(() => { pending = null; last = Date.now(); load().catch(() => {}); }, Math.max(500, 5000 - (Date.now() - last)));
  };
  const offs = [onEvent("job", soon), onEvent("health", soon), onEvent("history", soon), onEvent("queue", soon)];
  return () => { clearTimeout(pending); offs.forEach((f) => f()); };
}
