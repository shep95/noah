"use strict";

const byId = (id) => document.getElementById(id);
let current = null;

async function activeTab() {
  const tabs = await Shield.api.tabs.query({ active: true, currentWindow: true });
  return tabs && tabs[0] ? tabs[0] : null;
}

const regionNames = (() => { try { return new Intl.DisplayNames(["en"], { type: "region" }); } catch { return null; } })();
function countryName(code) {
  if (!code) return "";
  try { return (regionNames && regionNames.of(code.toUpperCase())) || code; } catch { return code; }
}
function flag(code) {
  if (!/^[A-Za-z]{2}$/.test(code || "")) return "";
  return String.fromCodePoint(...code.toUpperCase().split("").map((letter) => 0x1f1e6 + letter.charCodeAt(0) - 65)) + " ";
}
function placeOf(server) {
  if (server.id === "tor-local") return "anonymous route (Tor on this computer)";
  const country = countryName(server.country);
  const where = [country, server.city].filter(Boolean).join(" · ");
  return where ? flag(server.country) + where : server.name;
}

// Plain words: people want to know whether they are covered, where they come
// out, and what to do if not.
function describeTunnel(status) {
  if (!status || status.state === "off") return ["Off. Sites see your own address.", "", "off"];
  const place = status.server ? placeOf(status.server) : "the location";
  switch (status.state) {
    case "checking":
      return [`Connecting to ${place}…`, "checking", "connecting"];
    case "up": {
      const exit = status.exit || {};
      const where = [exit.city, countryName(exit.country)].filter(Boolean).join(", ");
      const mismatch = status.mismatch ? " That is not the country this location promised." : "";
      return [`Protected. Sites see ${exit.ip || "the location's address"}${where ? " in " + where : ""}; ${status.latencyMs} ms away.${mismatch}`, status.mismatch ? "held" : "up", "on · " + (where || place)];
    }
    case "held":
      return [`${place} is not answering (${status.error}). Nothing leaves the browser until it does or you disconnect, so you are not exposed.`, "held", "holding"];
    case "error":
      return ["Could not take over the connection: " + status.error, "error", "problem"];
    default:
      return [status.state, "", status.state];
  }
}

