// DOM and formatting helpers. Everything is built with createElement and textContent: server data never reaches innerHTML.

/** h("div.card#id", {attrs, on: {click}}, ...children). Children: nodes, strings, numbers, arrays, null/false (skipped). */
export function h(spec, attrs, ...kids) {
  if (attrs === null || typeof attrs !== "object" || attrs instanceof Node || Array.isArray(attrs)) {
    kids.unshift(attrs);
    attrs = {};
  }
  // "tag#id.class1.class2", id and classes in any order.
  const parts = spec.match(/^[^.#]*|[.#][^.#]*/g);
  const el = document.createElement(parts[0] || "div");
  const classes = [];
  for (const p of parts.slice(1)) {
    if (p[0] === "#") el.id = p.slice(1);
    else classes.push(p.slice(1));
  }
  if (classes.length) el.className = classes.join(" ");
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "on") for (const [ev, fn] of Object.entries(v)) el.addEventListener(ev, fn);
    else if (k === "class") el.className = [el.className, v].filter(Boolean).join(" ");
    else if (k === "value") el.value = v;
    else if (k === "checked" || k === "selected" || k === "disabled" || k === "required") el[k] = !!v;
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  append(el, kids);
  return el;
}

function append(el, kids) {
  for (const k of kids) {
    if (k === null || k === undefined || k === false) continue;
    if (Array.isArray(k)) append(el, k);
    else el.append(k instanceof Node ? k : String(k));
  }
}

export function clear(el, ...kids) {
  el.replaceChildren();
  append(el, kids);
  return el;
}

export const KINDS = [
  ["movie", "Movies", "Movie"],
  ["series", "Shows", "Show"],
  ["music", "Music", "Artist"],
  ["book", "Books", "Author"],
];
export const kindLabel = (k) => (KINDS.find((x) => x[0] === k) || [k, k, k])[2];

// Contract order is worst to best within each family.
export const QUALITIES = {
  video: ["unknown", "cam", "telesync", "telecine", "dvd", "sdtv", "web_rip480", "web_dl480", "bluray480", "bluray576", "hdtv720", "web_rip720", "web_dl720", "bluray720", "hdtv1080", "web_rip1080", "web_dl1080", "bluray1080", "remux1080", "hdtv2160", "web_rip2160", "web_dl2160", "bluray2160", "remux2160"],
  music: ["unknown", "mp3_low", "mp3_192", "mp3_vbr", "mp3_256", "mp3_320", "aac", "ogg", "alac", "flac", "flac24"],
  book: ["unknown", "pdf", "mobi", "azw3", "epub", "audiobook_mp3", "audiobook_m4b"],
};
export const qualitiesFor = (kind) => (kind === "music" ? QUALITIES.music : kind === "book" ? QUALITIES.book : QUALITIES.video);

const QNAMES = {
  unknown: "Unknown", cam: "CAM", telesync: "TELESYNC", telecine: "TELECINE", dvd: "DVD", sdtv: "SDTV",
  remux1080: "Remux-1080p", remux2160: "Remux-2160p", mp3_low: "MP3 (low)", mp3_192: "MP3-192", mp3_vbr: "MP3-VBR",
  mp3_256: "MP3-256", mp3_320: "MP3-320", aac: "AAC", ogg: "OGG Vorbis", alac: "ALAC", flac: "FLAC", flac24: "FLAC 24bit",
  pdf: "PDF", mobi: "MOBI", azw3: "AZW3", epub: "EPUB", audiobook_mp3: "MP3 audiobook", audiobook_m4b: "M4B audiobook",
};
export function qualityName(q) {
  if (!q) return "";
  if (QNAMES[q]) return QNAMES[q];
  const m = /^(web_rip|web_dl|bluray|hdtv)(\d+)$/.exec(q);
  if (!m) return q;
  const src = { web_rip: "WEBRip", web_dl: "WEBDL", bluray: "Bluray", hdtv: "HDTV" }[m[1]];
  return `${src}-${m[2]}p`;
}

export function bytes(n) {
  if (n === null || n === undefined) return "";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = Number(n);
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(i && v < 10 ? 1 : 0)} ${u[i]}`;
}
export const rate = (n) => (n ? `${bytes(n)}/s` : "0 B/s");

export function duration(secs) {
  if (secs === null || secs === undefined) return "";
  secs = Math.max(0, Math.round(secs));
  const d = Math.floor(secs / 86400), hh = Math.floor((secs % 86400) / 3600), m = Math.floor((secs % 3600) / 60);
  if (d) return `${d}d ${hh}h`;
  if (hh) return `${hh}h ${m}m`;
  if (m) return `${m}m`;
  return `${secs}s`;
}

export function when(ts) {
  if (!ts) return "never";
  const diff = Date.now() / 1000 - ts;
  const abs = Math.abs(diff);
  const s = abs < 60 ? "moments" : duration(abs);
  return diff >= 0 ? `${s} ago` : `in ${s}`;
}
export const dateTime = (ts) => (ts ? new Date(ts * 1000).toLocaleString() : "");
export const dateOnly = (ts) => (ts ? new Date(ts * 1000).toLocaleDateString() : "");

export function toast(msg, error = false) {
  const t = h("div.toast", { class: error ? "error" : "" }, msg);
  document.getElementById("toasts").append(t);
  setTimeout(() => t.remove(), error ? 8000 : 4000);
}

/** Run an async action from a button: disables it meanwhile, toasts errors. */
export async function act(btn, fn, okMsg) {
  if (btn) btn.disabled = true;
  try {
    const r = await fn();
    if (okMsg) toast(okMsg);
    return r;
  } catch (e) {
    toast(e.message || String(e), true);
  } finally {
    if (btn) btn.disabled = false;
  }
}

/** A modal <dialog>. `body` nodes; `buttons`: [{label, primary, danger, value}] ; resolves to the clicked value or null. */
export function dialog(title, body, buttons, { wide = false, onSubmit } = {}) {
  return new Promise((resolve) => {
    const d = h("dialog", { class: wide ? "wide" : "", "aria-labelledby": "dlg-title" });
    const form = h("form", { method: "dialog" });
    let result = null;
    const foot = h("footer",
      buttons.map((b) => h("button.btn", {
        type: b.value === "cancel" ? "button" : "submit",
        class: b.primary ? "primary" : b.danger ? "danger" : "",
        value: b.value,
        on: b.value === "cancel" ? { click: () => d.close() } : {},
      }, b.label)));
    form.append(h("header", h("h2#dlg-title", title)), h("div.body", body), foot);
    form.addEventListener("submit", async (ev) => {
      const v = ev.submitter ? ev.submitter.value : "ok";
      if (onSubmit) {
        ev.preventDefault();
        const btn = ev.submitter;
        if (btn) btn.disabled = true;
        try {
          const r = await onSubmit(v, form);
          if (r !== false) { result = r === undefined ? v : r; d.close(); }
        } catch (e) {
          toast(e.message || String(e), true);
        } finally {
          if (btn) btn.disabled = false;
        }
      } else {
        result = v;
      }
    });
    d.append(form);
    d.addEventListener("close", () => { d.remove(); resolve(result); });
    document.body.append(d);
    d.showModal();
    const first = form.querySelector("input, select, textarea");
    if (first) first.focus();
  });
}

export const confirm = (title, text, label = "Confirm", danger = true) =>
  dialog(title, [h("p", text)], [
    { label: "Cancel", value: "cancel" },
    { label, value: "yes", primary: !danger, danger },
  ]).then((v) => v === "yes");

export function field(label, input, hint) {
  return h("label.field", h("span", label), input, hint ? h("small", hint) : null);
}

export function sw(name, checked, label) {
  const input = h("input", { type: "checkbox", name, checked, role: "switch" });
  const word = h("span", checked ? "ON" : "OFF");
  input.addEventListener("change", () => { word.textContent = input.checked ? "ON" : "OFF"; });
  return h("label.switch", input, word, label ? h("span.muted", label) : null);
}

export function select(name, options, value) {
  return h("select", { name }, options.map(([v, l]) => h("option", { value: v, selected: String(v) === String(value) }, l)));
}

export function tabs(items, current, onPick) {
  const bar = h("div.tabs", { role: "tablist" });
  for (const [key, label] of items) {
    bar.append(h("button", {
      type: "button", role: "tab", "aria-selected": key === current ? "true" : "false",
      on: { click: () => onPick(key) },
    }, label));
  }
  return bar;
}

export function table(headers, rows, empty = "Nothing here yet.") {
  if (!rows.length) return h("div.empty", empty);
  return h("div.table-wrap", h("table",
    h("thead", h("tr", headers.map((x) => (Array.isArray(x) ? h("th", { class: x[1], scope: "col" }, x[0]) : h("th", { scope: "col" }, x))))),
    h("tbody", rows)));
}

export function initial(name) {
  const m = /[\p{L}\p{N}]/u.exec((name || "").replace(/^(the|a|an)\s+/i, ""));
  return m ? m[0].toUpperCase() : "?";
}

export function lines(text) {
  return (text || "").split("\n").map((s) => s.trim()).filter(Boolean);
}
