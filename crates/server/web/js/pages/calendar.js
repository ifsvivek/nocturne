import { h, clear } from "../ui.js";
import { get } from "../api.js";

const pad = (n) => String(n).padStart(2, "0");
const ymd = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

// ponytail: the API has no calendar route, so this reads every title's units (one request per title, four at a time). Add a server-side calendar endpoint when libraries get large.
async function allUnits() {
  const titles = await get("/library/titles");
  const out = [];
  let i = 0;
  async function worker() {
    while (i < titles.length) {
      const t = titles[i++];
      try {
        const d = await get(`/library/titles/${t.id}`);
        for (const u of d.units) if (u.air_date) out.push({ ...u, titleName: t.name, kind: t.kind });
      } catch { /* a title that fails to load is skipped */ }
    }
  }
  await Promise.all([worker(), worker(), worker(), worker()]);
  return out;
}

function label(u) {
  if (u.kind === "series" && u.season !== null && u.season !== undefined) {
    return `${u.titleName} ${u.season}x${pad(u.episode ?? 0)}`;
  }
  return `${u.titleName}: ${u.name}`;
}

export async function render(view) {
  view.title("Calendar");
  const now = new Date();
  let month = new Date(now.getFullYear(), now.getMonth(), 1);
  const units = await allUnits();
  const byDay = new Map();
  for (const u of units) {
    if (!byDay.has(u.air_date)) byDay.set(u.air_date, []);
    byDay.get(u.air_date).push(u);
  }
  const heading = h("h2");
  const prev = h("button.btn", { type: "button", "aria-label": "Previous month" }, "‹");
  const next = h("button.btn", { type: "button", "aria-label": "Next month" }, "›");
  const today = h("button.btn", { type: "button" }, "Today");
  const grid = h("div.cal");
  const agenda = h("div.agenda");
  prev.addEventListener("click", () => { month = new Date(month.getFullYear(), month.getMonth() - 1, 1); draw(); });
  next.addEventListener("click", () => { month = new Date(month.getFullYear(), month.getMonth() + 1, 1); draw(); });
  today.addEventListener("click", () => { month = new Date(now.getFullYear(), now.getMonth(), 1); draw(); });
  clear(view.body, h("div.toolbar", prev, today, next, heading), grid, agenda);

  const ev = (u) => h("a.ev", {
    href: `/library/${u.title}`, "data-link": true, class: u.file ? "has" : "",
    title: `${label(u)}${u.file ? " (downloaded)" : ""}`,
  }, label(u));

  function draw() {
    heading.textContent = month.toLocaleDateString(undefined, { month: "long", year: "numeric" });
    const start = new Date(month);
    start.setDate(1 - ((month.getDay() + 6) % 7)); // weeks start on Monday
    const days = [];
    for (let i = 0; i < 42; i++) days.push(new Date(start.getFullYear(), start.getMonth(), start.getDate() + i));
    const names = days.slice(0, 7).map((d) => h("div.dow", d.toLocaleDateString(undefined, { weekday: "short" })));
    const todayKey = ymd(now);
    clear(grid, names, days.map((d) => {
      const key = ymd(d);
      const items = byDay.get(key) || [];
      return h("div.day", { class: [d.getMonth() !== month.getMonth() ? "out" : "", key === todayKey ? "today" : ""].join(" ").trim() },
        h("span.n", d.getDate()), items.map(ev));
    }));
    // Narrow screens: a list of this month's days that have something.
    const inMonth = days.filter((d) => d.getMonth() === month.getMonth() && byDay.has(ymd(d)));
    clear(agenda, inMonth.length
      ? inMonth.map((d) => h("div.card.stack", h("h3", d.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })), (byDay.get(ymd(d)) || []).map(ev)))
      : h("div.empty", "Nothing airs or releases this month."));
  }
  draw();
}