function serverLabel(server) {
  const source = server.source === "vetted" ? "vetted" : server.source === "mine" ? "yours" : "";
  const place = placeOf(server);
  const name = server.id === "tor-local" || place.includes(server.name) ? "" : " · " + server.name;
  return `${place}${name}${source ? " · " + source : ""}`;
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
    byId("route").classList.toggle("on", Boolean(settings.tunnel.siteRoutes[site.site]));
    byId("trusted").checked = site.trusted;
    byId("strict").checked = site.strictCookies;
    byId("capture").checked = site.captureAllowed;
    byId("shopquiet").checked = site.shoppingQuiet;
    Shield.send({ type: "site.score", tabId: site.tabId, url: site.url }).then((score) => {
      if (!score || !score.grade) return;
      const grade = byId("grade");
      grade.textContent = score.grade;
      grade.className = "grade in " + score.grade.toLowerCase();
      byId("score").textContent = `${score.score}/100 · ${score.thirdParties} third parties, ${score.advertising} of them advertising, ${score.cookies} cookies${score.https ? "" : ", no https"}`;
    });
    Shield.send({ type: "site.trust", url: site.url }).then((trust) => {
      const line = byId("trust");
      if (!trust || trust.error) { line.hidden = true; return; }
      const parts = [];
      let tone = "";
      if (trust.lookalike) { parts.push(`looks like ${trust.lookalike.brand} but is not`); tone = "bad"; }
      if (!trust.https) { parts.push("no https: what you send can be read on the way"); tone = tone || "warn"; }
      if (Array.isArray(trust.breaches)) {
        if (trust.breaches.length) {
          const last = trust.breaches[0];
          const year = (last.date || "").slice(0, 4);
          const what = last.classes.slice(0, 3).map((name) => name.toLowerCase()).join(", ");
          parts.push(`leaked its users' data ${trust.breaches.length === 1 ? "once" : trust.breaches.length + " times"}${year ? ", last in " + year : ""}${what ? " (" + what + ")" : ""}; use a password you use nowhere else here`);
          if (!tone || tone === "warn") tone = Number(year) >= new Date().getFullYear() - 2 ? "bad" : "warn";
        } else {
          parts.push("no known data leak");
        }
      } else if (trust.breachError) {
        parts.push("leak history " + trust.breachError);
      }
      line.hidden = false;
      line.textContent = (tone === "bad" ? "be careful: " : tone === "warn" ? "worth knowing: " : "trust: ") + parts.join(" · ");
      line.className = "muted " + tone;
    });
    Shield.send({ type: "media.list" }).then((media) => {
      const live = (media && media.tabs || []).filter((entry) => entry.camera || entry.microphone);
      const node = byId("media-live");
      node.hidden = !live.length;
      node.textContent = live.map((entry) => `${entry.site} is using ${[entry.camera && "the camera", entry.microphone && "the microphone"].filter(Boolean).join(" and ")} (${entry.title.slice(0, 30)})`).join("; ");
    });
  } else {
    siteSection.hidden = true;
  }

  const connectButton = byId("tunnel-on");
  connectButton.textContent = settings.tunnel.enabled ? "disconnect" : "connect";
  connectButton.classList.toggle("on", settings.tunnel.enabled);
  const select = byId("server");
  select.replaceChildren();
  const auto = document.createElement("option");
  auto.value = "auto";
  auto.textContent = servers.length ? "fastest location" : "no locations yet";
  select.append(auto);
  // Real places first, sorted by country; the anonymous Tor route last.
  const sorted = [...servers].sort((left, right) => (left.id === "tor-local") - (right.id === "tor-local") || countryName(left.country).localeCompare(countryName(right.country)));
  for (const server of sorted) {
    const option = document.createElement("option");
    option.value = server.id;
    option.textContent = serverLabel(server);
    select.append(option);
  }
  select.value = settings.tunnel.serverId && servers.some((server) => server.id === settings.tunnel.serverId) ? settings.tunnel.serverId : "auto";
  byId("vpn-empty").hidden = servers.length > 0;
  connectButton.disabled = !servers.length;
  const [statusText, statusClass, shortState] = describeTunnel(tunnel);
  const statusNode = byId("tunnel-status");
  statusNode.textContent = statusText;
  statusNode.className = "muted " + statusClass;
  byId("vpn-dot").className = "dot " + statusClass;
  byId("vpn-state").textContent = shortState;
  byId("kill").checked = settings.tunnel.killSwitch;
  byId("webrtc").checked = settings.tunnel.webRtcGuard;

  const today = stats.today || {};
  const month = stats.month || {};
  byId("today").textContent = `${month.trackers || 0} trackers this month`;
  byId("d-trackers").textContent = today.trackers || 0;
  byId("d-cookies").textContent = (today.cookies || 0) + (today.burned || 0);
  byId("d-scams").textContent = (today.phishing || 0) + (today.popups || 0) + (today.leaks || 0) + (today.keylog || 0) + (today.wallets || 0) + (today.downloads || 0);
  byId("d-coupons").textContent = today.coupons || 0;
  byId("d-saved").textContent = today.saved || 0;
  byId("d-annoy").textContent = (today.banners || 0) + (today.overlays || 0);
  byId("trackers").checked = settings.privacy.trackers;
  byId("cookies").checked = !settings.privacy.thirdPartyCookies;
  byId("geo").checked = settings.privacy.geolocation === "block";
  byId("fingerprint").checked = settings.privacy.fingerprint;
  byId("params").checked = settings.privacy.stripParameters;
  byId("gpc").checked = settings.privacy.gpc;

  byId("compare").checked = settings.shopping.compare;
  byId("coupons").checked = settings.shopping.coupons && settings.shopping.autoApply;
  byId("shop-stats").textContent = `${month.compared || 0} products compared and ${month.coupons || 0} codes that worked this month.`;

  showLight(settings.light || { preset: "off", dim: 0, warmth: 0 });
  byId("mode-now").textContent = settings.modes.profile ? settings.modes.profile + (site && settings.modes.siteModes[site.site] ? " · this site: " + settings.modes.siteModes[site.site] : "") : site && settings.modes.siteModes[site.site] ? "this site: " + settings.modes.siteModes[site.site] : "none";
  for (const element of document.querySelectorAll("button[data-profile]")) element.classList.toggle("on", settings.modes.profile === element.dataset.profile);
  byId("guard-screen").checked = settings.capture.guardScreen;
  byId("guard-camera").checked = settings.capture.guardCamera;
  byId("watch-count").textContent = (month.captures || 0) + " stopped";
  byId("watch-note").textContent = "Other extensions that could record the screen are listed under settings.";
  Shield.send({ type: "report.weekly" }).then((report) => { if (report && report.text) byId("week").textContent = "This week: " + report.text; });
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

