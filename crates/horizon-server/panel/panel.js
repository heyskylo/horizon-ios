// The Horizon server panel (docs/server.md, section 6). It polls the control API once a second through the
// panel's session; every change carries the X-Horizon header. Text from the server is only ever set with
// textContent, never as HTML.
"use strict";

const $ = (id) => document.getElementById(id);

// --- Talking to the server -------------------------------------------------------------------------------

class SignedOut extends Error {}

async function api(method, path, body) {
  let response;
  try {
    response = await fetch(path, {
      method,
      credentials: "same-origin",
      headers: { "X-Horizon": "1", "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (e) {
    throw new Error("the server isn't answering");
  }
  if (response.status === 401) throw new SignedOut();
  const value = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(value.error || `error ${response.status}`);
  return value;
}

/// Runs a change and says how it went; returns the reply, or null when it failed.
async function act(method, path, body, done) {
  try {
    const reply = await api(method, path, body);
    const said = reply && Array.isArray(reply.output) ? reply.output.join("\n") : "";
    toast(said || done || "Done.");
    return reply;
  } catch (e) {
    if (e instanceof SignedOut) show("signed-out");
    toast(e.message, true);
    return null;
  }
}

// --- Small helpers -------------------------------------------------------------------------------------

/// An element: h("li", { class: "x", onclick }, child, "text", ...).
function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props || {})) {
    if (value == null || value === false) continue;
    if (key.startsWith("on")) el.addEventListener(key.slice(2), value);
    else if (key === "class") el.className = value;
    else el.setAttribute(key, value === true ? "" : value);
  }
  for (const child of children.flat()) {
    if (child == null || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

function icon(name) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

function button(label, action, { kind, iconName, small = true, title } = {}) {
  return h(
    "button",
    { type: "button", class: [kind, small && "small"].filter(Boolean).join(" "), title, "aria-label": title, onclick: action },
    iconName && icon(iconName),
    label && h("span", { class: "label" }, label),
  );
}

function duration(seconds) {
  if (seconds == null) return "";
  const d = Math.floor(seconds / 86400), hr = Math.floor(seconds / 3600) % 24, m = Math.floor(seconds / 60) % 60;
  if (d) return `${d} d ${hr} h`;
  if (hr) return `${hr} h ${m} min`;
  return m ? `${m} min` : `${seconds} s`;
}

function bytes(n) {
  if (n == null) return "–";
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  return `${Math.max(1, Math.ceil(n / 1e3))} kB`;
}

/// "2026-11-04_18-00-00-name.zip" (backup::file_name) → a date and the name given.
function backupParts(name) {
  const m = /^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})-(\d{2})-?(.*?)(\.zip)?$/.exec(name);
  if (!m) return { when: null, label: name.replace(/\.zip$/, "") };
  const when = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]);
  return { when, label: m[7] };
}

