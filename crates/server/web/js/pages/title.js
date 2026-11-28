import { h, clear, kindLabel, initial, table, act, confirm, dialog, qualityName, bytes, when, dateTime, toast } from "../ui.js";
import { get, getOr, post, put, del, onEvent } from "../api.js";

function unitLabel(u, kind) {
  if (kind === "series") {
    if (u.season === null || u.season === undefined) return u.name;
    return `S${String(u.season).padStart(2, "0")}E${String(u.episode ?? 0).padStart(2, "0")}`;
  }
  return u.year ? String(u.year) : "";
}

/** Interactive search: every release with its decision, and a grab button. */
async function releasesDialog(title, units) {
  const body = h("div", h("p.muted", "Searching every enabled indexer…"));
  const show = dialog(`Releases · ${title.name}`, [body], [{ label: "Close", value: "cancel" }], { wide: true });
  let list;
  try {
    list = await post(`/library/titles/${title.id}/releases`, { units });
  } catch (e) {
    clear(body, h("div.error-box", { role: "alert" }, e.message));
    return show;
  }
  list.sort((a, b) => (a.rejections.length - b.rejections.length) || ((b.release.seeders ?? -1) - (a.release.seeders ?? -1)));
  const rows = list.map((c) => {
    const r = c.release;
    const ok = !c.rejections.length;
    const usenet = r.protocol === "usenet";
    const grab = h("button.btn.small", { type: "button", class: ok ? "primary" : "" }, "Grab");
    grab.addEventListener("click", async () => {
      if (!ok && !(await confirm("Grab anyway?", `Rejected: ${c.rejections.join("; ")}`, "Grab anyway", false))) return;
      const res = await act(grab, () => post(`/library/titles/${title.id}/grab`, r));
      if (res !== undefined) { grab.textContent = "Grabbed"; grab.disabled = true; toast(`Grabbed ${r.title}`); }
    });
    if (usenet) { grab.disabled = true; grab.title = "Usenet releases need a Usenet download client"; }
    return h("tr",
      h("td", h("div.release-title", r.title),
        ok ? null : h("div.danger-text", c.rejections.join(" · "))),
      h("td", h("span.badge", { class: ok ? "accent" : "danger" }, qualityName(r.parsed?.quality))),
      h("td.num.nowrap", bytes(r.size)),
      h("td.num.hide-narrow", r.seeders ?? "–"),
      h("td.muted.hide-narrow", r.indexer),
      h("td.muted.hide-narrow.nowrap", r.published ? when(r.published) : ""),
      h("td.actions", grab));
  });
  clear(body, h("p.muted", `${list.length} releases, ${list.filter((c) => !c.rejections.length).length} acceptable.`),
    table(["Release", "Quality", ["Size", "num"], ["Seeders", "num hide-narrow"], ["Indexer", "hide-narrow"], ["Age", "hide-narrow"], ""], rows, "No releases found."));
  return show;
}

