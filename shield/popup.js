"use strict";

const byId = (id) => document.getElementById(id);
let current = null;

async function activeTab() {
  const tabs = await Shield.api.tabs.query({ active: true, currentWindow: true });
  return tabs && tabs[0] ? tabs[0] : null;
}

function describeTunnel(status) {
  if (!status || status.state === "off") return ["Off. Your traffic takes the normal route.", ""];
  const server = status.server ? status.server.name : "the server";
  switch (status.state) {
    case "checking":
      return ["Connecting through " + server + "…", "checking"];
    case "up": {
      const exit = status.exit || {};
      const where = [exit.city, exit.country].filter(Boolean).join(", ");
      const mismatch = status.mismatch ? " The exit country differs from the server's listing." : "";
      return [`Up through ${server}. Exit ${exit.ip || "?"}${where ? " in " + where : ""}, ${status.latencyMs} ms.${mismatch}`, status.mismatch ? "held" : "up"];
    }
    case "held":
      return [`Holding: ${status.error}. Nothing leaves the browser until ${server} answers or you turn the tunnel off.`, "held"];
    case "error":
      return ["Could not take the proxy: " + status.error, "error"];
    default:
      return [status.state, ""];
  }
}

function serverLabel(server) {
  const where = [server.city, server.country].filter(Boolean).join(", ");
  const source = server.source === "vetted" ? "vetted" : server.source === "mine" ? "yours" : "built in";
  return `${server.name}${where ? " · " + where : ""} · ${server.kind} · ${source}`;
}

function render(state) {
  current = state;
  const { settings, tunnel, stats, update, site, servers } = state;
  byId("version").textContent = update.running;
  byId("quiet").classList.toggle("on", settings.quiet);
  const notice = byId("update");
  if (update.newer) {
    notice.hidden = false;
    notice.replaceChildren(document.createTextNode(`noah shield ${update.latest} is out (you run ${update.running}). `));
    const anchor = document.createElement("a");
    anchor.href = update.page;
    anchor.target = "_blank";
    anchor.rel = "noopener";
    anchor.textContent = "Get it";
    notice.append(anchor);
  } else {
    notice.hidden = true;
  }

  const siteSection = byId("site-section");
  if (site && site.web) {
    siteSection.hidden = false;
    byId("site").textContent = site.site || site.host;
    byId("blocked").textContent = site.blocked + " stopped";
    byId("trusted").checked = site.trusted;
    byId("strict").checked = site.strictCookies;
    byId("capture").checked = site.captureAllowed;
    byId("shopquiet").checked = site.shoppingQuiet;
    Shield.send({ type: "site.score", tabId: site.tabId, url: site.url }).then((score) => {
      if (score && score.grade) byId("score").textContent = `privacy ${score.grade} (${score.score}/100): ${score.blocked} trackers stopped, ${score.thirdParties} third parties, ${score.cookies} cookies${score.https ? "" : ", no https"}`;
    });
  } else {
    siteSection.hidden = true;
  }

  byId("tunnel-on").checked = settings.tunnel.enabled;
  const select = byId("server");
  select.replaceChildren();
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.textContent = servers.length ? "choose a server" : "no servers yet: add one in settings";
  select.append(placeholder);
  for (const server of servers) {
    const option = document.createElement("option");
    option.value = server.id;
    option.textContent = serverLabel(server);
    select.append(option);
  }
  select.value = settings.tunnel.serverId || "";
  const [statusText, statusClass] = describeTunnel(tunnel);
  const statusNode = byId("tunnel-status");
  statusNode.textContent = statusText;
  statusNode.className = "muted " + statusClass;
  byId("kill").checked = settings.tunnel.killSwitch;
  byId("webrtc").checked = settings.tunnel.webRtcGuard;

  const today = stats.today || {};
  const month = stats.month || {};
  byId("today").textContent = `${today.trackers || 0} today · ${month.trackers || 0} this month`;
  byId("trackers").checked = settings.privacy.trackers;
  byId("cookies").checked = !settings.privacy.thirdPartyCookies;
  byId("geo").checked = settings.privacy.geolocation === "block";
  byId("fingerprint").checked = settings.privacy.fingerprint;
  byId("params").checked = settings.privacy.stripParameters;
  byId("gpc").checked = settings.privacy.gpc;

  byId("compare").checked = settings.shopping.compare;
  byId("coupons").checked = settings.shopping.coupons && settings.shopping.autoApply;
  byId("shop-stats").textContent = `${month.compared || 0} products compared and ${month.coupons || 0} codes that worked this month.`;

  byId("guard-screen").checked = settings.capture.guardScreen;
  byId("guard-camera").checked = settings.capture.guardCamera;
  byId("watch-count").textContent = (month.captures || 0) + " stopped";
  byId("watch-note").textContent = "Other extensions that could record the screen are listed under settings. Programs outside the browser are noah's device room's job.";
}

async function refresh() {
  const tab = await activeTab();
  const state = await Shield.send({ type: "state", tabId: tab ? tab.id : undefined, url: tab ? tab.url : "" });
  if (state && state.site && tab) {
    state.site.tabId = tab.id;
    state.site.url = tab.url;
  }
  if (state && !state.error) render(state);
  else byId("tunnel-status").textContent = "The shield's background is not answering: " + (state && state.error);
}

