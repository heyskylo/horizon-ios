// The Horizon server panel (docs/server.md, section 6). It polls the control API once a second through
// the panel's session; every change is a POST with the X-Horizon header. Text from the server is only
// ever set with textContent.
"use strict";

const $ = (id) => document.getElementById(id);
let logNext = 0;
let metricsSince = 0;
const samples = [];
let lastStatus = null;
let backupsShown = "";

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

function show(view) {
  for (const id of ["app", "signed-out", "gone"]) $(id).hidden = id !== view;
  for (const id of ["restart", "stop"]) $(id).disabled = view !== "app";
}

let toastTimer;
function toast(text) {
  $("toast").textContent = text;
  $("toast").hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => ($("toast").hidden = true), 5000);
}

function duration(seconds) {
  if (seconds == null) return "";
  const d = Math.floor(seconds / 86400), h = Math.floor(seconds / 3600) % 24, m = Math.floor(seconds / 60) % 60;
  if (d) return `${d} d ${h} h`;
  if (h) return `${h} h ${m} min`;
  return m ? `${m} min` : `${seconds} s`;
}

function bytes(n) {
  if (n == null) return "–";
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  return `${Math.ceil(n / 1e3)} kB`;
}

function cell(text) {
  const td = document.createElement("td");
  td.textContent = text;
  return td;
}

function button(label, action, danger) {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = label;
  if (danger) b.className = "danger";
  b.addEventListener("click", action);
  return b;
}

async function run(line) {
  const reply = await api("POST", "/api/v1/command", { line });
  return (reply.output || []).join("\n");
}

function renderStatus(status) {
  lastStatus = status;
  const phase = $("phase");
  phase.textContent = status.phase;
  phase.className = `pill ${status.phase}`;
  const game = status.sample && status.sample.game;
  const parts = [`up ${duration(status.engine_uptime_seconds)}`, `horizon-server ${status.version}`];
  if (status.crashes) parts.push(`${status.crashes} crash${status.crashes > 1 ? "es" : ""}`);
  $("summary").textContent = parts.join(" · ");
  renderUpdate(status.update);

  const players = (game && game.players) || [];
  $("player-count").textContent = game ? `${players.length} of ${game.max_players}` : "";
  $("no-players").hidden = players.length > 0;
  const body = $("players");
  body.replaceChildren(
    ...players.map((p) => {
      const tr = document.createElement("tr");
      const actions = cell("");
      actions.append(
        button("Op", async () => toast(await run(`op ${p.name}`).catch((e) => e.message))),
        button("Kick", async () => {
          if (confirm(`Kick ${p.name}?`)) toast(await run(`kick ${p.name}`).catch((e) => e.message));
        }, true),
        button("Ban", async () => {
          const reason = prompt(`Ban ${p.name}? Reason (optional):`);
          if (reason === null) return;
          await api("POST", "/api/v1/players/ban", { name: p.name, reason: reason.trim() || undefined }).then(
            (r) => { toast(r.output.join("\n") || "Banned."); refreshLists(); },
            (e) => toast(e.message),
          );
        }, true),
      );
      tr.append(cell(p.name), cell(p.ping_ms == null ? "–" : `${p.ping_ms} ms`), actions);
      return tr;
    }),
  );
}

// The banned players (GET /api/v1/players, "bans") with a Pardon each.
function renderBans(bans) {
  $("no-bans").hidden = bans.length > 0;
  $("bans").replaceChildren(
    ...bans.map((b) => {
      const tr = document.createElement("tr");
      const actions = cell("");
      actions.append(
        button("Pardon", async () => {
          await api("POST", "/api/v1/players/pardon", { name: b.name }).then(
            (r) => { toast(r.output.join("\n") || "Pardoned."); refreshLists(); },
            (e) => toast(e.message),
          );
        }),
      );
      tr.append(cell(b.name), cell(b.reason || "–"), actions);
      return tr;
    }),
  );
}