async function connect(serverId) {
  if (serverId === "auto") {
    byId("tunnel-status").textContent = "Trying every location once…";
    const result = await Shield.send({ type: "tunnel.speedTest", pickFastest: true });
    if (!result || !result.picked) byId("tunnel-status").textContent = "None of the locations answered. Check them under settings.";
  } else {
    await Shield.send({ type: "settings.update", change: { tunnel: { enabled: true, serverId } } });
  }
  await refresh();
  setTimeout(refresh, 2500);
}
byId("tunnel-on").addEventListener("click", async () => {
  if (!current) return;
  if (current.settings.tunnel.enabled) {
    await Shield.send({ type: "settings.update", change: { tunnel: { enabled: false } } });
    await refresh();
    setTimeout(refresh, 2500);
    return;
  }
  if (!current.servers.length) {
    byId("tunnel-status").textContent = "Add a location first, under settings.";
    return;
  }
  await connect(byId("server").value || "auto");
});
byId("server").addEventListener("change", async (event) => {
  // Choosing a place while connected switches to it at once.
  if (current && current.settings.tunnel.enabled) await connect(event.target.value || "auto");
});
bindSetting("kill", ["tunnel", "killSwitch"]);
bindSetting("webrtc", ["tunnel", "webRtcGuard"]);
byId("fastest").addEventListener("click", () => connect("auto"));

// ---- light -----------------------------------------------------------------------------------
const LIGHT_PRESETS = { sunny: [0, 0], dark: [0.45, 0.35], restaurant: [0.25, 0.5], night: [0.55, 0.7] };
const LIGHT_NAMES = { off: "off", sunny: "bright and sunny", dark: "in a dark room", restaurant: "restaurant", night: "night", custom: "your own" };
function showLight(light) {
  byId("light-dim").value = Math.round((light.dim || 0) * 100);
  byId("light-warm").value = Math.round((light.warmth || 0) * 100);
  byId("light-now").textContent = LIGHT_NAMES[light.preset] || light.preset || "off";
  for (const button of document.querySelectorAll("button[data-light]")) button.classList.toggle("on", light.preset === button.dataset.light);
}
async function setLight(preset, dim, warmth) {
  const result = await Shield.send({ type: "light.set", preset, dim, warmth });
  if (result && result.light) { showLight(result.light); if (current) current.settings.light = result.light; }
}
for (const button of document.querySelectorAll("button[data-light]")) {
  button.addEventListener("click", () => {
    const [dim, warmth] = LIGHT_PRESETS[button.dataset.light];
    const same = current && current.settings.light && current.settings.light.preset === button.dataset.light;
    if (same) setLight("off", 0, 0); else setLight(button.dataset.light, dim, warmth);
  });
}
byId("light-dim").addEventListener("input", () => setLight("custom", Number(byId("light-dim").value) / 100, Number(byId("light-warm").value) / 100));
byId("light-warm").addEventListener("input", () => setLight("custom", Number(byId("light-dim").value) / 100, Number(byId("light-warm").value) / 100));

// ---- frequency -------------------------------------------------------------------------------
(function tone() {
  const select = byId("tone-preset");
  const custom = document.createElement("option");
  custom.value = "";
  custom.textContent = "your own frequency";
  select.append(custom);
  for (const preset of Shield.TONE_PRESETS) {
    const option = document.createElement("option");
    option.value = String(preset.hz);
    option.textContent = preset.name;
    select.append(option);
  }
  const show = (state) => {
    if (!state) return;
    byId("tone-toggle").textContent = state.playing ? "stop" : "play";
    byId("tone-toggle").classList.toggle("on", Boolean(state.playing));
    byId("tone-now").textContent = state.playing ? `${state.hz} Hz${state.binaural ? " · binaural, use headphones" : ""}` : "off";
    if (state.hz) byId("tone-hz").value = state.hz;
    if (state.volume !== undefined) byId("tone-volume").value = Math.round(state.volume * 100);
    const match = Shield.TONE_PRESETS.find((preset) => preset.hz === Number(state.hz));
    select.value = match ? String(match.hz) : "";
  };
  Shield.send({ type: "tone.state" }).then(show);
  select.addEventListener("change", async () => {
    if (!select.value) return;
    byId("tone-hz").value = select.value;
    const state = await Shield.send({ type: "tone.state" });
    if (state && state.playing) show(await Shield.send({ type: "tone.play", hz: Number(select.value), volume: Number(byId("tone-volume").value) / 100 }));
  });
  byId("tone-toggle").addEventListener("click", async () => {
    const state = await Shield.send({ type: "tone.state" });
    if (state && state.playing) show(await Shield.send({ type: "tone.stop" }));
    else show(await Shield.send({ type: "tone.play", hz: Number(byId("tone-hz").value), volume: Number(byId("tone-volume").value) / 100 }));
  });
  byId("tone-hz").addEventListener("change", async () => {
    const hz = Math.min(Shield.TONE_MAX, Math.max(Shield.TONE_MIN, Number(byId("tone-hz").value) || 432));
    byId("tone-hz").value = hz;
    const match = Shield.TONE_PRESETS.find((preset) => preset.hz === hz);
    select.value = match ? String(match.hz) : "";
    const state = await Shield.send({ type: "tone.state" });
    if (state && state.playing) show(await Shield.send({ type: "tone.play", hz, volume: Number(byId("tone-volume").value) / 100 }));
  });
  byId("tone-volume").addEventListener("input", async () => {
    const state = await Shield.send({ type: "tone.state" });
    if (state && state.playing) show(await Shield.send({ type: "tone.volume", volume: Number(byId("tone-volume").value) / 100 }));
  });
})();
// While the VPN is connecting or holding, the card follows along instead of
// showing the moment the popup opened.
setInterval(() => {
  if (current && current.tunnel && ["checking", "held"].includes(current.tunnel.state)) refresh();
}, 2000);

