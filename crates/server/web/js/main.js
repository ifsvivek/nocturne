// Boot, sign-in/setup gate, shell and client-side router. The server answers index.html for unknown paths outside /api.
import { h, clear, toast } from "./ui.js";
import { get, post, token, setUnauthorizedHandler, connectEvents, disconnectEvents, onSocketState, onEvent } from "./api.js";

const NAV = [
  [null, [
    ["/", "Dashboard", "dashboard"],
    ["/library", "Library", "library"],
    ["/calendar", "Calendar", "calendar"],
    ["/queue", "Queue", "queue"],
    ["/requests", "Requests", "requests"],
    ["/history", "History", "history"],
  ]],
  ["Setup", [
    ["/indexers", "Indexers", "indexers"],
    ["/profiles", "Profiles", "profiles"],
    ["/storage", "Storage", "storage"],
    ["/gateway", "Gateway & remote", "gateway"],
    ["/servers", "Servers", "servers"],
  ]],
  ["System", [
    ["/settings", "Settings", "settings"],
    ["/apikeys", "API keys", "apikeys"],
  ]],
];

const ROUTES = [
  [/^\/$/, "dashboard"],
  [/^\/library\/(\d+)$/, "title"],
  [/^\/library$/, "library"],
  [/^\/(calendar|queue|requests|history|indexers|profiles|storage|gateway|servers|settings|apikeys)$/, null],
];

const app = document.getElementById("app");
let shell = null;
let cleanup = null;
// Focus moves to the content on navigation, not on the first page load (then Tab reaches the skip link first).
let routed = false;

export function navigate(path, replace = false) {
  if (replace) history.replaceState(null, "", path);
  else history.pushState(null, "", path);
  route();
}

function brand() {
  return h("div.brand", h("img", { src: "/favicon.svg", alt: "" }), "NOCTURNE");
}

async function boot() {
  setUnauthorizedHandler(() => {
    token.clear();
    disconnectEvents();
    shell = null;
    authScreen(false);
  });
  let needed = false;
  try {
    needed = (await get("/setup")).needed;
  } catch (e) {
    clear(app, h("div.auth", h("div.card", brand(), h("p.error-box", e.message), h("button.btn", { on: { click: boot } }, "Try again"))));
    return;
  }
  if (needed) return authScreen(true);
  if (!token.get()) return authScreen(false);
  start();
}

function authScreen(setup) {
  document.title = setup ? "Set up NOCTURNE" : "Sign in · NOCTURNE";
  const err = h("p.error-box.hidden", { role: "alert" });
  const user = h("input", { type: "text", name: "username", autocomplete: "username", required: true });
  const fill = `${setup ? "new" : "current"}-password`;
  const pass = h("input", { type: "password", name: "password", autocomplete: fill, required: true, minlength: setup ? 8 : undefined });
  const pass2 = setup ? h("input", { type: "password", name: "password2", autocomplete: "new-password", required: true, minlength: 8 }) : null;
  const btn = h("button.btn.primary", { type: "submit" }, setup ? "Create admin account" : "Sign in");
  const form = h("form", { on: { submit: async (ev) => {
    ev.preventDefault();
    err.classList.add("hidden");
    if (setup && pass.value !== pass2.value) {
      err.textContent = "The passwords do not match.";
      err.classList.remove("hidden");
      return;
    }
    btn.disabled = true;
    try {
      const r = await post(setup ? "/setup" : "/auth/login", { username: user.value.trim(), password: pass.value });
      token.set(r.token);
      if (location.pathname === "/login" || location.pathname === "/setup") history.replaceState(null, "", "/");
      start();
    } catch (e) {
      err.textContent = e.status === 401 ? "Wrong name or password." : e.message;
      err.classList.remove("hidden");
    } finally {
      btn.disabled = false;
    }
  } } },
    h("h1", setup ? "Welcome" : "Sign in"),
    setup ? h("p.muted", "Create the administrator account for this server. Passwords need at least 8 characters.") : null,
    err,
    h("label.field", h("span", "Name"), user),
    h("label.field", h("span", "Password"), pass),
    setup ? h("label.field", h("span", "Repeat password"), pass2) : null,
    btn);
  clear(app, h("main.auth#main", h("div.card", brand(), form)));
  user.focus();
}