// The crash reports (GET /api/v1/crash-reports) and one report's text.
function renderCrashes(reports) {
  $("no-crashes").hidden = reports.length > 0;
  $("crashes").replaceChildren(
    ...reports.map((r) => {
      const tr = document.createElement("tr");
      const actions = cell("");
      actions.append(
        button("Read", async () => {
          try {
            const response = await fetch(`/api/v1/crash-reports/read?name=${encodeURIComponent(r.name)}`, {
              credentials: "same-origin",
              headers: { "X-Horizon": "1" },
            });
            if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error || `error ${response.status}`);
            $("crash-text").textContent = await response.text();
            $("crash-text").hidden = false;
            $("crash-text").scrollIntoView({ block: "nearest" });
          } catch (e) {
            toast(e.message);
          }
        }),
      );
      tr.append(cell(r.name), cell(bytes(r.bytes)), actions);
      return tr;
    }),
  );
}

async function refreshLists() {
  const [players, crashes] = await Promise.all([
    api("GET", "/api/v1/players"),
    api("GET", "/api/v1/crash-reports"),
  ]);
  renderBans(players.bans || []);
  renderCrashes(crashes.reports || []);
}

// Updates (horizon-server update): a newer release, one under way, or how the last one ended.
function renderUpdate(update) {
  const box = $("update");
  const offer = update && !update.busy && update.available;
  let text = "";
  if (update && update.busy) text = `Updating: ${update.busy}…`;
  else if (offer) text = `Horizon ${update.available} is out (this server runs ${update.installed}).`;
  else if (update && update.last && !update.ok) text = update.last;
  box.hidden = !text;
  box.className = update && update.last && !update.ok && !update.busy && !offer ? "notice failed" : "notice";
  $("update-text").textContent = text;
  $("update-button").hidden = !offer;
}

function renderBackups(list) {
  const key = JSON.stringify(list);
  if (key === backupsShown) return;
  backupsShown = key;
  $("no-backups").hidden = list.length > 0;
  $("backups").replaceChildren(
    ...list.map((b) => {
      const tr = document.createElement("tr");
      const actions = cell("");
      actions.append(
        button("Restore", async () => {
          if (!confirm(`Restore ${b.name}? The server restarts with that world; the current one is kept in backups/.`)) return;
          try {
            await api("POST", "/api/v1/restore", { name: b.name });
            toast(`Restoring ${b.name}: the server is restarting.`);
          } catch (e) {
            toast(e.message);
          }
        }, true),
      );
      tr.append(cell(b.name), cell(bytes(b.bytes)), actions);
      return tr;
    }),
  );
}

function appendLog(lines) {
  if (!lines.length) return;
  const log = $("log");
  const atBottom = log.scrollTop + log.clientHeight >= log.scrollHeight - 8;
  log.append(document.createTextNode(lines.join("\n") + "\n"));
  // Keep the page light: the last 2000 lines, as the server does.
  const text = log.textContent;
  const all = text.split("\n");
  if (all.length > 2100) log.textContent = all.slice(-2000).join("\n");
  if (atBottom) log.scrollTop = log.scrollHeight;
}

function chart(id, values, { max, unit, line, decimals = 1 }) {
  const canvas = $(id);
  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth, height = canvas.clientHeight;
  if (canvas.width !== width * ratio) {
    canvas.width = width * ratio;
    canvas.height = height * ratio;
  }
  const g = canvas.getContext("2d");
  g.setTransform(ratio, 0, 0, ratio, 0, 0);
  g.clearRect(0, 0, width, height);
  const style = getComputedStyle(document.documentElement);
  const known = values.filter((v) => v != null);
  const top = Math.max(max || 0, ...known, 1e-9) * 1.1;
  if (line != null) {
    g.strokeStyle = style.getPropertyValue("--rule");
    g.setLineDash([4, 4]);
    g.beginPath();
    const y = height - (line / top) * height;
    g.moveTo(0, y);
    g.lineTo(width, y);
    g.stroke();
    g.setLineDash([]);
  }
  g.strokeStyle = style.getPropertyValue("--accent");
  g.lineWidth = 1.5;
  g.beginPath();
  const span = 600;
  let drawing = false;
  values.forEach((v, i) => {
    if (v == null) {
      drawing = false;
      return;
    }
    const x = width - ((values.length - 1 - i) / (span - 1)) * width;
    const y = height - (v / top) * height;
    if (drawing) g.lineTo(x, y);
    else g.moveTo(x, y);
    drawing = true;
  });
  g.stroke();
  const last = values[values.length - 1];
  $(`${id}-now`).textContent = last == null ? "–" : `${last.toFixed(decimals)}${unit}`;
}