const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
function ago(date) {
  const s = Math.round((Date.now() - date.getTime()) / 1000);
  if (s < 90) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

/// A steady colour per player name, for the letter avatar (the panel loads no skins from elsewhere).
function avatar(name) {
  let hash = 0;
  for (const c of name) hash = (hash * 31 + c.codePointAt(0)) >>> 0;
  const el = h("span", { class: "avatar", "aria-hidden": "true" }, name.slice(0, 1));
  el.style.background = `hsl(${hash % 360} 55% 42%)`;
  return el;
}

function ping(ms) {
  const quality = ms == null ? "" : ms < 80 ? "good" : ms < 200 ? "fair" : "poor";
  return h("span", { class: "row-side" }, h("span", { class: `ping ${quality}`, "aria-hidden": "true" }, h("i"), h("i"), h("i")), ms == null ? "–" : `${ms} ms`);
}

function fill(listId, emptyId, items) {
  $(listId).replaceChildren(...items);
  if (emptyId) $(emptyId).hidden = items.length > 0;
}

// --- Toasts and the dialog ------------------------------------------------------------------------------

function toast(text, error) {
  const el = h("div", { class: `toast${error ? " error" : ""}` }, text);
  $("toasts").append(el);
  setTimeout(() => el.remove(), error ? 7000 : 4000);
  while ($("toasts").children.length > 3) $("toasts").firstChild.remove();
}

/// Asks before a change: resolves to the fields' values (an object), or null when cancelled.
function ask({ title, text, ok = "OK", danger = false, fields = [] }) {
  const dialog = $("dialog");
  $("dialog-title").textContent = title;
  $("dialog-text").textContent = text || "";
  $("dialog-text").hidden = !text;
  $("dialog-ok").textContent = ok;
  $("dialog-ok").className = danger ? "primary destructive-fill" : "primary";
  const inputs = fields.map((f) => {
    const input = f.options
      ? h("select", { name: f.name }, f.options.map(([value, label]) => h("option", { value }, label)))
      : h("input", { name: f.name, placeholder: f.placeholder || "", maxlength: f.max || 200, autocomplete: "off" });
    return [f.name, input, h("label", {}, f.label, input)];
  });
  $("dialog-fields").replaceChildren(...inputs.map((i) => i[2]));
  dialog.returnValue = "";
  dialog.showModal();
  (inputs[0] ? inputs[0][1] : $("dialog-ok")).focus();
  return new Promise((resolve) => {
    dialog.addEventListener(
      "close",
      () => resolve(dialog.returnValue === "ok" ? Object.fromEntries(inputs.map(([n, i]) => [n, i.value.trim()])) : null),
      { once: true },
    );
  });
}

// --- Pages ----------------------------------------------------------------------------------------------

const PAGES = {
  overview: ["Overview", "How the server is doing right now."],
  players: ["Players", "Who is online, who may join and who is banned."],
  console: ["Console", "The server's log as it happens. Commands run as if typed in the server's console."],
  backups: ["Backups", "Zips of the world and the settings in backups/. Restoring keeps the current world as a backup."],
  reports: ["Crash reports", "The game writes one each time it stops unexpectedly."],
};
let page = "overview";

function route() {
  const wanted = location.hash.slice(1);
  page = PAGES[wanted] ? wanted : "overview";
  for (const section of document.querySelectorAll(".page")) section.hidden = section.dataset.page !== page;
  for (const link of document.querySelectorAll(".nav a")) {
    if (link.dataset.page === page) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
  $("page-title").textContent = PAGES[page][0];
  $("page-lead").textContent = PAGES[page][1];
  document.title = `${PAGES[page][0]} · ${state.config ? state.config.server.motd : "Horizon server"}`;
  if (page === "console") scrollLog(true);
  if (state.status) renderUpdate(state.status.update);
  renderCharts();
}

function show(view) {
  $("app").hidden = view !== "app";
  $("signed-out").hidden = view !== "signed-out";
  $("gone").hidden = view !== "gone";
}

// --- State ----------------------------------------------------------------------------------------------

const state = {
  status: null,
  samples: [],
  metricsSince: 0,
  logNext: 0,
  lines: [],
  players: null,
  backups: [],
  crashes: [],
  config: null,
};

// --- Status, power and update ---------------------------------------------------------------------------

function renderStatus(status) {
  state.status = status;
  const phase = $("phase");
  phase.textContent = status.phase;
  phase.className = `state ${status.phase}`;
  $("uptime").textContent = status.engine_uptime_seconds != null ? `· up ${duration(status.engine_uptime_seconds)}` : "";
  $("version").textContent = `horizon-server ${status.version}`;
  const stopped = status.phase === "stopped";
  $("start").hidden = !stopped;
  $("restart").hidden = stopped;
  const crashes = state.crashes.length;
  $("nav-reports").textContent = crashes ? String(crashes) : "";
  renderUpdate(status.update);
  renderOnline();
  renderFacts();
}

// Updates (horizon-server update): a newer release, one under way, or how the last one ended.
function renderUpdate(update) {
  const offer = update && !update.busy && update.available;
  let text = "";
  if (update && update.busy) text = `Updating: ${update.busy}…`;
  else if (offer) text = `Horizon ${update.available} is out. This server runs ${update.installed}.`;
  else if (update && update.last && !update.ok) text = update.last;
  $("update").hidden = !text || page !== "overview";
  $("update").classList.toggle("failed", Boolean(update && update.last && !update.ok && !update.busy && !offer));
  $("update-text").textContent = text;
  $("update-button").hidden = !offer;
}

function onlinePlayers() {
  const game = state.status && state.status.sample && state.status.sample.game;
  return (game && game.players) || [];
}

function maxPlayers() {
  const game = state.status && state.status.sample && state.status.sample.game;
  return (game && game.max_players) || (state.config && state.config.server.max_players) || null;
}

// --- Charts ---------------------------------------------------------------------------------------------

function sparkline(id, values, { max, line } = {}) {
  const canvas = $(id);
  if (!canvas || !canvas.offsetParent) return;
  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth, height = canvas.clientHeight;
  if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) {
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
  }
  const g = canvas.getContext("2d");
  g.setTransform(ratio, 0, 0, ratio, 0, 0);
  g.clearRect(0, 0, width, height);
  const css = getComputedStyle(document.documentElement);
  const accent = css.getPropertyValue("--accent-soft").trim();
  const known = values.filter((v) => v != null);
  const top = Math.max(max || 0, ...known, 1e-9) * 1.08;
  const y = (v) => height - 2 - (v / top) * (height - 4);
  if (line != null) {
    g.strokeStyle = css.getPropertyValue("--rule").trim();
    g.lineWidth = 1;
    g.setLineDash([3, 4]);
    g.beginPath();
    g.moveTo(0, y(line));
    g.lineTo(width, y(line));
    g.stroke();
    g.setLineDash([]);
  }
  const span = 600;
  const x = (i) => width - ((values.length - 1 - i) / (span - 1)) * width;
  // Runs of known values, each drawn as a line over a soft fill.
  const runs = [];
  values.forEach((v, i) => {
    if (v == null) return;
    if (!runs.length || values[i - 1] == null) runs.push([]);
    runs[runs.length - 1].push([x(i), y(v)]);
  });
  const gradient = g.createLinearGradient(0, 0, 0, height);
  gradient.addColorStop(0, `color-mix(in srgb, ${accent} 30%, transparent)`);
  gradient.addColorStop(1, "transparent");
  for (const run of runs) {
    g.beginPath();
    run.forEach(([px, py], i) => (i ? g.lineTo(px, py) : g.moveTo(px, py)));
    g.strokeStyle = accent;
    g.lineWidth = 1.6;
    g.lineJoin = "round";
    g.stroke();
    g.lineTo(run[run.length - 1][0], height);
    g.lineTo(run[0][0], height);
    g.closePath();
    g.fillStyle = gradient;
    g.fill();
  }
}

function renderCharts() {
  if (page !== "overview") return;
  const s = state.samples;
  const last = s[s.length - 1] || {};
  const tps = last.tps, mspt = last.mspt;
  $("tps-now").textContent = tps == null ? "–" : tps.toFixed(1);
  $("tps-now").className = tps == null ? "none" : tps < 15 ? "bad" : tps < 19.5 ? "meh" : "";
  $("mspt-now").textContent = mspt == null ? "–" : `${mspt.toFixed(1)} ms`;
  $("mspt-now").className = mspt == null ? "none" : mspt > 50 ? "bad" : mspt > 40 ? "meh" : "";
  $("mspt-note").textContent = last.mspt_p95 != null ? `95 % of ticks under ${last.mspt_p95.toFixed(1)} ms · budget 50 ms` : "budget 50 ms";
  const online = onlinePlayers().length, cap = maxPlayers();
  $("players-now").textContent = state.status && state.status.sample && state.status.sample.game ? `${online}${cap ? ` / ${cap}` : ""}` : "–";
  const peak = Math.max(0, ...s.map((x) => x.players || 0));
  $("players-note").textContent = s.length ? `peak ${peak} in the last ${Math.max(1, Math.round(s.length / 60))} min` : "";
  $("ram-now").textContent = bytes(last.rss_bytes);
  $("cpu-note").textContent = last.cpu_percent == null ? "" : `CPU ${Math.round(last.cpu_percent)} % of a core`;
  sparkline("tps", s.map((x) => x.tps), { max: 20, line: 20 });
  sparkline("mspt", s.map((x) => x.mspt), { max: 50, line: 50 });
  sparkline("players-chart", s.map((x) => x.players), { max: cap || 1 });
  sparkline("ram", s.map((x) => (x.rss_bytes == null ? null : x.rss_bytes / 1e9)));
}

// --- Overview -------------------------------------------------------------------------------------------

function renderOnline() {
  const players = onlinePlayers();
  $("nav-players").textContent = state.status && state.status.sample && state.status.sample.game ? String(players.length) : "";
  fill("overview-players", "overview-no-players", players.slice(0, 6).map((p) => h("li", {}, avatar(p.name), h("span", { class: "row-main" }, h("b", {}, p.name), isOp(p.name) && h("small", {}, "Operator")), ping(p.ping_ms))));
  if (page === "players") renderPlayers();
}

function renderFacts() {
  const c = state.config, s = state.status;
  if (!c || !s) return;
  $("server-name").textContent = c.server.motd || "Horizon server";
  $("server-address").textContent = c.server.bind ? `${c.server.bind}:${c.server.port}` : `port ${c.server.port}`;
  const last = state.backups.map((b) => backupParts(b.name).when).filter(Boolean).sort((a, b) => b - a)[0];
  const facts = [
    ["Game", `${cap(c.world.game_mode)}, ${c.world.difficulty}${c.world.hardcore ? ", hardcore" : ""}`],
    ["World", c.world.name],
    ["Who can join", c.server.whitelist ? "Whitelisted players" : "Everyone"],
    ["Accounts", c.server.online_mode ? "Checked with Minecraft" : "Not checked (offline mode)"],
    ["Last backup", last ? `${ago(last)} · every ${c.backups.every}` : `none yet · every ${c.backups.every}`],
    ["Supervisor up", duration(s.uptime_seconds)],
  ];
  if (s.crashes) facts.push(["Crashes", `${s.crashes} since the supervisor started`]);
  $("facts").replaceChildren(...facts.flatMap(([k, v]) => [h("dt", {}, k), h("dd", {}, v)]));
}

const cap = (text) => (text ? text[0].toUpperCase() + text.slice(1) : "");

// --- Players --------------------------------------------------------------------------------------------

function isOp(name) {
  return Boolean(state.players && state.players.ops.some((o) => o.name.toLowerCase() === name.toLowerCase()));
}

async function kick(name) {
  const answer = await ask({ title: `Kick ${name}?`, text: "They can join again straight away.", ok: "Kick", danger: true, fields: [{ name: "reason", label: "Reason (optional)", placeholder: "Shown to the player" }] });
  if (answer) await act("POST", "/api/v1/players/kick", { name, reason: answer.reason || undefined }, `Kicked ${name}.`);
}

async function ban(name) {
  const answer = await ask({ title: `Ban ${name}?`, text: "They are kicked and can't join until you pardon them.", ok: "Ban", danger: true, fields: [{ name: "reason", label: "Reason (optional)", placeholder: "Shown to the player" }] });
  if (answer && (await act("POST", "/api/v1/players/ban", { name, reason: answer.reason || undefined }, `Banned ${name}.`))) refreshLists();
}

async function toggleOp(name) {
  const op = isOp(name);
  if (await act("POST", op ? "/api/v1/players/deop" : "/api/v1/players/op", { name }, op ? `${name} is no longer an operator.` : `${name} is now an operator.`)) refreshLists();
}

function renderPlayers() {
  const lists = state.players;
  const online = onlinePlayers();
  fill("players-online", "players-none", online.map((p) => h(
    "li", {},
    avatar(p.name),
    h("span", { class: "row-main" }, h("b", {}, p.name), h("small", {}, isOp(p.name) ? "Operator" : "Player")),
    ping(p.ping_ms),
    h("span", { class: "row-actions" },
      button(isOp(p.name) ? "Deop" : "Op", () => toggleOp(p.name), { kind: "ghost" }),
      button("Kick", () => kick(p.name), { kind: "ghost" }),
      button("Ban", () => ban(p.name), { kind: "destructive" })),
  )));
  if (!lists) return;
  const banRow = (title, detail, tag, pardon) => h(
    "li", {}, h("span", { class: "file-icon" }, icon("user")),
    h("span", { class: "row-main" }, h("b", {}, title), h("small", {}, detail)),
    h("span", { class: "tag danger" }, tag),
    h("span", { class: "row-actions" }, button("Pardon", pardon, { kind: "ghost" })),
  );
  const since = (b) => [b.reason || "No reason given", b.created ? `since ${b.created.slice(0, 10)}` : null, b.expires ? `until ${b.expires.slice(0, 10)}` : null].filter(Boolean).join(" · ");
  fill("bans", "bans-none", [
    ...lists.bans.map((b) => banRow(b.name, since(b), "Player", async () => {
      if (await act("POST", "/api/v1/players/pardon", { name: b.name }, `Pardoned ${b.name}.`)) refreshLists();
    })),
  ]);
}

// --- Console --------------------------------------------------------------------------------------------

const MAX_LINES = 2000;

function kind(line) {
  if (line.startsWith("> ")) return "you";
  if (/\/(ERROR|FATAL)\]/.test(line)) return "error";
  if (/\/WARN\]/.test(line)) return "warn";
  if (/\]: (<[^>]+>|\[[^\]]+\] )/.test(line) && !/\[Supervisor\//.test(line)) return "chat";
  if (/\[Supervisor\//.test(line)) return "sup";
  return "";
}

function lineElement(line) {
  const k = kind(line);
  const m = /^(\[\d\d:\d\d:\d\d\])(.*)$/.exec(line);
  return h("span", { class: `line ${k}` }, m ? [h("span", { class: "t" }, m[1]), m[2]] : line);
}

function atBottom(el) {
  return el.scrollTop + el.clientHeight >= el.scrollHeight - 12;
}

function scrollLog(force) {
  const log = $("log");
  if (force) log.scrollTop = log.scrollHeight;
  $("jump").hidden = atBottom(log);
}

function appendLog(lines) {
  if (!lines.length) return;
  state.lines.push(...lines);
  const trimmed = state.lines.length > MAX_LINES + 100;
  if (trimmed) state.lines.splice(0, state.lines.length - MAX_LINES);
  const log = $("log");
  const stick = atBottom(log);
  if (trimmed) renderLog();
  else log.append(...lines.map(lineElement));
  if (stick) log.scrollTop = log.scrollHeight;
  else $("jump").hidden = false;
  $("log-tail").replaceChildren(...state.lines.slice(-10).map(lineElement));
  $("log-tail").scrollTop = $("log-tail").scrollHeight;
}

function renderLog() {
  $("log").replaceChildren(...state.lines.map(lineElement));
  scrollLog(true);
}

const history = [];
let historyAt = 0;

// --- Backups --------------------------------------------------------------------------------------------

function renderBackups() {
  const list = [...state.backups].sort((a, b) => (backupParts(b.name).when || 0) - (backupParts(a.name).when || 0));
  const c = state.config;
  $("backup-schedule").textContent = c ? (c.backups.every === "0" ? "Automatic backups are off." : `Automatic backup every ${c.backups.every}, the last ${c.backups.keep} kept.`) : "";
  fill("backups", "no-backups", list.map((b) => {
    const { when, label } = backupParts(b.name);
    const title = label || (when ? dateFormat.format(when) : b.name);
    const detail = [when && label ? dateFormat.format(when) : null, when ? ago(when) : null].filter(Boolean).join(" · ");
    return h(
      "li", {}, h("span", { class: "file-icon" }, icon("folder-01")),
      h("span", { class: "row-main" }, h("b", {}, title), h("small", {}, detail || b.name)),
      h("span", { class: "row-side" }, bytes(b.bytes)),
      h("span", { class: "row-actions" },
        button("Restore", () => restore(b.name), { kind: "ghost" })),
    );
  }));
}

async function restore(name) {
  const answer = await ask({
    title: "Restore this backup?",
    text: "The server restarts with it; players are disconnected for a moment. The current world is kept in backups/.",
    ok: "Restore",
    danger: true,
  });
  if (answer) await act("POST", "/api/v1/restore", { name }, `Restoring ${name}: the server is restarting.`);
}

// --- Crash reports ------- -----------------------------------------------------------------------------

let crashShown = null;

/// "crash-2026-11-03_21.14.07-server.txt" → "Nov 3, 2026, 9:14 PM".
function crashTitle(name) {
  const m = /^crash-(\d{4})-(\d{2})-(\d{2})_(\d{2})\.(\d{2})\.(\d{2})/.exec(name);
  return m ? dateFormat.format(new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6])) : name;
}

