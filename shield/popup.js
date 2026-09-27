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
  if (server.id === Shield.NOAH_TOR_ID) return server.country ? flag(server.country) + countryName(server.country) + " · through noah" : "fastest location · through noah";
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
    case "checking": {
      const percent = Number.isFinite(status.progress) ? ` ${status.progress}%` : "";
      const note = status.note ? ` · ${status.note}` : "";
      return [`Connecting to ${place}…${percent}${note}`, "checking", "connecting" + percent];
    }
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
  const noah = servers.find((server) => server.id === Shield.NOAH_TOR_ID);
  byId("vpn-noah").hidden = Boolean(noah);
  if (noah) {
    // noah's route first: the fastest exit, then a country to come out in.
    const fastest = document.createElement("option");
    fastest.value = Shield.NOAH_TOR_ID + ":";
    fastest.textContent = "fastest location · through noah";
    select.append(fastest);
    const group = document.createElement("optgroup");
    group.label = "come out in";
    for (const code of Shield.TOR_COUNTRIES) {
      const option = document.createElement("option");
      option.value = Shield.NOAH_TOR_ID + ":" + code;
      option.textContent = flag(code) + countryName(code);
      group.append(option);
    }
    select.append(group);
  } else {
    const auto = document.createElement("option");
    auto.value = "auto";
    auto.textContent = "fastest of the locations below";
    select.append(auto);
  }
  const others = servers.filter((server) => server.id !== Shield.NOAH_TOR_ID).sort((left, right) => (left.id === "tor-local") - (right.id === "tor-local") || countryName(left.country).localeCompare(countryName(right.country)));
  if (others.length) {
    const group = document.createElement("optgroup");
    group.label = noah ? "other routes" : "routes";
    for (const server of others) {
      const option = document.createElement("option");
      option.value = server.id;
      option.textContent = serverLabel(server);
      group.append(option);
    }
    select.append(group);
  }
  const chosen = settings.tunnel.serverId === Shield.NOAH_TOR_ID
    ? Shield.NOAH_TOR_ID + ":" + (settings.tunnel.torCountry || "")
    : settings.tunnel.serverId && servers.some((server) => server.id === settings.tunnel.serverId) ? settings.tunnel.serverId : select.options[0].value;
  select.value = chosen;
  if (select.value !== chosen) select.selectedIndex = 0;
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

  showLight(settings.light || { preset: "off", brightness: null, warmth: 0 });
  byId("search-clean").checked = settings.search.clean;
  byId("search-farms").checked = settings.search.farms;
  byId("search-look").checked = settings.search.look;
  byId("search-peek").checked = settings.search.peek;
  byId("search-now").textContent = (today.paidResults || 0) + (today.farmResults || 0) ? `${(today.paidResults || 0) + (today.farmResults || 0)} results removed today` : "";
  byId("mode-now").textContent = settings.modes.profile ? settings.modes.profile + (site && settings.modes.siteModes[site.site] ? " · this site: " + settings.modes.siteModes[site.site] : "") : site && settings.modes.siteModes[site.site] ? "this site: " + settings.modes.siteModes[site.site] : "none";
  for (const element of document.querySelectorAll("button[data-profile]")) element.classList.toggle("on", settings.modes.profile === element.dataset.profile);
  byId("guard-screen").checked = settings.capture.guardScreen;
  byId("guard-camera").checked = settings.capture.guardCamera;
  byId("watch-count").textContent = (month.captures || 0) + " stopped";
  byId("watch-note").textContent = "Other extensions that could record the screen are listed under settings.";
  Shield.send({ type: "report.weekly" }).then((report) => { if (report && report.text) byId("week").textContent = "This week: " + report.text; });

  const persona = settings.persona || { enabled: true, autoSeed: true };
  byId("persona-enabled").checked = Boolean(persona.enabled);
  byId("persona-autoseed").checked = Boolean(persona.autoSeed);
  byId("persona-status").textContent = persona.enabled ? (persona.autoSeed ? "on · auto-seed" : "on · manual") : "off";

  const profileIntel = settings.profileIntel || { enabled: true };
  byId("profile-enabled").checked = Boolean(profileIntel.enabled);
  byId("profile-status").textContent = profileIntel.enabled ? "on" : "off";

  const spaces = settings.spaces || { enabled: true, newAccountDays: 30, flagLowFollowers: true, lowFollowersUnder: 20 };
  byId("spaces-enabled").checked = Boolean(spaces.enabled);
  byId("spaces-lowfollow").checked = Boolean(spaces.flagLowFollowers);
  byId("spaces-days").value = String(spaces.newAccountDays);
  byId("spaces-underfollow").value = String(spaces.lowFollowersUnder);
  const isXTab = Boolean(site && /(^|\.)(twitter\.com|x\.com)$/.test(site.host || ""));
  byId("spaces-status").textContent = spaces.enabled ? (isXTab ? "on · watching this tab" : "on") : "off";
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