function renderCharts() {
  chart("tps", samples.map((s) => s.tps), { max: 20, unit: "", line: 20 });
  chart("mspt", samples.map((s) => s.mspt), { max: 50, unit: " ms", line: 50 });
  chart("ram", samples.map((s) => (s.rss_bytes == null ? null : s.rss_bytes / 1e9)), { unit: " GB", decimals: 2 });
  chart("cpu", samples.map((s) => s.cpu_percent), { max: 100, unit: " %", decimals: 0 });
  $("ram-now").textContent = bytes(samples.length ? samples[samples.length - 1].rss_bytes : null);
}

let ticks = 0;
async function poll() {
  try {
    const [status, metrics, logs] = await Promise.all([
      api("GET", "/api/v1/status"),
      api("GET", `/api/v1/metrics?since=${metricsSince}`),
      api("GET", `/api/v1/logs?since=${logNext}`),
    ]);
    show("app");
    renderStatus(status);
    for (const s of metrics.samples) {
      samples.push(s);
      metricsSince = s.t;
    }
    samples.splice(0, Math.max(0, samples.length - 600));
    renderCharts();
    logNext = logs.next;
    appendLog(logs.lines);
    if (ticks++ % 10 === 0) {
      renderBackups((await api("GET", "/api/v1/backups")).backups);
      await refreshLists();
    }
  } catch (e) {
    if (e instanceof SignedOut) show("signed-out");
    else if (lastStatus) {
      show("gone");
      $("phase").textContent = "down";
      $("phase").className = "pill down";
    } else show("gone");
  }
}

function setUp() {
  $("command-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const line = $("command").value.trim();
    if (!line) return;
    $("command").value = "";
    try {
      const output = await run(line);
      appendLog([`> ${line}`]);
      if (!output) toast("Done.");
    } catch (e) {
      toast(e.message);
    }
  });
  $("backup-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = $("backup-name").value.trim();
    toast("Backing up…");
    try {
      const made = await api("POST", "/api/v1/backups", name ? { name } : {});
      toast(`Backed up to backups/${made.name}`);
      $("backup-name").value = "";
      renderBackups((await api("GET", "/api/v1/backups")).backups);
    } catch (e) {
      toast(e.message);
    }
  });
  $("update-button").addEventListener("click", async () => {
    const update = lastStatus && lastStatus.update;
    if (!update || !update.available) return;
    if (!confirm(`Update to ${update.available}? The world is backed up, then the game restarts on it (players are disconnected for a moment). If it doesn't come up, the server goes back to ${update.installed} by itself.`)) return;
    await api("POST", "/api/v1/update", {}).then(() => toast("Updating…"), (e) => toast(e.message));
  });
  $("restart").addEventListener("click", async () => {
    if (!confirm("Restart the game? Players are disconnected for a moment.")) return;
    await api("POST", "/api/v1/restart").then(() => toast("Restarting…"), (e) => toast(e.message));
  });
  $("stop").addEventListener("click", async () => {
    if (!confirm("Stop the server? The panel stops with it; start it again on the server.")) return;
    await api("POST", "/api/v1/stop").then(() => toast("Stopping…"), (e) => toast(e.message));
  });
  poll();
  setInterval(poll, 1000);
}

document.addEventListener("DOMContentLoaded", setUp);