function renderCrashes() {
  $("nav-reports").textContent = state.crashes.length ? String(state.crashes.length) : "";
  fill("crashes", "no-crashes", state.crashes.map((r) => {
    const li = h(
      "li", { class: r.name === crashShown ? "selected" : "" }, h("span", { class: "file-icon" }, icon("information-circle")),
      h("span", { class: "row-main" }, h("b", {}, crashTitle(r.name)), h("small", {}, `${r.name} · ${bytes(r.bytes)}`)),
      h("span", { class: "row-actions" }, button("Read", () => readCrash(r.name), { kind: "ghost" })),
    );
    return li;
  }));
}

async function readCrash(name) {
  try {
    const response = await fetch(`/api/v1/crash-reports/read?name=${encodeURIComponent(name)}`, { credentials: "same-origin", headers: { "X-Horizon": "1" } });
    if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error || `error ${response.status}`);
    crashShown = name;
    $("crash-name").textContent = name;
    $("crash-text").textContent = await response.text();
    $("crash-viewer").hidden = false;
    renderCrashes();
  } catch (e) {
    toast(e.message, true);
  }
}

// --- Polling --------------------------------------------------------------------------------------------

async function refreshLists() {
  try {
    const [players, backups, crashes] = await Promise.all([
      api("GET", "/api/v1/players"),
      api("GET", "/api/v1/backups"),
      api("GET", "/api/v1/crash-reports"),
    ]);
    state.players = players;
    state.crashes = crashes.reports || [];
    const key = JSON.stringify(backups.backups);
    if (key !== JSON.stringify(state.backups)) {
      state.backups = backups.backups;
      renderBackups();
    }
    renderPlayers();
    renderOnline();
    renderCrashes();
    renderFacts();
  } catch (e) {
    if (e instanceof SignedOut) show("signed-out");
  }
}