function start() {
  connectEvents();
  buildShell();
  route();
}

function buildShell() {
  const live = h("span.dot", { "aria-hidden": "true" });
  const liveText = h("span", "Connecting");
  onSocketState((up) => {
    live.className = up ? "dot on" : "dot bad";
    liveText.textContent = up ? "Live" : "Offline";
  });
  const nav = h("nav.nav", { "aria-label": "Main" },
    brand(),
    NAV.map(([group, items]) => [
      group ? h("div.group", group) : null,
      h("ul", items.map(([href, label]) => h("li", h("a", { href, "data-link": true }, label)))),
    ]),
    h("div.foot", live, liveText, h("span.spacer"),
      h("button.btn.small", { type: "button", on: { click: signOut } }, "Sign out")));
  const main = h("main#main", { tabindex: "-1" });
  shell = { el: h("div.shell", nav, main), nav, main };
  shell.el.addEventListener("click", (ev) => {
    if (ev.target === shell.el && shell.el.classList.contains("nav-open")) shell.el.classList.remove("nav-open");
  });
  clear(app, shell.el);
  onEvent("health", (m) => {
    if (m && m.error) toast(`${m.name}: ${m.error}`, true);
  });
}

async function signOut() {
  try { await post("/auth/logout"); } catch { /* the token is dropped either way */ }
  token.clear();
  disconnectEvents();
  shell = null;
  history.replaceState(null, "", "/");
  authScreen(false);
}

function menuButton() {
  return h("button.btn.icon.menu-btn", {
    type: "button", "aria-label": "Menu",
    on: { click: () => shell.el.classList.toggle("nav-open") },
  }, "☰");
}

async function route() {
  if (!shell) return;
  if (cleanup) { try { cleanup(); } catch { /* ignore */ } cleanup = null; }
  shell.el.classList.remove("nav-open");
  const path = location.pathname.replace(/\/+$/, "") || "/";
  let page = null;
  let params = [];
  for (const [re, name] of ROUTES) {
    const m = re.exec(path);
    if (m) { page = name || m[1]; params = m.slice(1); break; }
  }
  for (const a of shell.nav.querySelectorAll("a[data-link]")) {
    const href = a.getAttribute("href");
    const on = href === "/" ? path === "/" : path === href || path.startsWith(`${href}/`);
    if (on) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  }
  const title = h("h1");
  const actions = h("div.row");
  const head = h("header.page-head", menuButton(), title, h("span.spacer"), actions);
  const body = h("div.page-body");
  clear(shell.main, head, body);
  const view = {
    body, actions,
    title(t) { title.textContent = t; document.title = `${t} · NOCTURNE`; },
    navigate,
  };
  if (!page) {
    view.title("Not found");
    body.append(h("div.empty", "There is no page here. ", h("a", { href: "/", "data-link": true }, "Go to the dashboard")));
    return;
  }
  view.title("");
  body.append(h("p.muted", "Loading…"));
  try {
    const mod = await import(`./pages/${page}.js`);
    clear(body);
    const r = await mod.render(view, ...params);
    if (typeof r === "function") cleanup = r;
  } catch (e) {
    if (e.status === 401) return;
    const settings = h("a", { href: "/settings", "data-link": true }, "Settings");
    if (e.status === 503) clear(body, h("div.empty", "This module is switched off. Turn it on in ", settings, "."));
    else if (e.status === 404) clear(body, h("div.empty", "Not available: this server does not include this module yet, or the item is gone. Modules are listed in ", settings, "."));
    else clear(body, h("div.error-box", { role: "alert" }, e.message || String(e)));
  }
  if (routed) shell.main.focus({ preventScroll: true });
  routed = true;
}

document.addEventListener("click", (ev) => {
  const a = ev.target.closest && ev.target.closest("a[data-link]");
  if (!a || ev.ctrlKey || ev.metaKey || ev.shiftKey || ev.button !== 0) return;
  ev.preventDefault();
  navigate(a.getAttribute("href"));
});
window.addEventListener("popstate", route);
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape" && shell && shell.el.classList.contains("nav-open")) shell.el.classList.remove("nav-open");
});

boot();