export async function render(view, id) {
  let detail;
  let profiles = [];
  let libraries = [];

  async function load() {
    [detail, profiles, libraries] = await Promise.all([get(`/library/titles/${id}`), getOr("/library/profiles", []), getOr("/library/libraries", [])]);
    const t = detail.title;
    view.title(t.name);
    const profile = profiles.find((p) => p.id === t.profile);
    const library = libraries.find((l) => l.id === t.library);
    const have = detail.units.filter((u) => u.file).length;
    const size = detail.units.reduce((n, u) => n + (u.file ? u.file.size : 0), 0);

    const btn = (label, fn, cls = "") => {
      const b = h("button.btn", { type: "button", class: cls }, label);
      b.addEventListener("click", () => fn(b));
      return b;
    };
    clear(view.actions,
      btn("Search releases", () => releasesDialog(t, [])),
      btn("Search wanted", (b) => act(b, () => post(`/library/titles/${id}/search`, { units: [] }), "Search started")),
      btn("Refresh", (b) => act(b, async () => { await post(`/library/titles/${id}/refresh`); await load(); }, "Refreshed")),
      btn(t.monitored ? "Unmonitor" : "Monitor", (b) => act(b, async () => { await put(`/library/titles/${id}/monitor`, { monitored: !t.monitored, units: [] }); await load(); })),
      btn("Remove", async (b) => {
        if (!(await confirm("Stop managing this title?", "Its downloads are removed. Imported files stay where they are."))) return;
        if (await act(b, () => del(`/library/titles/${id}`), "Removed") !== undefined) view.navigate("/library");
      }, "danger"));

    const ids = Object.entries(t.ids || {}).filter(([, v]) => v !== null && v !== undefined && v !== "");
    const head = h("div.detail-head",
      h("div.art", { "aria-hidden": "true" }, initial(t.name)),
      h("div.stack",
        h("div.row", h("span.badge", kindLabel(t.kind)), t.year ? h("span.badge", t.year) : null,
          t.monitored ? h("span.badge.accent", "Monitored") : h("span.badge", "Unmonitored")),
        h("dl.kv",
          h("dt", "Profile"), h("dd", profile ? profile.name : `#${t.profile}`),
          h("dt", "Library"), h("dd", library ? `${library.name} · ${library.root}` : `#${t.library}`),
          h("dt", "Files"), h("dd", `${have} of ${detail.units.length} · ${bytes(size)}`),
          h("dt", "Added"), h("dd", dateTime(t.added)),
          ids.length ? [h("dt", "Ids"), h("dd.mono", ids.map(([k, v]) => `${k} ${v}`).join(" · "))] : null)));

    const rows = detail.units.map((u) => {
      const mon = h("input", { type: "checkbox", checked: u.monitored, "aria-label": `Monitor ${u.name}` });
      mon.addEventListener("change", () => act(null, () => put(`/library/titles/${id}/monitor`, { monitored: mon.checked, units: [u.id] })));
      const search = h("button.btn.small", { type: "button" }, "Search");
      search.addEventListener("click", () => releasesDialog(t, [u.id]));
      const f = u.file;
      return h("tr",
        h("td", mon),
        h("td.nowrap.mono", unitLabel(u, t.kind)),
        h("td", h("div", u.name), f ? h("div.release-title.faint.hide-narrow", { title: f.paths.join("\n") }, f.paths[0] || f.release) : null),
        h("td.nowrap.hide-narrow.muted", u.air_date || ""),
        h("td", f ? h("span.badge.accent", qualityName(f.quality)) : h("span.badge", { class: u.monitored ? "warn" : "" }, u.monitored ? "Missing" : "Not wanted")),
        h("td.num.hide-narrow", f ? bytes(f.size) : ""),
        h("td.actions", search));
    });

    // Series: one table per season, newest first.
    let units;
    if (t.kind === "series") {
      const seasons = new Map();
      detail.units.forEach((u, i) => {
        const s = u.season ?? -1;
        if (!seasons.has(s)) seasons.set(s, []);
        seasons.get(s).push(rows[i]);
      });
      units = [...seasons.entries()].sort((a, b) => b[0] - a[0]).map(([s, r]) => h("section.stack",
        h("h3", s === 0 ? "Specials" : s < 0 ? "Other" : `Season ${s}`),
        table(["", "Episode", "Title", ["Aired", "hide-narrow"], "Status", ["Size", "num hide-narrow"], ""], r)));
    } else {
      const what = { movie: "Movie", music: "Albums", book: "Books" }[t.kind];
      units = [h("h3", what), table(["", "Year", "Title", ["Released", "hide-narrow"], "Status", ["Size", "num hide-narrow"], ""], rows, "No units yet: refresh to fetch them.")];
    }
    clear(view.body, head, h("div.stack", units));
  }
  await load();
  return onEvent("title", (tid) => { if (String(tid) === String(id)) load().catch(() => {}); });
}
