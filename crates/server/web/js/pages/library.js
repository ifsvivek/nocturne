import { h, clear, KINDS, kindLabel, tabs, table, initial, dialog, field, select, sw, dateOnly } from "../ui.js";
import { get, post, onEvent } from "../api.js";

const VIEW_KEY = "nocturne.library.view";

/** The add-title dialog; resolves to the new title id or null. */
export async function addTitleDialog(kind, preset = {}) {
  const [profiles, libraries] = await Promise.all([get("/library/profiles"), get("/library/libraries")]);
  const kindSel = select("kind", KINDS.map(([k, , one]) => [k, one]), preset.kind || kind || "movie");
  const name = h("input", { type: "text", name: "name", required: true, value: preset.name || "" });
  const year = h("input", { type: "number", name: "year", min: "1800", max: "2100", value: preset.year || "" });
  const ids = {
    imdb: h("input", { type: "text", name: "imdb", placeholder: "tt0133093", value: preset.ids?.imdb || "" }),
    tmdb: h("input", { type: "number", name: "tmdb", value: preset.ids?.tmdb || "" }),
    tvdb: h("input", { type: "number", name: "tvdb", value: preset.ids?.tvdb || "" }),
    tvmaze: h("input", { type: "number", name: "tvmaze", value: preset.ids?.tvmaze || "" }),
    musicbrainz: h("input", { type: "text", name: "musicbrainz", value: preset.ids?.musicbrainz || "" }),
    openlibrary: h("input", { type: "text", name: "openlibrary", placeholder: "OL23919A", value: preset.ids?.openlibrary || "" }),
  };
  const prof = h("select", { name: "profile", required: true });
  const lib = h("select", { name: "library", required: true });
  const monitor = select("monitor", [["all", "All"], ["future", "Future only"], ["none", "None"]], "all");
  const search = sw("search", true, "Search now");
  const idFields = h("div.form-grid.full");
  const fill = () => {
    const k = kindSel.value;
    const fam = (x) => (x === "series" ? "series" : x);
    clear(prof, profiles.filter((p) => fam(p.kind) === fam(k)).map((p) => h("option", { value: p.id }, p.name)));
    clear(lib, libraries.filter((l) => l.kind === k).map((l) => h("option", { value: l.id }, `${l.name} (${l.root})`)));
    const want = { movie: ["imdb", "tmdb"], series: ["tvdb", "tvmaze", "imdb"], music: ["musicbrainz"], book: ["openlibrary"] }[k];
    clear(idFields, want.map((w) => field(w === "musicbrainz" ? "MusicBrainz id" : w === "openlibrary" ? "Open Library id" : `${w.toUpperCase()} id`, ids[w])));
  };
  kindSel.addEventListener("change", fill);
  fill();
  const note = h("p.note", "Units (episodes, albums, books) come from TVmaze, MusicBrainz or Open Library; give the id you know so the right work is matched.");
  return dialog("Add title", [
    h("div.form-grid",
      field("Kind", kindSel), field("Name", name), field("Year", year),
      idFields,
      field("Quality profile", prof, profiles.length ? null : "Add a profile first"),
      field("Library folder", lib, libraries.length ? null : "Add a library folder under Storage first"),
      field("Monitor", monitor), h("div.field", h("span", "Search"), search)),
    note,
  ], [{ label: "Cancel", value: "cancel" }, { label: "Add", value: "add", primary: true }], {
    onSubmit: async () => {
      const num = (v) => (v === "" ? null : Number(v));
      const body = {
        kind: kindSel.value,
        name: name.value.trim(),
        year: num(year.value),
        ids: {
          imdb: ids.imdb.value.trim() || null, tmdb: num(ids.tmdb.value), tvdb: num(ids.tvdb.value),
          tvmaze: num(ids.tvmaze.value), musicbrainz: ids.musicbrainz.value.trim() || null,
          openlibrary: ids.openlibrary.value.trim() || null,
        },
        profile: Number(prof.value),
        library: Number(lib.value),
        monitor: monitor.value,
        search: search.querySelector("input").checked,
      };
      if (!prof.value || !lib.value) throw new Error("Choose a profile and a library folder.");
      const r = await post("/library/titles", body);
      return r.id;
    },
  });
}

export async function render(view) {
  view.title("Library");
  const params = new URLSearchParams(location.search);
  let kind = params.get("kind") || "";
  let mode = localStorage.getItem(VIEW_KEY) || "grid";
  let query = "";
  const add = h("button.btn.primary", { type: "button" }, "Add title");
  view.actions.append(add);
  const box = h("div");
  view.body.append(box);
  let titles = [];

  add.addEventListener("click", async () => {
    const id = await addTitleDialog(kind || "movie");
    if (id) view.navigate(`/library/${id}`);
  });

  const search = h("input", { type: "search", placeholder: "Filter", "aria-label": "Filter titles" });
  search.addEventListener("input", () => { query = search.value.toLowerCase(); draw(); });
  const modeBtn = h("button.btn", { type: "button" });
  modeBtn.addEventListener("click", () => { mode = mode === "grid" ? "list" : "grid"; localStorage.setItem(VIEW_KEY, mode); draw(); });
  const results = h("div");

  function draw() {
    modeBtn.textContent = mode === "grid" ? "List view" : "Grid view";
    modeBtn.setAttribute("aria-pressed", mode === "list" ? "true" : "false");
    const list = titles.filter((t) => !query || t.name.toLowerCase().includes(query));
    if (!titles.length) {
      clear(results, h("div.empty", "Nothing is monitored yet. Use “Add title”, or approve a request."));
    } else if (mode === "grid") {
      clear(results, h("div.posters", list.map((t) => h("a.poster", {
        href: `/library/${t.id}`, "data-link": true,
        class: t.kind === "music" ? "square" : "",
        "aria-label": `${t.name}${t.year ? ` (${t.year})` : ""}`,
      },
      h("div.art", { "aria-hidden": "true" }, initial(t.name)),
      h("div.name", t.name),
      h("div.sub", [kindLabel(t.kind), t.year, t.monitored ? null : "unmonitored"].filter(Boolean).join(" · "))))));
    } else {
      clear(results, table(["Name", "Kind", ["Year", "num"], ["Added", "hide-narrow"], "Monitored"], list.map((t) => h("tr",
        h("td", h("a", { href: `/library/${t.id}`, "data-link": true }, t.name)),
        h("td", kindLabel(t.kind)),
        h("td.num", t.year || ""),
        h("td.muted.hide-narrow", dateOnly(t.added)),
        h("td", t.monitored ? h("span.badge.accent", "Yes") : h("span.badge", "No"))))));
    }
  }

  async function load() {
    titles = await get(kind ? `/library/titles?kind=${kind}` : "/library/titles");
    titles.sort((a, b) => a.name.localeCompare(b.name));
    const bar = tabs([["", "All"], ...KINDS.map(([k, many]) => [k, many])], kind, (k) => {
      kind = k;
      history.replaceState(null, "", k ? `/library?kind=${k}` : "/library");
      load();
    });
    clear(box, bar, h("div.toolbar", search, h("span.spacer"), h("span.muted", `${titles.length} titles`), modeBtn), results);
    draw();
  }
  await load();
  return onEvent("title", () => load().catch(() => {}));
}
