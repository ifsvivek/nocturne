// REST client for /api/v1 (docs/api/openapi.yaml) and the events WebSocket.

const KEY = "nocturne.token";
export const token = {
  get: () => localStorage.getItem(KEY),
  set: (t) => localStorage.setItem(KEY, t),
  clear: () => localStorage.removeItem(KEY),
};

export class ApiError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

let onUnauthorized = () => {};
export function setUnauthorizedHandler(fn) { onUnauthorized = fn; }

export async function api(method, path, body) {
  const headers = { Accept: "application/json" };
  const t = token.get();
  if (t) headers.Authorization = `Bearer ${t}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";
  let r;
  try {
    r = await fetch(`/api/v1${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  } catch {
    throw new ApiError(0, "The server cannot be reached.");
  }
  const text = await r.text();
  let data = null;
  if (text) {
    try { data = JSON.parse(text); } catch { data = text; }
  }
  if (!r.ok) {
    const msg = (data && data.error) || {
      401: "Please sign in again.",
      403: "Not allowed.",
      404: "Not found.",
      503: "That module is switched off.",
    }[r.status] || `Request failed (${r.status}).`;
    if (r.status === 401 && !path.startsWith("/auth/login") && !path.startsWith("/setup")) onUnauthorized();
    throw new ApiError(r.status, msg);
  }
  return data;
}

export const get = (p) => api("GET", p);
export const post = (p, b) => api("POST", p, b);
export const put = (p, b) => api("PUT", p, b);
export const del = (p) => api("DELETE", p);

/** GET that resolves to `fallback` instead of failing when a module is off (503) or a route is missing (404). */
export async function getOr(p, fallback) {
  try { return await get(p); } catch (e) {
    if (e.status === 503 || e.status === 404) return fallback;
    throw e;
  }
}

/** GET that never throws for 404/503: `{state: "ok"|"off"|"missing", data}`. For module routes that may not exist in this server version. */
export async function probe(p) {
  try { return { state: "ok", data: await get(p) }; } catch (e) {
    if (e.status === 503) return { state: "off", data: null };
    if (e.status === 404) return { state: "missing", data: null };
    throw e;
  }
}

// Events: one socket for the whole app, reconnecting with backoff; listeners by event type ("*" for all).
const listeners = new Map();
let socket = null;
let retry = 1000;
let stateCb = () => {};
export function onEvent(type, fn) {
  if (!listeners.has(type)) listeners.set(type, new Set());
  listeners.get(type).add(fn);
  return () => listeners.get(type).delete(fn);
}
export function onSocketState(fn) { stateCb = fn; }

export function connectEvents() {
  const t = token.get();
  if (!t || socket) return;
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const ws = new WebSocket(`${proto}//${location.host}/api/v1/events?token=${encodeURIComponent(t)}`);
  socket = ws;
  ws.addEventListener("open", () => { retry = 1000; stateCb(true); });
  ws.addEventListener("message", (m) => {
    let ev;
    try { ev = JSON.parse(m.data); } catch { return; }
    for (const key of [ev.type, "*"]) for (const fn of listeners.get(key) || []) fn(ev.data, ev);
  });
  ws.addEventListener("close", () => {
    stateCb(false);
    if (socket !== ws) return;
    socket = null;
    if (token.get()) setTimeout(connectEvents, retry);
    retry = Math.min(retry * 2, 30000);
  });
}
export function disconnectEvents() {
  const s = socket;
  socket = null;
  if (s) s.close();
}