function bindSetting(id, path, transform = (value) => value) {
  byId(id).addEventListener("change", async (event) => {
    const value = transform(event.target.checked);
    const change = {};
    let cursor = change;
    path.slice(0, -1).forEach((key) => { cursor[key] = {}; cursor = cursor[key]; });
    cursor[path[path.length - 1]] = value;
    await Shield.send({ type: "settings.update", change });
    await refresh();
  });
}

function bindSite(id, field) {
  byId(id).addEventListener("change", async (event) => {
    if (!current || !current.site) return;
    await Shield.send({ type: "site.set", site: current.site.site, field, value: event.target.checked });
    await refresh();
  });
}

bindSite("trusted", "trusted");
bindSite("strict", "strictCookies");
bindSite("capture", "captureAllowed");
bindSite("shopquiet", "shoppingQuiet");

byId("tunnel-on").addEventListener("change", async (event) => {
  const serverId = byId("server").value || (current && current.servers[0] && current.servers[0].id) || null;
  if (event.target.checked && !serverId) {
    event.target.checked = false;
    byId("tunnel-status").textContent = "Add a server first (settings and servers).";
    return;
  }
  await Shield.send({ type: "settings.update", change: { tunnel: { enabled: event.target.checked, serverId } } });
  await refresh();
  setTimeout(refresh, 2500);
});
byId("server").addEventListener("change", async (event) => {
  await Shield.send({ type: "settings.update", change: { tunnel: { serverId: event.target.value || null } } });
  await refresh();
  setTimeout(refresh, 2500);
});
bindSetting("kill", ["tunnel", "killSwitch"]);
bindSetting("webrtc", ["tunnel", "webRtcGuard"]);
byId("recheck").addEventListener("click", async () => {
  byId("tunnel-status").textContent = "Checking…";
  await Shield.send({ type: "tunnel.check" });
  await refresh();
});

bindSetting("trackers", ["privacy", "trackers"]);
bindSetting("cookies", ["privacy", "thirdPartyCookies"], (checked) => !checked);
bindSetting("geo", ["privacy", "geolocation"], (checked) => (checked ? "block" : "ask"));
bindSetting("fingerprint", ["privacy", "fingerprint"]);
bindSetting("params", ["privacy", "stripParameters"]);
bindSetting("gpc", ["privacy", "gpc"]);
bindSetting("compare", ["shopping", "compare"]);
byId("coupons").addEventListener("change", async (event) => {
  await Shield.send({ type: "settings.update", change: { shopping: { coupons: event.target.checked, autoApply: event.target.checked } } });
  await refresh();
});
bindSetting("guard-screen", ["capture", "guardScreen"]);
bindSetting("guard-camera", ["capture", "guardCamera"]);

byId("burn").addEventListener("click", async () => {
  if (!current || !current.site) return;
  const result = await Shield.send({ type: "site.burn", site: current.site.site });
  byId("score").textContent = `burned: ${result.removed} cookies and this site's storage are gone. Reload the page.`;
});
byId("watchers").addEventListener("click", async () => {
  if (!current || !current.site) return;
  const list = byId("watchers-list");
  const { watchers } = await Shield.send({ type: "site.watchers", tabId: current.site.tabId, site: current.site.site });
  list.replaceChildren();
  if (!watchers || !watchers.length) list.append(document.createTextNode("No third parties on this page yet."));
  for (const watcher of watchers.slice(0, 40)) {
    const row = document.createElement("div");
    row.className = "watcher";
    row.textContent = `${watcher.owner} · ${watcher.kind}${watcher.country ? " · " + watcher.country : ""} · ${watcher.host} · ${watcher.blocked ? watcher.blocked + " stopped" : watcher.count + " allowed"}`;
    list.append(row);
  }
  list.hidden = !list.hidden;
});
byId("alias").addEventListener("click", async () => {
  if (!current || !current.site) return;
  const result = await Shield.send({ type: "alias.make", site: current.site.site });
  const out = byId("alias-out");
  out.hidden = false;
  if (result.error) out.textContent = result.error;
  else {
    out.textContent = result.alias + " (" + result.source + ", copied)";
    try { await navigator.clipboard.writeText(result.alias); } catch { /* the text is shown anyway */ }
  }
});
byId("decoy").addEventListener("click", async () => {
  if (!current || !current.site) return;
  const result = await Shield.send({ type: "decoy.fill", tabId: current.site.tabId, site: current.site.site });
  const out = byId("alias-out");
  out.hidden = false;
  out.textContent = result.error ? result.error : `filled with ${result.identity.fullName}, ${result.identity.email}`;
});
byId("quiet").addEventListener("click", async () => {
  await Shield.send({ type: "settings.update", change: { quiet: !(current && current.settings.quiet) } });
  await refresh();
});
byId("options").addEventListener("click", (event) => {
  event.preventDefault();
  Shield.api.runtime.openOptionsPage();
});

refresh();
