"use strict";

const byId = (id) => document.getElementById(id);
let state = null;

function pathGet(object, path) {
  return path.split(".").reduce((cursor, key) => (cursor ? cursor[key] : undefined), object);
}
function changeFor(path, value) {
  const change = {};
  let cursor = change;
  const keys = path.split(".");
  keys.slice(0, -1).forEach((key) => { cursor[key] = {}; cursor = cursor[key]; });
  cursor[keys[keys.length - 1]] = value;
  return change;
}
function cell(content, className = "") {
  const td = document.createElement("td");
  if (content instanceof Node) td.append(content);
  else td.textContent = content;
  if (className) td.className = className;
  return td;
}
function button(label, onClick) {
  const element = document.createElement("button");
  element.className = "ghost";
  element.textContent = label;
  element.addEventListener("click", onClick);
  return element;
}

function renderServers() {
  const table = byId("servers");
  table.replaceChildren();
  const head = document.createElement("tr");
  for (const title of ["server", "where", "kind", "from", ""]) {
    const th = document.createElement("th");
    th.textContent = title;
    head.append(th);
  }
  table.append(head);
  if (!state.servers.length) {
    const row = document.createElement("tr");
    row.append(cell("No servers yet.", "muted"));
    table.append(row);
  }
  for (const server of state.servers) {
    const row = document.createElement("tr");
    const name = document.createElement("div");
    name.textContent = server.name;
    if (server.notes) {
      const notes = document.createElement("div");
      notes.className = "muted";
      notes.textContent = server.notes;
      name.append(notes);
    }
    row.append(cell(name), cell([server.city, server.country].filter(Boolean).join(", ") || "—"), cell(server.kind + " " + server.host + ":" + server.port));
    row.append(cell(server.source === "vetted" ? "noah's feed" : server.source === "mine" ? "you" : "built in"));
    const actions = cell("", "actions");
    actions.append(button(state.settings.tunnel.serverId === server.id && state.settings.tunnel.enabled ? "in use" : "use", async () => {
      await Shield.send({ type: "settings.update", change: { tunnel: { enabled: true, serverId: server.id } } });
      await load();
    }));
    if (server.source === "mine") {
      actions.append(document.createTextNode(" "));
      actions.append(button("remove", async () => {
        await Shield.send({ type: "tunnel.removeServer", id: server.id });
        await load();
      }));
    }
    row.append(actions);
    table.append(row);
  }
}

function renderFeeds() {
  const describe = (name) => {
    const feed = state.feeds[name];
    if (!feed || (!feed.fetched && !feed.error)) return "not fetched yet";
    const parts = [];
    if (feed.fetched) parts.push(`${feed.count ?? 0} entries, fetched ${new Date(feed.fetched).toLocaleString()}`);
    if (feed.error) parts.push("last attempt failed: " + feed.error);
    return parts.join("; ");
  };
  byId("feed-servers").textContent = describe("servers.json");
  byId("feed-coupons").textContent = describe("coupons.json");
}