async function loadConfig() {
  try {
    state.config = (await api("GET", "/api/v1/config")).config;
    renderFacts();
    renderBackups();
    route();
  } catch (e) {
    if (e instanceof SignedOut) show("signed-out");
  }
}

let ticks = 0;
let wasDown = false;
async function poll() {
  try {
    const [status, metrics, logs] = await Promise.all([
      api("GET", "/api/v1/status"),
      api("GET", `/api/v1/metrics?since=${state.metricsSince}`),
      api("GET", `/api/v1/logs?since=${state.logNext}`),
    ]);
    show("app");
    if (wasDown || !state.config) {
      wasDown = false;
      await loadConfig();
    }
    for (const s of metrics.samples) {
      state.samples.push(s);
      state.metricsSince = s.t;
    }
    state.samples.splice(0, Math.max(0, state.samples.length - 600));
    renderStatus(status);
    renderCharts();
    state.logNext = logs.next;
    appendLog(logs.lines);
    if (ticks++ % 5 === 0) await refreshLists();
    if (ticks % 30 === 0) loadConfig();
  } catch (e) {
    if (e instanceof SignedOut) show("signed-out");
    else {
      wasDown = true;
      show(state.status ? "app" : "gone");
      if (state.status) {
        $("phase").textContent = "not answering";
        $("phase").className = "state down";
      }
    }
  }
}