async function connect(choice) {
  if (choice === "auto") {
    byId("tunnel-status").textContent = "Trying every location once…";
    const result = await Shield.send({ type: "tunnel.speedTest", pickFastest: true });
    if (!result || !result.picked) byId("tunnel-status").textContent = "None of the locations answered. Check them under settings.";
  } else if (choice.startsWith(Shield.NOAH_TOR_ID + ":")) {
    byId("tunnel-status").textContent = "Asking noah to start Tor…";
    await Shield.send({ type: "tunnel.connect", serverId: Shield.NOAH_TOR_ID, country: choice.slice(Shield.NOAH_TOR_ID.length + 1) });
  } else {
    await Shield.send({ type: "tunnel.connect", serverId: choice });
  }
  await refresh();
  setTimeout(refresh, 2500);
}
byId("tunnel-on").addEventListener("click", async () => {
  if (!current) return;
  if (current.settings.tunnel.enabled) {
    byId("tunnel-status").textContent = "Disconnecting…";
    await Shield.send({ type: "tunnel.disconnect" });
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
// Scenes are brightness levels for the screen itself; nothing is laid over pages.
const LIGHT_PRESETS = { sunny: 100, day: 75, restaurant: 45, dark: 20, night: 8 };
const LIGHT_NAMES = { off: "", sunny: "bright and sunny", day: "daytime", restaurant: "restaurant", dark: "in a dark room", night: "night", custom: "your own" };
let lightRead = null;
function showLight(light) {
  if (light.brightness !== null && light.brightness !== undefined) byId("light-level").value = light.brightness;
  byId("light-warm").value = Math.round((light.warmth || 0) * 100);
  const level = light.brightness !== null && light.brightness !== undefined ? light.brightness + "%" : "";
  byId("light-now").textContent = [LIGHT_NAMES[light.preset] || "", level].filter(Boolean).join(" · ");
  for (const button of document.querySelectorAll("button[data-light]")) button.classList.toggle("on", light.preset === button.dataset.light);
}
async function readLight() {
  if (lightRead) return lightRead;
  lightRead = await Shield.send({ type: "light.read" });
  const note = byId("light-note");
  if (!lightRead || lightRead.error && !lightRead.present) {
    note.textContent = "Turning the screen's own brightness needs noah on this computer (free, noah.asherin.com/download). The warm tint below still works.";
    note.className = "muted warn";
    byId("light-level").disabled = true;
  } else if (!lightRead.supported) {
    note.textContent = "This screen does not take brightness commands from programs (most external monitors do not). " + (lightRead.error || "");
    note.className = "muted warn";
    byId("light-level").disabled = true;
  } else {
    byId("light-level").value = lightRead.level;
    byId("light-level").disabled = false;
    note.textContent = `The screen is at ${lightRead.level}% now. Scenes and the slider turn the real brightness; the warm tint is the one thing laid over pages, and only if you slide it up.`;
    note.className = "muted";
  }
  return lightRead;
}
async function setLight(preset, brightness, warmth) {
  const result = await Shield.send({ type: "light.set", preset, brightness, warmth });
  if (result && result.light) { showLight(result.light); if (current) current.settings.light = result.light; }
  if (result && result.error) { const note = byId("light-note"); note.textContent = result.error; note.className = "muted warn"; }
  else if (result && result.light && result.light.brightness !== null) { const note = byId("light-note"); note.textContent = `Screen brightness set to ${result.light.brightness}%.`; note.className = "muted"; }
}
for (const button of document.querySelectorAll("button[data-light]")) {
  button.addEventListener("click", () => setLight(button.dataset.light, LIGHT_PRESETS[button.dataset.light], undefined));
}
let levelTimer = null;
byId("light-level").addEventListener("input", () => {
  clearTimeout(levelTimer);
  levelTimer = setTimeout(() => setLight("custom", Number(byId("light-level").value), undefined), 250);
});
byId("light-warm").addEventListener("input", () => setLight(current && current.settings.light ? current.settings.light.preset : "custom", null, Number(byId("light-warm").value) / 100));
readLight();

// ---- search --------------------------------------------------------------------------------
bindSetting("search-clean", ["search", "clean"]);
bindSetting("search-farms", ["search", "farms"]);
bindSetting("search-look", ["search", "look"]);
bindSetting("search-peek", ["search", "peek"]);

// ---- what happened here ---------------------------------------------------------------------
byId("here").addEventListener("click", async () => {
  if (!current || !current.site) return;
  const list = byId("here-list");
  if (!list.hidden) { list.hidden = true; return; }
  list.replaceChildren();
  list.hidden = false;
  const [blocked, log] = await Promise.all([
    Shield.send({ type: "tab.blocked", tabId: current.site.tabId }),
    Shield.send({ type: "log.list", tabId: current.site.tabId, limit: 40 }),
  ]);
  const line = (text, className = "") => { const node = document.createElement("div"); node.textContent = text; if (className) node.className = className; list.append(node); };
  if (blocked && blocked.groups && blocked.groups.length) {
    line(`${blocked.total} request${blocked.total === 1 ? "" : "s"} stopped on this page:`);
    for (const group of blocked.groups.slice(0, 30)) {
      const who = group.owner ? `${group.owner.owner} (${group.owner.kind}${group.owner.country ? ", " + group.owner.country : ""})` : "";
      line(`  ${group.site} · ${group.count}× ${group.types.join("/")}${who ? " · " + who : ""}`);
    }
  } else {
    line("Nothing had to be stopped on this page so far.");
  }
  const entries = (log && log.entries) || [];
  if (entries.length) {
    line("What the shield did here:");
    for (const entry of entries.slice(0, 25)) {
      const when = new Date(entry.at);
      line(`  ${when.getHours().toString().padStart(2, "0")}:${when.getMinutes().toString().padStart(2, "0")} ${entry.text}${entry.detail ? " · " + entry.detail : ""}`);
    }
  }
  const more = document.createElement("a");
  more.href = "#";
  more.textContent = "open the whole log";
  more.addEventListener("click", (event) => { event.preventDefault(); Shield.api.tabs.create({ url: Shield.api.runtime.getURL("log.html") + "?site=" + encodeURIComponent(current.site.site || "") }); });
  list.append(more);
});

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
byId("log").addEventListener("click", (event) => {
  event.preventDefault();
  Shield.api.tabs.create({ url: Shield.api.runtime.getURL("log.html") });
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

async function pushSpaces(change) {
  await Shield.send({ type: "spaces.set", change });
  await refresh();
}
byId("spaces-enabled").addEventListener("change", (event) => pushSpaces({ enabled: event.target.checked }));

async function pushProfile(change) {
  await Shield.send({ type: "profile.set", change });
  await refresh();
}
byId("profile-enabled").addEventListener("change", (event) => pushProfile({ enabled: event.target.checked }));

async function pushPersona(change) {
  await Shield.send({ type: "persona.set", change });
  await refresh();
}
byId("persona-enabled").addEventListener("change", (event) => pushPersona({ enabled: event.target.checked }));
byId("persona-autoseed").addEventListener("change", (event) => pushPersona({ autoSeed: event.target.checked }));
byId("persona-reseed").addEventListener("click", async () => {
  await Shield.send({ type: "persona.reseed" });
});
byId("spaces-lowfollow").addEventListener("change", (event) => pushSpaces({ flagLowFollowers: event.target.checked }));
byId("spaces-days").addEventListener("change", (event) => pushSpaces({ newAccountDays: Math.max(1, Math.min(365, Number(event.target.value) || 30)) }));
byId("spaces-underfollow").addEventListener("change", (event) => pushSpaces({ lowFollowersUnder: Math.max(0, Math.min(100000, Number(event.target.value) || 20)) }));

async function refreshInspect() {
  const tab = await activeTab();
  if (!tab) return;
  const status = await Shield.send({ type: "inspect.status", tabId: tab.id });
  const on = Boolean(status && status.enabled);
  byId("inspect-toggle").textContent = on ? "stop inspecting" : "arm this tab and reload";
  byId("inspect-quiet").textContent = on ? "clear" : "arm without reload";
  byId("inspect-count").textContent = on ? "recording · click the shield icon on the tab to see live" : "off";
}
byId("inspect-toggle").addEventListener("click", async () => {
  const tab = await activeTab();
  if (!tab) return;
  const status = await Shield.send({ type: "inspect.status", tabId: tab.id });
  const wasOn = Boolean(status && status.enabled);
  if (wasOn) {
    // Turn it off; no reload.
    await Shield.send({ type: "inspect.arm", tabId: tab.id, on: false });
    await refreshInspect();
    return;
  }
  // Arm and reload so the panel picks up everything from the first request.
  await Shield.send({ type: "inspect.arm", tabId: tab.id, on: true });
  await Shield.api.tabs.reload(tab.id);
  window.close();
});
byId("inspect-quiet").addEventListener("click", async () => {
  const tab = await activeTab();
  if (!tab) return;
  const status = await Shield.send({ type: "inspect.status", tabId: tab.id });
  const wasOn = Boolean(status && status.enabled);
  if (wasOn) {
    // Clear the recorded events without touching the arm state.
    try { await Shield.api.tabs.sendMessage(tab.id, { type: "inspect.control", action: "clear" }); } catch {}
    return;
  }
  await Shield.send({ type: "inspect.arm", tabId: tab.id, on: true });
  await refreshInspect();
});
refreshInspect();

refresh();