function renderSwitches() {
  for (const input of document.querySelectorAll("input[data-path]")) {
    const value = pathGet(state.settings, input.dataset.path);
    if ("block" in input.dataset) input.checked = value === "block";
    else if ("invert" in input.dataset) input.checked = !value;
    else input.checked = Boolean(value);
  }
  for (const area of document.querySelectorAll("textarea[data-list]")) {
    area.value = (pathGet(state.settings, area.dataset.list) || []).join("\n");
  }
  byId("codes").value = state.settings.shopping.codes.map((entry) => [entry.site || "*", entry.code, entry.note || ""].join(" ").trim()).join("\n");
  for (const input of document.querySelectorAll("input[data-number]")) {
  input.addEventListener("change", async () => {
    await Shield.send({ type: "settings.update", change: changeFor(input.dataset.number, Number(input.value) || 1) });
    await load();
  });
}
for (const area of document.querySelectorAll("textarea[data-mode-list]")) {
  area.addEventListener("change", async () => {
    const mode = area.dataset.modeList;
    const siteModes = { ...state.settings.modes.siteModes };
    for (const [site, existing] of Object.entries(siteModes)) if (existing === mode) delete siteModes[site];
    for (const line of area.value.split(/\n+/)) {
      const site = Shield.siteOf(Shield.hostOf("https://" + line.trim().replace(/^https?:\/\//, "")));
      if (site) siteModes[site] = mode;
    }
    await Shield.send({ type: "settings.update", change: { modes: { siteModes } } });
    await load();
  });
}
for (const element of document.querySelectorAll("button[data-profile]")) {
  element.addEventListener("click", async () => {
    await Shield.send({ type: "profile.apply", name: element.dataset.profile });
    await load();
  });
}
byId("lock-pin-set").addEventListener("click", async () => {
  const result = await Shield.send({ type: "pin.set", target: "lock", pin: byId("lock-pin").value });
  byId("lock-pin-status").textContent = result.error || "PIN set";
  byId("lock-pin").value = "";
});
byId("parental-setpin").addEventListener("click", async () => {
  const current = state.settings.modes.parental.pinHash ? prompt("Current parental PIN") : "";
  const result = await Shield.send({ type: "pin.set", target: "parental", pin: byId("parental-pin").value, current });
  byId("parental-status").textContent = result.error || "PIN set";
});
byId("parental-on").addEventListener("click", async () => {
  if (!state.settings.modes.parental.pinHash) { byId("parental-status").textContent = "set a PIN first"; return; }
  const result = await Shield.send({ type: "parental.set", enabled: true, pin: byId("parental-pin").value });
  byId("parental-status").textContent = result.error || "on";
  await load();
});
byId("parental-off").addEventListener("click", async () => {
  const result = await Shield.send({ type: "parental.set", enabled: false, pin: byId("parental-pin").value });
  byId("parental-status").textContent = result.error || "off";
  await load();
});
for (const select of document.querySelectorAll("select[data-select]")) {
  select.addEventListener("change", async () => {
    await Shield.send({ type: "settings.update", change: changeFor(select.dataset.select, select.value) });
    await load();
  });
}
for (const area of document.querySelectorAll("textarea[data-list-raw]")) {
  area.addEventListener("change", async () => {
    const values = area.value.split(/\n+/).map((line) => line.trim()).filter(Boolean).slice(0, 20);
    await Shield.send({ type: "settings.update", change: changeFor(area.dataset.listRaw, values) });
    await load();
  });
}
byId("rotate-now").addEventListener("click", async () => {
  await Shield.send({ type: "fingerprint.rotate" });
  byId("rotate-now").textContent = "rotated";
});
byId("sync-set").addEventListener("click", async () => {
  const result = await Shield.send({ type: "sync.set", passphrase: byId("sync-passphrase").value });
  byId("sync-result").textContent = result.error || "sync is on";
  await load();
});
byId("sync-pull").addEventListener("click", async () => {
  const result = await Shield.send({ type: "sync.pull" });
  byId("sync-result").textContent = result.error || "pulled settings from " + new Date(result.at).toLocaleString();
  await load();
});
byId("sync-off").addEventListener("click", async () => {
  await Shield.send({ type: "sync.set", passphrase: "" });
  byId("sync-result").textContent = "sync is off";
  await load();
});
for (const input of document.querySelectorAll("input[data-text]")) {
    input.value = pathGet(state.settings, input.dataset.text) || "";
  }
  const citySelect = byId("city-select");
  if (citySelect && !citySelect.children.length) {
    for (const city of Shield.CITIES) {
      const option = document.createElement("option");
      option.value = city.id;
      option.textContent = `${city.name}, ${city.country}`;
      citySelect.append(option);
    }
  }
  for (const select of document.querySelectorAll("select[data-select]")) {
    select.value = pathGet(state.settings, select.dataset.select) || select.options[0].value;
  }
  for (const area of document.querySelectorAll("textarea[data-list-raw]")) {
    area.value = (pathGet(state.settings, area.dataset.listRaw) || []).join("\n");
  }
  for (const input of document.querySelectorAll("input[data-number]")) {
    input.value = pathGet(state.settings, input.dataset.number) ?? "";
  }
  for (const area of document.querySelectorAll("textarea[data-mode-list]")) {
    area.value = Object.entries(state.settings.modes.siteModes).filter(([, mode]) => mode === area.dataset.modeList).map(([site]) => site).join("\n");
  }
  const profileNow = byId("profile-now");
  if (profileNow) profileNow.textContent = state.settings.modes.profile ? "profile: " + state.settings.modes.profile : "no profile applied";
  const days = byId("focus-days");
  if (days && !days.children.length) {
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].forEach((name, index) => {
      const label = document.createElement("label");
      label.className = "switch";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.dataset.day = String(index);
      box.addEventListener("change", async () => {
        const chosen = Array.from(days.querySelectorAll("input:checked")).map((element) => Number(element.dataset.day));
        await Shield.send({ type: "settings.update", change: { modes: { focus: { days: chosen } } } });
      });
      label.append(box, document.createTextNode(name));
      days.append(label);
    });
  }
  if (days) for (const box of days.querySelectorAll("input")) box.checked = state.settings.modes.focus.days.includes(Number(box.dataset.day));
  const parentalStatus = byId("parental-status");
  if (parentalStatus) parentalStatus.textContent = state.settings.modes.parental.enabled ? "on" : "off";
  const lockStatus = byId("lock-pin-status");
  if (lockStatus) lockStatus.textContent = state.settings.modes.lock.pinHash ? "PIN set" : "no PIN yet";
}

async function renderPermissions() {
  const table = byId("permissions");
  if (!table) return;
  const { log } = await Shield.send({ type: "permissions.log" });
  table.replaceChildren();
  if (!log || !log.length) {
    const row = document.createElement("tr");
    row.append(cell("Nothing granted yet.", "muted"));
    table.append(row);
    return;
  }
  for (const entry of log.slice(0, 60)) {
    const row = document.createElement("tr");
    row.append(cell(new Date(entry.at).toLocaleString()), cell(entry.site), cell(entry.kind), cell(entry.decision));
    table.append(row);
  }
}

async function renderExtensions() {
  const table = byId("extensions");
  table.replaceChildren();
  const audit = await Shield.send({ type: "watch.audit" });
  if (!audit || !audit.supported) {
    const row = document.createElement("tr");
    row.append(cell("This browser does not let extensions list each other" + (audit && audit.error ? ": " + audit.error : ".") + " Check the browser's own extensions page for anything you did not install.", "muted"));
    table.append(row);
    return;
  }
  const head = document.createElement("tr");
  for (const title of ["extension", "what it can do", "installed", ""]) {
    const th = document.createElement("th");
    th.textContent = title;
    head.append(th);
  }
  table.append(head);
  if (!audit.extensions.length) {
    const row = document.createElement("tr");
    row.append(cell("No other extensions installed.", "muted"));
    table.append(row);
  }
  for (const extension of audit.extensions) {
    const row = document.createElement("tr");
    const name = document.createElement("div");
    name.className = "level-" + extension.level;
    name.textContent = extension.name + " " + extension.version + (extension.enabled ? "" : " (disabled)");
    row.append(cell(name), cell(extension.reasons.join("; ") || "nothing that reaches your screen or data"), cell(extension.installType));
    const actions = cell("", "actions");
    if (extension.mayDisable) {
      actions.append(button(extension.enabled ? "disable" : "enable", async () => {
        const result = await Shield.send({ type: "watch.setEnabled", id: extension.id, enabled: !extension.enabled });
        if (result && result.error) alert(result.error);
        await renderExtensions();
      }));
    }
    row.append(actions);
    table.append(row);
  }
}

async function load() {
  state = await Shield.send({ type: "state" });
  if (!state || state.error) {
    byId("about").textContent = "The shield's background is not answering: " + (state && state.error);
    return;
  }
  byId("version").textContent = state.update.running + (state.update.newer ? " · " + state.update.latest + " is out" : "");
  byId("about").textContent = `noah shield ${state.update.running}, part of noah by House of Asher. ${state.update.newer ? "Version " + state.update.latest + " is available at " + state.update.page + "." : "This is the newest version the feed knows."}`;
  renderServers();
  renderFeeds();
  renderSwitches();
}

for (const input of document.querySelectorAll("input[data-path]")) {
  input.addEventListener("change", async () => {
    let value = input.checked;
    if ("block" in input.dataset) value = input.checked ? "block" : "ask";
    else if ("invert" in input.dataset) value = !input.checked;
    await Shield.send({ type: "settings.update", change: changeFor(input.dataset.path, value) });
    await load();
  });
}
for (const area of document.querySelectorAll("textarea[data-list]")) {
  area.addEventListener("change", async () => {
    const sites = area.value.split(/\n+/).map((line) => Shield.siteOf(Shield.hostOf("https://" + line.trim().replace(/^https?:\/\//, "")))).filter(Boolean);
    await Shield.send({ type: "settings.update", change: changeFor(area.dataset.list, Array.from(new Set(sites))) });
    await load();
  });
}
for (const input of document.querySelectorAll("input[data-text]")) {
  input.addEventListener("change", async () => {
    await Shield.send({ type: "settings.update", change: changeFor(input.dataset.text, input.value.trim().slice(0, 200)) });
    await load();
  });
}
byId("check-email").addEventListener("click", async () => {
  const email = byId("email-to-check").value.trim();
  if (!email) return;
  byId("email-result").textContent = "checking…";
  const result = await Shield.send({ type: "breach.email", email });
  if (result.error) byId("email-result").textContent = result.error;
  else if (!result.breaches.length) byId("email-result").textContent = "not in any known breach";
  else byId("email-result").textContent = "found in: " + result.breaches.join(", ");
});
byId("forget-passwords").addEventListener("click", async () => {
  await Shield.send({ type: "passwords.forget" });
  byId("forget-passwords").textContent = "forgotten";
});
byId("codes").addEventListener("change", async () => {
  const codes = byId("codes").value.split(/\n+/).map((line) => line.trim()).filter(Boolean).map((line) => {
    const [first, second, ...rest] = line.split(/\s+/);
    if (!second) return { site: "", code: first.toUpperCase(), note: "" };
    return { site: first === "*" ? "" : Shield.siteOf(first.toLowerCase()), code: second.toUpperCase(), note: rest.join(" ") };
  }).filter((entry) => /^[A-Z0-9][A-Z0-9-]{2,24}$/.test(entry.code)).slice(0, 200);
  await Shield.send({ type: "settings.update", change: { shopping: { codes } } });
  await load();
});
byId("add-server").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = new FormData(event.target);
  const server = Object.fromEntries(form.entries());
  const result = await Shield.send({ type: "tunnel.addServer", server });
  byId("add-server-error").textContent = result && result.error ? result.error : "";
  if (result && !result.error) event.target.reset();
  await load();
});
byId("refresh-feeds").addEventListener("click", async () => {
  byId("feed-servers").textContent = "fetching…";
  await Shield.send({ type: "feeds.refresh" });
  await load();
});
byId("purge").addEventListener("click", async () => {
  const result = await Shield.send({ type: "privacy.purgeCookies" });
  byId("purge-result").textContent = result && result.removed !== undefined ? `removed ${result.removed}` : "could not";
});

load().then(renderExtensions).then(renderPermissions);