// ---- capture ----------------------------------------------------------------------------------
async function capture(mode) {
  const status = byId("capture-status");
  status.textContent = mode === "full" ? "taking the whole page…" : mode === "area" ? "drag over the page…" : "taking…";
  const result = await Shield.send({ type: "capture.start", mode, blur: byId("capture-blur").checked });
  if (result.error) status.textContent = result.error;
  else if (result.cancelled) status.textContent = "";
  else status.textContent = "saved to Downloads/noah-shield" + (result.blurred ? ` · ${result.blurred} private detail${result.blurred === 1 ? "" : "s"} blurred` : "");
}
byId("shot-visible").addEventListener("click", () => capture("visible"));
byId("shot-full").addEventListener("click", () => capture("full"));
byId("shot-area").addEventListener("click", () => capture("area"));
byId("record").addEventListener("click", () => Shield.send({ type: "record.open" }));
Shield.api.storage.local.get("captureBlur").then((stored) => { byId("capture-blur").checked = stored.captureBlur !== false; });
byId("capture-blur").addEventListener("change", (event) => Shield.api.storage.local.set({ captureBlur: event.target.checked }));
byId("route").addEventListener("click", async () => {
  if (!current || !current.site) return;
  const site = current.site.site;
  const existing = current.settings.tunnel.siteRoutes[site];
  if (existing) {
    await Shield.send({ type: "tunnel.route", site, serverId: null });
  } else {
    const choice = prompt(`Send ${site} through which location? Type a country or a name from the list, or "direct" to keep this site off the VPN.`, "direct");
    if (choice === null) return;
    const typed = choice.trim().toLowerCase();
    const server = current.servers.find((entry) => entry.name.toLowerCase() === typed || entry.id === typed || countryName(entry.country).toLowerCase() === typed || (entry.city || "").toLowerCase() === typed);
    await Shield.send({ type: "tunnel.route", site, serverId: choice.trim().toLowerCase() === "direct" ? "direct" : server ? server.id : null });
  }
  await refresh();
});
byId("policy").addEventListener("click", async () => {
  if (!current || !current.site) return;
  const result = await Shield.send({ type: "tools.policy", tabId: current.site.tabId });
  const out = byId("alias-out");
  out.hidden = false;
  if (result.error) { out.textContent = result.error; return; }
  out.textContent = `Policy grade ${result.grade} (${result.minutes} min of reading). ${result.lines.map((line) => line.line).join(" ")}`;
});
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
for (const element of document.querySelectorAll("button[data-profile]")) {
  element.addEventListener("click", async () => {
    await Shield.send({ type: "profile.apply", name: element.dataset.profile });
    await refresh();
  });
}
byId("panic").addEventListener("click", async () => {
  if (!confirm("Close every tab, clear history and drop the tunnel?")) return;
  await Shield.send({ type: "panic" });
  window.close();
});
byId("tools").addEventListener("click", (event) => {
  event.preventDefault();
  Shield.api.tabs.create({ url: Shield.api.runtime.getURL("tools.html") });
});
byId("leak").addEventListener("click", (event) => {
  event.preventDefault();
  Shield.api.tabs.create({ url: Shield.api.runtime.getURL("leak.html") });
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