// --- Wiring ---------------------------------------------------------------------------------------------

function setUp() {
  window.addEventListener("hashchange", route);
  window.addEventListener("resize", renderCharts);

  $("start").addEventListener("click", () => act("POST", "/api/v1/start", undefined, "Starting…"));
  $("restart").addEventListener("click", async () => {
    if (await ask({ title: "Restart the game?", text: "Players are disconnected for a moment and can join again once it's up.", ok: "Restart" })) {
      await act("POST", "/api/v1/restart", undefined, "Restarting…");
    }
  });
  $("stop").addEventListener("click", async () => {
    if (await ask({ title: "Stop the server?", text: "The game saves and stops, and this panel goes with it. Start it again on the server with horizon-server start.", ok: "Stop", danger: true })) {
      await act("POST", "/api/v1/stop", undefined, "Stopping…");
    }
  });
  $("update-button").addEventListener("click", async () => {
    const update = state.status && state.status.update;
    if (!update || !update.available) return;
    if (await ask({ title: `Update to ${update.available}?`, text: `The world is backed up, then the game restarts on the new release. If it doesn't come up, the server goes back to ${update.installed} by itself.`, ok: "Update" })) {
      await act("POST", "/api/v1/update", {}, "Updating…");
    }
  });
  $("sign-out").addEventListener("click", async () => {
    await fetch("/logout", { method: "POST", credentials: "same-origin", headers: { "X-Horizon": "1" } }).catch(() => {});
    show("signed-out");
  });

  // Players.
  $("ban-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const target = event.target.target.value.trim();
    const reason = event.target.reason.value.trim() || undefined;
    const reply = await act("POST", "/api/v1/players/ban", { name: target, reason }, `Banned ${target}.`);
    if (reply) {
      event.target.reset();
      refreshLists();
    }
  });

  // Console.
  $("log").addEventListener("scroll", () => scrollLog(false));
  $("jump").addEventListener("click", () => scrollLog(true));
  $("command-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const line = $("command").value.trim().replace(/^\//, "");
    if (!line) return;
    $("command").value = "";
    history.push(line);
    historyAt = history.length;
    appendLog([`> ${line}`]);
    scrollLog(true);
    try {
      const reply = await api("POST", "/api/v1/command", { line });
      const output = (reply.output || []).join("\n");
      if (output) appendLog(output.split("\n").map((l) => `  ${l}`));
    } catch (e) {
      toast(e.message, true);
    }
  });
  $("command").addEventListener("keydown", (event) => {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    historyAt = Math.max(0, Math.min(history.length, historyAt + (event.key === "ArrowUp" ? -1 : 1)));
    $("command").value = history[historyAt] || "";
  });

  // Backups.
  $("backup-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = $("backup-name").value.trim();
    toast("Backing up…");
    const made = await act("POST", "/api/v1/backups", name ? { name } : {}, "Backed up.");
    if (made) {
      toast(`Backed up to backups/${made.name}`);
      $("backup-name").value = "";
      refreshLists();
    }
  });

  // Crash reports.
  $("crash-copy").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText($("crash-text").textContent);
      toast("Copied the report.");
    } catch (e) {
      toast("Couldn't copy: select the text instead.", true);
    }
  });

  route();
  poll();
  setInterval(poll, 1000);
}

document.addEventListener("DOMContentLoaded", setUp);
