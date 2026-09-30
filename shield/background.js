// The background of noah shield. Chromium runs it as a service worker and
// pulls the pieces in with importScripts; Firefox lists the same files in the
// manifest and runs them as one event page.
"use strict";

if (typeof importScripts === "function") {
  importScripts("lib/common.js", "lib/feeds.js", "lib/host.js", "lib/tunnel.js", "lib/privacy.js", "lib/watch.js", "lib/shopping.js", "lib/brands.js", "lib/safety.js", "lib/owners.js", "lib/cities.js", "lib/tracking.js", "lib/tools.js", "lib/look.js");
}

const api = Shield.api;
const ALARMS = { feeds: 6 * 60, cookies: 12 * 60, tunnel: 10 };

async function applyAll(reason) {
  const settings = await Shield.loadSettings();
  await Shield.applyPrivacy(settings);
  await Shield.applyTunnel(settings);
  await Shield.applyHeaderRules(settings);
  await applyModes(settings);
  if (reason === "install" || reason === "startup") {
    await Shield.refreshFeeds();
    const status = await Shield.updateStatus();
    if (status.newer) await noteUpdate(status);
  }
}

async function notify(id, title, message) {
  // Every notice is in the log too, so one that flashed by can be read again.
  await Shield.log({ kind: "notice", text: title, detail: message });
  const settings = await Shield.loadSettings();
  if (settings.quiet || !api.notifications) return;
  try {
    await api.notifications.create(id, { type: "basic", iconUrl: "icons/128.png", title, message });
  } catch {
    // A browser that refuses notifications is not worth failing over.
  }
}

function openLog(filter = {}) {
  const query = new URLSearchParams(Object.entries(filter).filter(([, value]) => value)).toString();
  return api.tabs.create({ url: api.runtime.getURL("log.html") + (query ? "?" + query : "") });
}

async function noteUpdate(status) {
  const stored = await api.storage.local.get("updateNoted");
  if (stored.updateNoted === status.latest) return;
  await api.storage.local.set({ updateNoted: status.latest });
  await notify("shield-update", "noah shield " + status.latest + " is out", "You run " + status.running + ". Open the shield to get it.");
}

if (api.notifications && api.notifications.onClicked) {
  api.notifications.onClicked.addListener((id) => {
    (async () => {
      const session = await api.storage.session.get("reminderLinks");
      const url = (session.reminderLinks || {})[id];
      if (url && /^https?:\/\//.test(url)) await api.tabs.create({ url });
      else await openLog();
      api.notifications.clear(id);
    })().catch(() => {});
  });
}

api.runtime.onInstalled.addListener((details) => {
  applyAll("install").catch((error) => console.error("shield: install", error));
  if (details.reason === "install") {
    api.tabs.create({ url: Shield.SITE + "/shield?installed" }).catch?.(() => {});
  }
});
api.runtime.onStartup.addListener(() => {
  (async () => {
    const settings = await Shield.loadSettings();
    if (settings.tunnel.connectAtStartup && settings.tunnel.serverId && !settings.tunnel.enabled) {
      await Shield.updateSettings({ tunnel: { enabled: true } });
    }
    await applyAll("startup");
  })().catch((error) => console.error("shield: startup", error));
});

api.alarms.create("feeds", { periodInMinutes: ALARMS.feeds });
api.alarms.create("cookies", { delayInMinutes: 2, periodInMinutes: ALARMS.cookies });
api.alarms.create("tunnel", { periodInMinutes: ALARMS.tunnel });
api.alarms.create("watchlist", { delayInMinutes: 30, periodInMinutes: 6 * 60 });
api.alarms.create("battery", { periodInMinutes: 5 });
api.alarms.create("focus", { periodInMinutes: 1 });
api.alarms.create("weekly", { delayInMinutes: 24 * 60, periodInMinutes: 7 * 24 * 60 });

async function announceDrop(entry, from, to) {
  await notify("shield-drop-" + entry.key, "Price drop: " + entry.title.slice(0, 60), `${from} → ${to}. Open the shield's purchases page for the link.`);
}
api.alarms.onAlarm.addListener((alarm) => {
  (async () => {
    const settings = await Shield.loadSettings();
    if (alarm.name === "feeds") {
      await Shield.refreshFeeds();
      const status = await Shield.updateStatus();
      if (status.newer) await noteUpdate(status);
      if (settings.tunnel.enabled) await Shield.applyTunnel(settings);
    } else if (alarm.name === "cookies") {
      await Shield.purgeTrackerCookies(settings);
    } else if (alarm.name === "battery") {
      await Shield.discardIdleTabs(settings);
    } else if (alarm.name === "focus") {
      if (settings.modes.focus.enabled) await Shield.applyFocusHours(settings);
    } else if (alarm.name === "weekly") {
      if (settings.modes.weeklyReport) {
        const report = await Shield.weeklyReport();
        await notify("shield-weekly", "Your week with noah shield", report.text);
      }
    } else if (alarm.name.startsWith("expire-")) {
      const entry = await Shield.expireDownload(alarm.name.slice("expire-".length));
      if (entry) await notify(alarm.name, "A download expired", entry.name + " from " + entry.source + " was deleted as you asked.");
    } else if (alarm.name === "watchlist") {
      if (!settings.localOnly) await Shield.checkWatchlist(announceDrop);
    } else if (alarm.name.startsWith("reminder-")) {
      const reminder = await Shield.dueReminder(alarm.name);
      if (reminder) {
        const session = await api.storage.session.get("reminderLinks");
        const reminderLinks = session.reminderLinks || {};
        reminderLinks[reminder.id] = reminder.url;
        await api.storage.session.set({ reminderLinks });
        await notify(reminder.id, reminder.kind === "trial" ? "A trial is about to charge" : reminder.kind === "warranty" ? "A warranty is ending" : "You asked to be reminded", reminder.label + " (press to open)");
      }
    } else if (alarm.name === "tunnel" && settings.tunnel.enabled) {
      const before = await Shield.tunnelStatus();
      const after = await Shield.applyTunnel(settings);
      if (after.state === "held" && before.state !== "held") {
        await notify("shield-held", "The tunnel is holding your traffic", after.error + ". Nothing leaves until the server answers or you turn the tunnel off.");
      }
    }
  })().catch((error) => console.error("shield: alarm", alarm.name, error));
});

// Settings changed anywhere (popup, options, another window): apply again.
// Settings changes are applied one after another, each against the newest
// settings, so a slow earlier change cannot undo a later one with stale values.
let settingsQueue = Promise.resolve();
api.storage.onChanged.addListener((changes, area) => {
  if (area !== "local" || !changes.settings) return;
  const before = changes.settings.oldValue || {};
  const after = changes.settings.newValue || {};
  settingsQueue = settingsQueue.then(async () => {
    const settings = await Shield.loadSettings();
    if (JSON.stringify(before.privacy) !== JSON.stringify(after.privacy) || JSON.stringify(before.security) !== JSON.stringify(after.security)) await Shield.applyPrivacy(settings);
    if (JSON.stringify(before.tunnel) !== JSON.stringify(after.tunnel)) await Shield.applyTunnel(settings);
    await Shield.applyHeaderRules(settings);
    if (JSON.stringify(before.modes) !== JSON.stringify(after.modes)) await applyModes(settings);
    await Shield.pushSync(settings).catch((error) => console.warn("shield: sync", error));
  }).catch((error) => console.error("shield: settings", error));
});

// Requests the rulesets stopped show up here; that is the badge count.
if (api.webRequest && api.webRequest.onErrorOccurred) {
  api.webRequest.onErrorOccurred.addListener(
    (details) => {
      if (details.error !== "net::ERR_BLOCKED_BY_CLIENT" && details.error !== "NS_ERROR_ABORT") return;
      if (details.type === "main_frame") return;
      Shield.count("trackers", 1, details.tabId).catch(() => {});
      if (details.tabId >= 0) Shield.noteBlocked(details.tabId, details.url, details.type).catch(() => {});
      const tabSite = Shield.siteOf(Shield.hostOf(details.initiator || details.documentUrl || ""));
      if (tabSite) Shield.noteThirdParty(details.tabId, tabSite, Shield.hostOf(details.url), true).catch(() => {});
    },
    { urls: ["<all_urls>"] },
  );
}

if (api.webRequest && api.webRequest.onAuthRequired) {
  try {
    api.webRequest.onAuthRequired.addListener((details) => Shield.proxyCredentials(details), { urls: ["<all_urls>"] }, ["blocking"]);
  } catch (error) {
    console.warn("shield: proxy credentials unavailable", error);
  }
}

if (api.webNavigation) {
  api.webNavigation.onCommitted.addListener((details) => {
    if (details.frameId === 0) {
      Shield.resetTabCount(details.tabId).catch(() => {});
      Shield.forgetBlocked(details.tabId).catch(() => {});
    }
    if (/^https?:/.test(details.url || "")) hideAds(details).catch(() => {});
  });
}

// ---- the sound host ----------------------------------------------------------------------------
// Chrome plays through an offscreen document that lives as long as the sound;
// Firefox has none, so the tools page hosts the sound there.
async function toneHost(message, create = true) {
  if (api.offscreen) {
    const contexts = api.runtime.getContexts ? await api.runtime.getContexts({ contextTypes: ["OFFSCREEN_DOCUMENT"] }) : [];
    if (!contexts.length) {
      if (!create) return { playing: false };
      await api.offscreen.createDocument({ url: "offscreen.html", reasons: ["AUDIO_PLAYBACK"], justification: "plays the frequency the person chose under their videos" });
    }
    const state = await new Promise((resolve) => api.runtime.sendMessage({ target: "tone-host", ...message }, (response) => { void api.runtime.lastError; resolve(response || {}); }));
    if (message.action === "stop" && api.offscreen.closeDocument) await api.offscreen.closeDocument().catch(() => {});
    return state;
  }
  const url = api.runtime.getURL("tools.html");
  let [tab] = await api.tabs.query({ url });
  if (!tab) {
    if (!create) return { playing: false };
    tab = await api.tabs.create({ url, active: false });
    await new Promise((resolve) => setTimeout(resolve, 1500));
  }
  return new Promise((resolve) => api.tabs.sendMessage(tab.id, { target: "tone-host", ...message }, (response) => { void api.runtime.lastError; resolve(response || {}); }));
}

// Firefox wears the look on its frame; elsewhere the pages and the new tab do.
async function wearLook(look) {
  if (!api.theme || !api.theme.update) return;
  try {
    if (look) await api.theme.update(Shield.themeFromPalette(look.palette));
    else if (api.theme.reset) await api.theme.reset();
  } catch (error) {
    console.warn("shield: theme", error);
  }
}

// ---- ad slots ---------------------------------------------------------------------------------
// The network rules stop ad requests; what the page reserved for them (the
// boxes, the "sponsored" rails) is hidden with EasyList's element rules: one
// stylesheet of generic selectors on every page, plus the site's own.
let cosmeticTable = null;
async function siteHidingCss(host) {
  if (!cosmeticTable) {
    const response = await fetch(api.runtime.getURL("rules/cosmetic.json"));
    cosmeticTable = await response.json();
  }
  const labels = host.split(".");
  const allowed = new Set();
  const selectors = [];
  for (let index = 0; index < labels.length - 1; index++) {
    const domain = labels.slice(index).join(".");
    for (const selector of cosmeticTable.allow[domain] || []) allowed.add(selector);
  }
  for (let index = 0; index < labels.length - 1; index++) {
    const domain = labels.slice(index).join(".");
    for (const selector of cosmeticTable.hide[domain] || []) if (!allowed.has(selector)) selectors.push(selector);
  }
  if (!selectors.length) return "";
  const rules = [];
  for (let start = 0; start < selectors.length; start += 250) {
    rules.push(":is(" + selectors.slice(start, start + 250).join(", ") + ") { display: none !important; }");
  }
  return rules.join("\n");
}

async function hideAds(details) {
  if (!api.scripting || !api.scripting.insertCSS) return;
  const settings = await Shield.loadSettings();
  if (!settings.privacy.trackers) return;
  const host = Shield.hostOf(details.url);
  if (settings.privacy.trustedSites.includes(Shield.siteOf(host))) return;
  const target = { tabId: details.tabId, frameIds: [details.frameId] };
  const insert = async (what) => {
    try {
      await api.scripting.insertCSS({ target, origin: "USER", ...what });
    } catch {
      // Firefox has no USER origin for extension pages' styles; the sheet still applies.
      await api.scripting.insertCSS({ target, ...what }).catch(() => {});
    }
  };
  await insert({ files: ["rules/cosmetic.css"] });
  const css = await siteHidingCss(host);
  if (css) await insert({ css });
}
api.tabs.onRemoved.addListener((tabId) => {
  Shield.resetTabCount(tabId).catch(() => {});
  Shield.forgetBlocked(tabId).catch(() => {});
  api.storage.session.get("mediaTabs").then((session) => {
    const mediaTabs = session.mediaTabs || {};
    if (mediaTabs[tabId]) { delete mediaTabs[tabId]; return api.storage.session.set({ mediaTabs }); }
    return null;
  }).catch(() => {});
  Shield.loadSettings().then((settings) => Shield.tabClosed(tabId, settings)).catch(() => {});
});

const proxyErrors = api.proxy && (api.proxy.onProxyError || api.proxy.onError);
if (proxyErrors) {
  proxyErrors.addListener((details) => {
    api.storage.session.set({ tunnelError: { at: new Date().toISOString(), details: String(details && (details.details || details.message) || details) } }).catch?.(() => {});
  });
}

async function siteState(tabId, url) {
  const settings = await Shield.loadSettings();
  const host = Shield.hostOf(url || "");
  const site = Shield.siteOf(host);
  return {
    host,
    site,
    web: /^https?:/.test(url || ""),
    trusted: settings.privacy.trustedSites.includes(site),
    strictCookies: settings.privacy.strictCookieSites.includes(site),
    captureAllowed: settings.capture.allowedSites.includes(site),
    shoppingQuiet: settings.shopping.quietSites.includes(site),
    blocked: tabId === undefined ? 0 : await Shield.tabCount(tabId),
  };
}

function toggleInList(list, value, present) {
  const set = new Set(list);
  if (present) set.add(value);
  else set.delete(value);
  return Array.from(set);
}

async function onceAllowed(site, kind) {
  const session = await api.storage.session.get("captureOnce");
  const grants = session.captureOnce || {};
  const grant = grants[site + "|" + kind];
  return Boolean(grant && Date.now() - grant < 10 * 60 * 1000);
}

// Token buckets per tab and kind: a page that floods the worker with lookups
// (breach prefixes, store searches) gets a quiet refusal, not a queue that
// runs on its behalf.
const rateBuckets = new Map();
function allowRate(kind, sender, perMinute) {
  const key = kind + "|" + (sender && sender.tab ? sender.tab.id : "popup");
  const now = Date.now();
  let bucket = rateBuckets.get(key);
  if (!bucket || now - bucket.start > 60000) {
    bucket = { start: now, used: 0 };
    rateBuckets.set(key, bucket);
  }
  if (rateBuckets.size > 2000) {
    for (const [otherKey, other] of rateBuckets) if (now - other.start > 60000) rateBuckets.delete(otherKey);
  }
  bucket.used += 1;
  return bucket.used <= perMinute;
}

const handlers = {
  async state(message, sender) {
    const [settings, tunnel, stats, update, feeds] = await Promise.all([
      Shield.loadSettings(), Shield.tunnelStatus(), Shield.statsSummary(), Shield.updateStatus(), Shield.loadFeeds(),
    ]);
    const tab = message.tabId !== undefined ? { id: message.tabId, url: message.url } : sender.tab ? { id: sender.tab.id, url: sender.tab.url } : null;
    const site = tab ? await siteState(tab.id, tab.url) : null;
    const servers = await Shield.allServers();
    return { settings, tunnel, stats, update, site, servers, feeds: Object.fromEntries(Object.entries(feeds).map(([name, entry]) => [name, { fetched: entry.fetched, error: entry.error, count: entry.data && (entry.data.servers || entry.data.coupons || []).length }])) };
  },
  async "settings.update"(message) {
    return { settings: await Shield.updateSettings(message.change) };
  },
  async "site.set"(message) {
    const settings = await Shield.loadSettings();
    const site = String(message.site || "");
    if (!site) return { error: "no site" };
    const change = {};
    if (message.field === "trusted") change.privacy = { trustedSites: toggleInList(settings.privacy.trustedSites, site, message.value) };
    else if (message.field === "strictCookies") change.privacy = { strictCookieSites: toggleInList(settings.privacy.strictCookieSites, site, message.value) };
    else if (message.field === "captureAllowed") change.capture = { allowedSites: toggleInList(settings.capture.allowedSites, site, message.value) };
    else if (message.field === "shoppingQuiet") change.shopping = { quietSites: toggleInList(settings.shopping.quietSites, site, message.value) };
    else return { error: "unknown field" };
    return { settings: await Shield.updateSettings(change) };
  },
  async "tunnel.apply"() {
    const settings = await Shield.loadSettings();
    const status = await Shield.applyTunnel(settings);
    await Shield.applyHeaderRules(settings);
    return { status };
  },
  async "tunnel.connect"(message) {
    const serverId = String(message.serverId || "");
    if (serverId === Shield.NOAH_TOR_ID) {
      // The popup closes long before Tor is up; the connect goes on without it.
      Shield.connectNoahTor(message.country).catch((error) => console.warn("shield: noah tor", error));
      await new Promise((resolve) => setTimeout(resolve, 300));
      return { status: await Shield.tunnelStatus() };
    }
    const settings = await Shield.updateSettings({ tunnel: { enabled: true, serverId } });
    return { status: await Shield.applyTunnel(settings) };
  },
  async "tunnel.disconnect"() {
    return { status: await Shield.disconnectTunnel() };
  },
  async "tunnel.speedTest"(message) {
    const settings = await Shield.loadSettings();
    const results = await Shield.speedTest(settings, Array.isArray(message.ids) ? message.ids : null);
    if (message.pickFastest) {
      const best = results.find((result) => result.state === "up");
      if (best) await Shield.updateSettings({ tunnel: { enabled: true, serverId: best.id } });
      return { results, picked: best ? best.id : null };
    }
    return { results };
  },
  async "tunnel.route"(message) {
    const settings = await Shield.loadSettings();
    const site = String(message.site || "");
    if (!site) return { error: "no site" };
    const siteRoutes = { ...settings.tunnel.siteRoutes };
    if (!message.serverId) delete siteRoutes[site];
    else siteRoutes[site] = String(message.serverId);
    const updated = await Shield.updateSettings({ tunnel: { siteRoutes } });
    await Shield.applyTunnel(updated);
    return { siteRoutes };
  },
  async "media.state"(message, sender) {
    if (!sender.tab) return { ok: true };
    const session = await api.storage.session.get("mediaTabs");
    const mediaTabs = session.mediaTabs || {};
    if (message.camera || message.microphone) {
      mediaTabs[sender.tab.id] = { camera: Boolean(message.camera), microphone: Boolean(message.microphone), site: Shield.siteOf(Shield.hostOf(sender.url || "")), title: sender.tab.title || "", since: (mediaTabs[sender.tab.id] || {}).since || Date.now() };
    } else {
      delete mediaTabs[sender.tab.id];
    }
    await api.storage.session.set({ mediaTabs });
    return { ok: true };
  },
  async "media.list"() {
    const session = await api.storage.session.get("mediaTabs");
    return { tabs: Object.entries(session.mediaTabs || {}).map(([tabId, entry]) => ({ tabId: Number(tabId), ...entry })) };
  },
  // For the tools page: where traffic comes out and who steers the route.
  async "network.state"() {
    const settings = await Shield.loadSettings();
    const tunnel = await Shield.tunnelStatus();
    let control = null;
    let systemProxy = null;
    if (api.proxy && api.proxy.settings) {
      try {
        const current = await api.proxy.settings.get({});
        control = current.levelOfControl;
        const value = current.value || {};
        if (value.mode === "system" || value.mode === "auto_detect") systemProxy = null;
        else if (value.mode === "fixed_servers" && control !== "controlled_by_this_extension") {
          const rules = value.rules || {};
          const server = rules.singleProxy || rules.proxyForHttps || rules.proxyForHttp;
          systemProxy = server ? `${server.host}:${server.port}` : "a fixed proxy";
        } else if (value.mode === "pac_script" && control !== "controlled_by_this_extension") systemProxy = "a proxy script";
      } catch (error) {
        control = null;
      }
    }
    let exit = null;
    if (!settings.localOnly && tunnel.state !== "up") {
      try {
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), 8000);
        const response = await fetch(Shield.WHOAMI_URL, { cache: "no-store", credentials: "omit", signal: controller.signal });
        clearTimeout(timer);
        if (response.ok) {
          const body = await response.json();
          exit = { ip: String(body.ip || ""), country: String(body.country || ""), city: String(body.city || "") };
        }
      } catch (error) {
        exit = null;
      }
    }
    return { tunnel, control, systemProxy, exit };
  },
  // ---- light: the screen's real brightness, through noah -------------------------------------
  async "light.state"() {
    const settings = await Shield.loadSettings();
    return settings.light;
  },
  async "light.read"() {
    const info = await Shield.hostInfo();
    if (!info.present) return { present: false, error: info.error };
    const answer = await Shield.host({ type: "brightness.get" });
    return { present: true, supported: !answer.error, level: answer.level, error: answer.error };
  },
  async "light.set"(message) {
    const settings = await Shield.loadSettings();
    const light = {
      preset: String(message.preset || "custom").slice(0, 20),
      brightness: message.brightness === null || message.brightness === undefined ? null : Math.min(100, Math.max(0, Math.round(Number(message.brightness) || 0))),
      warmth: Math.min(1, Math.max(0, Number(message.warmth === undefined ? settings.light.warmth : message.warmth) || 0)),
    };
    let outcome = { ok: true };
    if (light.brightness !== null) {
      const info = await Shield.hostInfo();
      if (!info.present) outcome = { ok: false, error: "the screen's brightness needs noah on this computer" };
      else {
        const answer = await Shield.host({ type: "brightness.set", level: light.brightness });
        outcome = answer.error ? { ok: false, error: answer.error } : { ok: true };
      }
    }
    await Shield.updateSettings({ light });
    const tabs = await api.tabs.query({ url: ["http://*/*", "https://*/*"] });
    for (const tab of tabs) api.tabs.sendMessage(tab.id, { type: "light.apply", light }, () => void api.runtime.lastError);
    await Shield.log({
      kind: "light",
      text: light.brightness === null ? "light: scene off" : outcome.ok ? `screen brightness set to ${light.brightness}%` : `screen brightness could not be set`,
      detail: [light.preset !== "custom" ? "scene: " + light.preset : "", light.warmth ? `warm tint ${Math.round(light.warmth * 100)}%` : "", outcome.error || ""].filter(Boolean).join(" · "),
    });
    return { light, ...outcome };
  },
  // ---- the log: what the shield did, in words ---------------------------------------------------
  async "log.list"(message) {
    return { entries: await Shield.logEntries({ kind: String(message.kind || ""), site: String(message.site || ""), tabId: Number.isInteger(message.tabId) ? message.tabId : null, limit: Math.min(3000, Number(message.limit) || 500) }) };
  },
  async "log.clear"() {
    await Shield.clearLog();
    return { ok: true };
  },
  async "log.open"(message, sender) {
    // A page can ask for the log only a few times a minute: the button under
    // the search counter is a person's click, a loop is not.
    if (sender && sender.tab && !allowRate("logopen", sender, 3)) return { ok: false };
    await openLog({ site: sender && sender.tab ? Shield.siteOf(Shield.hostOf(sender.tab.url || "")) : String(message.site || "") });
    return { ok: true };
  },
  // What was stopped on one tab, grouped by who was on the other end.
  async "tab.blocked"(message) {
    const tabId = Number(message.tabId);
    const entries = await Shield.blockedOnTab(tabId);
    const groups = new Map();
    for (const entry of entries) {
      const key = Shield.siteOf(entry.host) || entry.host;
      const group = groups.get(key) || { site: key, hosts: new Set(), count: 0, types: new Set(), owner: Shield.ownerOf ? Shield.ownerOf(entry.host) : null, last: 0 };
      group.hosts.add(entry.host);
      group.count += 1;
      group.types.add(entry.type);
      group.last = Math.max(group.last, entry.at);
      groups.set(key, group);
    }
    const list = Array.from(groups.values()).map((group) => ({ ...group, hosts: Array.from(group.hosts).slice(0, 6), types: Array.from(group.types) })).sort((left, right) => right.count - left.count);
    return { total: entries.length, groups: list };
  },
  // ---- search: cleaned results ---------------------------------------------------------------
  async "search.state"() {
    const settings = await Shield.loadSettings();
    const stored = await api.storage.local.get("look");
    const look = stored.look || null;
    const image = look && look.image ? (look.image.startsWith("data:") ? look.image : api.runtime.getURL("looks/" + look.image)) : null;
    return { clean: settings.search.clean, farms: settings.search.farms, look: settings.search.look && Boolean(image), peek: settings.search.peek, image, palette: look ? look.palette : null };
  },
  // A result's page, fetched once without cookies or a referrer, for the
  // "peek" under it. Only when asked, only web pages, never local addresses.
  async "search.peek"(message, sender) {
    const settings = await Shield.loadSettings();
    if (!settings.search.peek || settings.localOnly) return { error: "peeking is off" };
    if (!allowRate("peek", sender, 20)) return { error: "too many in a minute; a moment" };
    let url;
    try { url = new URL(String(message.url || "")); } catch { return { error: "not a web address" }; }
    if (!/^https?:$/.test(url.protocol) || Shield.isLocalHost(url.hostname)) return { error: "not a web address" };
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 9000);
    try {
      const response = await fetch(url.href, { credentials: "omit", referrerPolicy: "no-referrer", redirect: "follow", cache: "default", signal: controller.signal, headers: { accept: "text/html,application/xhtml+xml;q=0.9,*/*;q=0.5" } });
      if (!response.ok) return { error: "the site answered " + response.status };
      const type = response.headers.get("content-type") || "";
      if (!/html|xml/.test(type)) return { error: "not a page (" + type.split(";")[0] + ")" };
      if (Shield.isLocalHost(Shield.hostOf(response.url))) return { error: "the page moved somewhere local" };
      const reader = response.body.getReader();
      const chunks = [];
      let size = 0;
      while (size < 1500000) {
        const { value, done } = await reader.read();
        if (done) break;
        chunks.push(value);
        size += value.byteLength;
      }
      reader.cancel().catch(() => {});
      const html = new TextDecoder().decode(await new Blob(chunks).arrayBuffer());
      await Shield.count("peeks", 1, sender.tab ? sender.tab.id : null);
      return { html, url: response.url };
    } catch (error) {
      return { error: controller.signal.aborted ? "no answer in nine seconds" : String(error.message || error) };
    } finally {
      clearTimeout(timer);
    }
  },
  async "search.hidden"(message, sender) {
    if (!allowRate("search", sender, 30)) return { ok: false };
    const paid = Math.min(100, Math.max(0, Number(message.paid) || 0));
    const farms = Math.min(100, Math.max(0, Number(message.farms) || 0));
    const dimmed = Math.min(100, Math.max(0, Number(message.dimmed) || 0));
    if (paid) await Shield.count("paidResults", paid, sender.tab ? sender.tab.id : null);
    if (farms) await Shield.count("farmResults", farms, sender.tab ? sender.tab.id : null);
    const parts = [paid && `${paid} paid result${paid === 1 ? "" : "s"} hidden`, farms && `${farms} content-farm page${farms === 1 ? "" : "s"} hidden`, dimmed && `${dimmed} faded as written for the engine`].filter(Boolean);
    await Shield.log({ kind: "search", site: Shield.siteOf(Shield.hostOf(sender.url || "")), tabId: sender.tab ? sender.tab.id : null, text: `${String(message.engine || "search")}: ${parts.join(", ")}`, detail: message.query ? `search: ${String(message.query).slice(0, 120)}` : "" });
    return { ok: true };
  },
  // ---- noah on this computer ----------------------------------------------------------------------
  async "host.info"(message) {
    return Shield.hostInfo(Boolean(message.fresh));
  },
  // ---- frequency: a tone or a binaural beat under whatever plays ------------------------------
  async "tone.play"(message) {
    const state = await toneHost({ action: "play", hz: message.hz, volume: message.volume });
    await api.storage.session.set({ tone: state });
    return state;
  },
  async "tone.volume"(message) {
    const state = await toneHost({ action: "volume", volume: message.volume });
    await api.storage.session.set({ tone: state });
    return state;
  },
  async "tone.stop"() {
    const state = await toneHost({ action: "stop" }, false);
    await api.storage.session.set({ tone: state });
    return state;
  },
  async "tone.state"() {
    const session = await api.storage.session.get("tone");
    return session.tone || { playing: false, hz: 432, volume: 0.15 };
  },
  // ---- the look ---------------------------------------------------------------------------------
  async "look.set"(message) {
    const look = message.look;
    if (!look || typeof look !== "object" || !look.palette) return { error: "not a look" };
    const image = typeof look.image === "string" ? look.image : null;
    if (image && !image.startsWith("data:image/jpeg;base64,") && !/^[a-z]+\.jpg$/.test(image)) return { error: "not a picture" };
    if (image && image.length > 6 * 1024 * 1024) return { error: "the picture is too large; it is shrunk before saving, so try a smaller file" };
    const clean = { id: String(look.id || "custom").slice(0, 40), name: String(look.name || "").slice(0, 80), image, palette: {} };
    for (const [key, value] of Object.entries(look.palette)) {
      if (/^[a-zA-Z]+$/.test(key) && (typeof value === "string" && value.length < 60 || typeof value === "number" || typeof value === "boolean")) clean.palette[key] = value;
    }
    await api.storage.local.set({ look: clean });
    await wearLook(clean);
    return { look: clean };
  },
  async "look.reset"() {
    await api.storage.local.remove("look");
    await wearLook(null);
    return { ok: true };
  },
  async "stats.today"() {
    return Shield.statsSummary();
  },
  async "tunnel.check"() {
    const server = Shield.activeServer();
    if (!server) return { status: await Shield.tunnelStatus() };
    return { status: await Shield.checkExit(server) };
  },
  async "tunnel.addServer"(message) {
    try {
      return { server: await Shield.addServer(message.server) };
    } catch (error) {
      return { error: String(error.message || error) };
    }
  },
  async "tunnel.removeServer"(message) {
    await Shield.removeServer(String(message.id));
    const settings = await Shield.loadSettings();
    if (settings.tunnel.serverId === message.id) await Shield.updateSettings({ tunnel: { enabled: false, serverId: null } });
    return { ok: true };
  },
  async "privacy.purgeCookies"() {
    return { removed: await Shield.purgeTrackerCookies(await Shield.loadSettings()) };
  },
  async "feeds.refresh"() {
    await Shield.refreshFeeds();
    return { update: await Shield.updateStatus() };
  },
  async "watch.audit"() {
    return Shield.auditExtensions();
  },
  async "watch.setEnabled"(message) {
    try {
      await Shield.setExtensionEnabled(String(message.id), Boolean(message.enabled));
      return { ok: true };
    } catch (error) {
      return { error: String(error.message || error) };
    }
  },
  // The page-side guard asks what this site may do, once per document.
  async "guard.config"(message, sender) {
    const settings = await Shield.loadSettings();
    const site = Shield.siteOf(Shield.hostOf(sender.url || (sender.tab && sender.tab.url) || ""));
    const trusted = settings.privacy.trustedSites.includes(site);
    const security = settings.security;
    return {
      site,
      trusted,
      geolocation: trusted ? "ask" : settings.privacy.geolocation,
      fingerprint: settings.privacy.fingerprint && !trusted,
      guardScreen: settings.capture.guardScreen,
      guardCamera: settings.capture.guardCamera,
      shopping: settings.shopping.compare || settings.shopping.coupons,
      shoppingQuiet: settings.shopping.quietSites.includes(site),
      autoApply: settings.shopping.autoApply,
      clipCoupons: settings.shopping.clipCoupons,
      lookalike: security.lookalike,
      passwordReuse: security.passwordReuse,
      passwordHttp: security.passwordHttp,
      breachCheck: security.breachCheck && !settings.localOnly,
      clipboardGuard: security.clipboardGuard && !security.clipboardSites.includes(site),
      clipboardWipe: security.clipboardWipe,
      hiddenFields: security.hiddenFields,
      typingGuard: security.typingGuard && !trusted,
      blockKeyListeners: security.blockKeyListeners && !trusted,
      formLeak: security.formLeak && !trusted,
      walletGuard: security.walletGuard && !security.walletSites.includes(site),
      scamPopups: security.scamPopups,
      hiddenFrames: security.hiddenFrames,
      seed: await Shield.fingerprintSeed(site, settings),
      blendIn: settings.privacy.blendIn && !trusted,
      audioNoise: settings.privacy.audioNoise && settings.privacy.fingerprint && !trusted,
      webglNoise: settings.privacy.webglNoise && settings.privacy.fingerprint && !trusted,
      location: await locationFor(settings, trusted),
      hideNames: settings.privacy.hideNames,
      linkCleaner: settings.privacy.linkCleaner,
      tag: await elementTag(settings),
      annoyances: {
        banners: settings.annoyances.banners,
        overlays: settings.annoyances.overlays,
        autoplay: settings.annoyances.autoplay,
        timers: settings.annoyances.timers,
        copy: settings.annoyances.copySites.includes(site),
        dark: settings.annoyances.darkEverywhere || settings.annoyances.darkSites.includes(site),
      },
      pasteGuard: settings.annoyances.pasteGuard,
      uploadStrip: settings.annoyances.uploadStrip,
      lockMinutes: settings.modes.lock.sites.includes(site) && settings.modes.lock.pinHash ? settings.modes.lock.minutes : 0,
      mode: settings.modes.siteModes[site] || settings.modes.profile || "",
    };
  },
  async "lock.check"(message) {
    const settings = await Shield.loadSettings();
    const hash = await Shield.hashPin(String(message.pin || ""));
    return { ok: Boolean(settings.modes.lock.pinHash) && hash === settings.modes.lock.pinHash };
  },
  async "pin.set"(message) {
    const pin = String(message.pin || "");
    if (pin && !/^\d{4,12}$/.test(pin)) return { error: "a PIN is 4 to 12 digits" };
    const hash = pin ? await Shield.hashPin(pin) : "";
    if (message.target === "parental") {
      const settings = await Shield.loadSettings();
      if (settings.modes.parental.pinHash && (await Shield.hashPin(String(message.current || ""))) !== settings.modes.parental.pinHash) return { error: "the current PIN is needed to change it" };
      await Shield.updateSettings({ modes: { parental: { pinHash: hash } } });
    } else {
      await Shield.updateSettings({ modes: { lock: { pinHash: hash } } });
    }
    return { ok: true };
  },
  async "parental.set"(message) {
    const settings = await Shield.loadSettings();
    if (settings.modes.parental.pinHash && (await Shield.hashPin(String(message.pin || ""))) !== settings.modes.parental.pinHash) return { error: "the parental PIN is needed" };
    await Shield.updateSettings({ modes: { parental: { enabled: Boolean(message.enabled) } } });
    return { ok: true };
  },
  async "profile.apply"(message) {
    try {
      const settings = await Shield.applyProfile(String(message.name || ""));
      return { settings };
    } catch (error) {
      return { error: String(error.message || error) };
    }
  },
  async "report.weekly"() {
    return Shield.weeklyReport();
  },
  async "tools.unshorten"(message) {
    const settings = await Shield.loadSettings();
    if (settings.localOnly) return { error: "local-only mode: no lookups leave the browser" };
    const url = String(message.url || "");
    if (!/^https?:\/\//i.test(url)) return { error: "not a web address" };
    const result = await Shield.unshorten(url);
    result.lookalike = Shield.lookalike(Shield.hostOf(result.final));
    result.shortener = Shield.SHORTENERS.test(Shield.hostOf(url));
    return result;
  },
  async "tools.hops"(message) {
    return { hops: await Shield.hopsFor(Number(message.tabId)) };
  },
  async "vault.save"(message) {
    if (String(message.passphrase || "").length < 6) return { error: "use at least six characters" };
    await Shield.vaultSave(String(message.passphrase), Array.isArray(message.notes) ? message.notes.slice(0, 500) : []);
    return { ok: true };
  },
  async "vault.open"(message) {
    try {
      return { notes: await Shield.vaultOpen(String(message.passphrase || "")) };
    } catch {
      return { error: "wrong passphrase, or no vault yet" };
    }
  },
  async "tools.letter"(message) {
    return { text: Shield.deletionLetter({ name: String(message.name || ""), email: String(message.email || ""), company: String(message.company || ""), law: String(message.law || "") }), brokers: Shield.BROKERS };
  },
  async "tools.policy"(message) {
    const tabId = Number(message.tabId);
    let text = String(message.text || "");
    if (!text && tabId) {
      try {
        const [result] = await api.scripting.executeScript({ target: { tabId }, func: () => document.body ? document.body.innerText.slice(0, 400000) : "" });
        text = result && result.result ? result.result : "";
      } catch (error) {
        return { error: String(error.message || error) };
      }
    }
    return Shield.readPolicy(text);
  },
  async "tools.throwaway"(message) {
    const url = String(message.url || "");
    if (!/^https?:\/\//i.test(url)) return { error: "not a web address" };
    let allowed = false;
    try { allowed = await api.extension.isAllowedIncognitoAccess(); } catch { allowed = false; }
    if (allowed) {
      await api.windows.create({ url, incognito: true });
      return { ok: true, incognito: true };
    }
    const window = await api.windows.create({ url });
    return { ok: true, incognito: false, windowId: window.id, note: "Allow the shield in incognito (browser's extension page) for a truly throwaway window; this one is a normal window whose site cookies burn when it closes." };
  },
  async "container.open"(message) {
    const identities = api.contextualIdentities;
    if (!identities) return { error: "only Firefox has containers" };
    const url = String(message.url || "");
    const site = Shield.siteOf(Shield.hostOf(url));
    const existing = (await identities.query({ name: "shield: " + site }))[0];
    const identity = existing || (await identities.create({ name: "shield: " + site, color: "green", icon: "fence" }));
    await api.tabs.create({ url, cookieStoreId: identity.cookieStoreId });
    return { ok: true };
  },
  async "site.burn"(message) {
    return Shield.burnSite(String(message.site || ""), Array.isArray(message.origins) ? message.origins.map(String) : []);
  },
  async "site.watchers"(message) {
    return { watchers: await Shield.watchers(Number(message.tabId), String(message.site || "")) };
  },
  async "site.score"(message) {
    return Shield.privacyScore(Number(message.tabId), String(message.url || ""));
  },
  // Is this site worth trusting: has it leaked its users' data before, is it
  // a lookalike, is it plain http. For the popup only.
  async "site.trust"(message) {
    const url = String(message.url || "");
    const host = Shield.hostOf(url);
    const site = Shield.siteOf(host);
    if (!site) return { error: "not a site" };
    const settings = await Shield.loadSettings();
    const lookalike = settings.security.allowedLookalikes.includes(site) ? null : Shield.lookalike(host);
    let breaches = null;
    let breachError = null;
    if (settings.localOnly) breachError = "not checked in local-only mode";
    else {
      try {
        breaches = await Shield.breachHistory(site);
      } catch (error) {
        breachError = String(error.message || error);
      }
    }
    return { site, https: url.startsWith("https:"), lookalike, breaches, breachError };
  },
  async "alias.make"(message) {
    const settings = await Shield.loadSettings();
    const site = String(message.site || "");
    if (settings.privacy.simpleLoginKey && !settings.localOnly) {
      try {
        return { alias: await Shield.simpleLoginAlias(settings.privacy.simpleLoginKey, "noah shield for " + site), source: "SimpleLogin" };
      } catch (error) {
        return { error: String(error.message || error) };
      }
    }
    if (!settings.privacy.aliasBase) return { error: "set your email address under settings first; aliases are plus-addresses at it" };
    return { alias: Shield.makeAlias(settings.privacy.aliasBase, site), source: "plus address" };
  },
  async "decoy.fill"(message) {
    const settings = await Shield.loadSettings();
    const identity = Shield.decoyIdentity(settings.privacy.aliasBase, String(message.site || ""));
    const tabId = Number(message.tabId);
    try {
      await api.scripting.executeScript({ target: { tabId }, func: fillDecoy, args: [identity] });
    } catch (error) {
      return { error: String(error.message || error), identity };
    }
    return { identity };
  },
  async "fingerprint.rotate"() {
    await Shield.rotateFingerprint();
    return { ok: true };
  },
  async "sync.set"(message) {
    const passphrase = String(message.passphrase || "");
    if (passphrase.length < 8 && passphrase.length !== 0) return { error: "use at least eight characters" };
    if (!passphrase) {
      await api.storage.local.remove("syncPassphrase");
      await Shield.updateSettings({ privacy: { sync: false } });
      return { ok: true };
    }
    await api.storage.local.set({ syncPassphrase: passphrase });
    const settings = await Shield.updateSettings({ privacy: { sync: true } });
    await Shield.pushSync(settings);
    return { ok: true };
  },
  async "sync.pull"() {
    try {
      const pulled = await Shield.pullSync();
      if (!pulled) return { error: "nothing synced yet, or no passphrase" };
      await Shield.saveSettings(Shield.deepMerge(Shield.DEFAULT_SETTINGS, pulled.settings));
      return { ok: true, at: pulled.at };
    } catch (error) {
      return { error: "could not decrypt: wrong passphrase or damaged data (" + String(error.message || error) + ")" };
    }
  },
  async "permissions.log"() {
    const stored = await api.storage.local.get("permissionLog");
    return { log: stored.permissionLog || [] };
  },
  async "panic"() {
    await panic();
    return { ok: true };
  },
  // ---- screenshots and the recorder --------------------------------------------------------------
  // The popup starts a capture for the active tab; the tab's content script
  // drives it and comes back here for each shot and for the file. A tab may
  // only do that while a capture the person started is open for it.
  async "capture.start"(message) {
    const [tab] = await api.tabs.query({ active: true, currentWindow: true });
    if (!tab || !/^https?:/.test(tab.url || "")) return { error: "open a web page first; the browser's own pages cannot be captured" };
    const session = await api.storage.session.get("captureTabs");
    const captureTabs = session.captureTabs || {};
    captureTabs[tab.id] = Date.now() + 5 * 60 * 1000;
    await api.storage.session.set({ captureTabs });
    const mode = ["visible", "full", "area"].includes(message.mode) ? message.mode : "visible";
    try {
      // A tab open since before the shield was installed or updated has no
      // capture script yet; it is put in now rather than asking for a reload.
      const ready = !message.inject && await new Promise((resolve) => {
        api.tabs.sendMessage(tab.id, { type: "capture.ping" }, (response) => {
          void api.runtime.lastError;
          resolve(Boolean(response && response.ready));
        });
      });
      if (!ready) {
        if (!api.scripting || !api.scripting.executeScript) throw new Error("reload the page once, then try again");
        try {
          await api.scripting.executeScript({ target: { tabId: tab.id }, files: ["content/capture.js"] });
        } catch (error) {
          throw new Error("this page does not let extensions in (" + String(error.message || error).replace(/^Error: /, "") + ")");
        }
      }
      const result = await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("the page did not answer; reload it and try again")), 6 * 60 * 1000);
        api.tabs.sendMessage(tab.id, { type: "capture.run", mode, blur: Boolean(message.blur) }, (response) => {
          clearTimeout(timer);
          if (api.runtime.lastError) reject(new Error("the page did not answer; reload it and try again"));
          else resolve(response || {});
        });
      });
      const site = Shield.siteOf(Shield.hostOf(tab.url));
      if (result.error) await Shield.log({ kind: "capture", site, tabId: tab.id, text: `screenshot (${mode}) failed`, detail: result.error });
      else if (!result.cancelled) await Shield.log({ kind: "capture", site, tabId: tab.id, text: `screenshot (${mode === "full" ? "whole page" : mode === "area" ? "an area" : "what was on screen"}) saved to Downloads/noah-shield`, detail: result.blurred ? `${result.blurred} private detail(s) blurred first` : "nothing needed blurring" });
      return result;
    } catch (error) {
      await Shield.log({ kind: "capture", site: Shield.siteOf(Shield.hostOf(tab.url)), tabId: tab.id, text: `screenshot (${mode}) failed`, detail: String(error.message || error) });
      return { error: String(error.message || error) };
    } finally {
      const after = await api.storage.session.get("captureTabs");
      const remaining = after.captureTabs || {};
      delete remaining[tab.id];
      await api.storage.session.set({ captureTabs: remaining });
    }
  },
  async "capture.shot"(message, sender) {
    if (!sender.tab) return { error: "not a tab" };
    const session = await api.storage.session.get("captureTabs");
    const until = (session.captureTabs || {})[sender.tab.id];
    if (!until || until < Date.now()) return { error: "no capture is open for this page" };
    try {
      const dataUrl = await api.tabs.captureVisibleTab(sender.tab.windowId, { format: "png" });
      return { dataUrl };
    } catch (error) {
      return { error: String(error.message || error) };
    }
  },
  async "capture.save"(message, sender) {
    if (!sender.tab) return { error: "not a tab" };
    const session = await api.storage.session.get("captureTabs");
    const until = (session.captureTabs || {})[sender.tab.id];
    if (!until || until < Date.now()) return { error: "no capture is open for this page" };
    const dataUrl = String(message.dataUrl || "");
    const filename = String(message.filename || "");
    if (!dataUrl.startsWith("data:image/png;base64,")) return { error: "not a picture" };
    if (!/^noah-shield\/[A-Za-z0-9 ._-]+\.png$/.test(filename)) return { error: "bad file name" };
    try {
      const id = await api.downloads.download({ url: dataUrl, filename, saveAs: false, conflictAction: "uniquify" });
      return { ok: true, id };
    } catch (error) {
      return { error: String(error.message || error) };
    }
  },
  async "record.open"() {
    const url = api.runtime.getURL("record.html");
    const windows = await api.windows.getAll({ populate: true });
    for (const window of windows) {
      const tab = (window.tabs || []).find((entry) => entry.url === url);
      if (tab) {
        await api.windows.update(window.id, { focused: true });
        return { ok: true, reused: true };
      }
    }
    await api.windows.create({ url, type: "popup", width: 470, height: 760 });
    return { ok: true };
  },
  async "capture.ask"(message, sender) {
    const settings = await Shield.loadSettings();
    const site = Shield.siteOf(Shield.hostOf(sender.url || ""));
    const kind = ["screen", "camera", "clipboard", "wallet"].includes(message.kind) ? message.kind : "camera";
    const guarded = {
      screen: settings.capture.guardScreen,
      camera: settings.capture.guardCamera,
      clipboard: settings.security.clipboardGuard,
      wallet: settings.security.walletGuard,
    }[kind];
    if (!guarded) return { decision: "allow" };
    const allowed = {
      screen: settings.capture.allowedSites,
      camera: settings.capture.allowedSites,
      clipboard: settings.security.clipboardSites,
      wallet: settings.security.walletSites,
    }[kind];
    if (allowed.includes(site) || (await onceAllowed(site, kind))) return { decision: "allow" };
    pendingCaptureAsks.set(captureAskKey(sender, kind), Date.now());
    return { decision: "ask", site };
  },
  // A decision counts only as the answer to a question this worker asked
  // that tab; a script that never asked cannot grant itself the capture.
  async "capture.decide"(message, sender) {
    const site = Shield.siteOf(Shield.hostOf(sender.url || ""));
    const kind = ["screen", "camera", "clipboard", "wallet"].includes(message.kind) ? message.kind : "camera";
    const key = captureAskKey(sender, kind);
    const askedAt = pendingCaptureAsks.get(key);
    pendingCaptureAsks.delete(key);
    if (!askedAt || Date.now() - askedAt > 10 * 60 * 1000) return { error: "nothing was asked" };
    if (message.always) {
      const settings = await Shield.loadSettings();
      if (kind === "clipboard") await Shield.updateSettings({ security: { clipboardSites: toggleInList(settings.security.clipboardSites, site, true) } });
      else if (kind === "wallet") await Shield.updateSettings({ security: { walletSites: toggleInList(settings.security.walletSites, site, true) } });
      else await Shield.updateSettings({ capture: { allowedSites: toggleInList(settings.capture.allowedSites, site, true) } });
    } else if (message.allow) {
      const session = await api.storage.session.get("captureOnce");
      const grants = session.captureOnce || {};
      grants[site + "|" + kind] = Date.now();
      await api.storage.session.set({ captureOnce: grants });
    }
    if (!message.allow) await Shield.count(kind === "wallet" ? "wallets" : kind === "clipboard" ? "clipboard" : "captures", 1, sender.tab ? sender.tab.id : null);
    return { ok: true };
  },
  async "safety.event"(message, sender) {
    if (!allowRate("safety", sender, 120)) return { ok: false, limited: true };
    const kinds = new Set(["phishing", "passwordHttp", "reuse", "breached", "hiddenFields", "frames", "popups", "leaks", "keylog", "wallets", "clipboard", "fees", "darkPatterns", "traps", "reviewsChecked", "banners", "overlays", "autoplay", "timers", "pastes", "uploads"]);
    const kind = kinds.has(message.kind) ? message.kind : "other";
    const amount = Math.min(50, Math.max(1, Number(message.amount) || 1));
    await Shield.count(kind, amount, sender.tab ? sender.tab.id : null);
    const words = {
      phishing: "a page that imitates another site was stopped",
      passwordHttp: "a password field on a page without https was flagged",
      reuse: "a password you use elsewhere was typed here",
      breached: "a password known from a breach was typed here",
      hiddenFields: "hidden form fields were cleared before sending",
      frames: "hidden frames were removed",
      popups: "a scam pop-up was closed",
      leaks: "a form leak to a third party was stopped",
      keylog: "a script recording keystrokes was caught",
      wallets: "a page reached for a crypto wallet",
      clipboard: "a page read the clipboard without being asked",
      fees: "a hidden fee was pointed out at checkout",
      darkPatterns: "a dark pattern was flagged",
      traps: "a subscription trap was flagged",
      reviewsChecked: "reviews were checked",
      banners: "a cookie banner was answered with no",
      overlays: "an overlay was removed",
      autoplay: "autoplay was stopped",
      timers: "a fake countdown was removed",
      pastes: "a paste was checked",
      uploads: "an upload had its hidden data stripped",
    };
    const detail = message.detail && typeof message.detail === "object" ? Object.entries(message.detail).map(([key, value]) => `${key}: ${String(value).slice(0, 120)}`).join(" · ") : String(message.detail || "").slice(0, 300);
    await Shield.log({ kind, site: Shield.siteOf(Shield.hostOf(sender.url || "")), tabId: sender.tab ? sender.tab.id : null, text: (amount > 1 ? amount + "× " : "") + (words[kind] || kind), detail });
    return { ok: true };
  },
  async "lookalike.check"(message) {
    const host = String(message.host || "");
    const found = Shield.lookalike(host);
    if (!found) return { brand: null };
    const settings = await Shield.loadSettings();
    const site = Shield.siteOf(host);
    const allowed = settings.security.allowedLookalikes.includes(site) || (await onceAllowed(site, "lookalike"));
    return { ...found, site, allowed };
  },
  async "lookalike.allow"(message) {
    const site = String(message.site || "");
    const session = await api.storage.session.get("captureOnce");
    const grants = session.captureOnce || {};
    grants[site + "|lookalike"] = Date.now() + 10 * 60 * 60 * 1000;
    await api.storage.session.set({ captureOnce: grants });
    return { ok: true };
  },
  async "password.salt"() {
    return { salt: await Shield.installSalt() };
  },
  async "password.seen"(message, sender) {
    const site = Shield.siteOf(Shield.hostOf(sender.url || ""));
    return Shield.passwordSeen(String(message.hash || ""), site);
  },
  async "password.breach"(message, sender) {
    const settings = await Shield.loadSettings();
    if (settings.localOnly || !settings.security.breachCheck) return { count: 0, skipped: true };
    if (!allowRate("breach", sender, 5)) return { count: 0, skipped: true, limited: true };
    try {
      return { count: await Shield.breachCount(String(message.prefix || ""), String(message.suffix || "")) };
    } catch (error) {
      return { count: 0, error: String(error.message || error) };
    }
  },
  async "passwords.forget"() {
    await Shield.forgetPasswords();
    return { ok: true };
  },
  async "breach.email"(message) {
    const settings = await Shield.loadSettings();
    try {
      return { breaches: await Shield.breachedEmail(String(message.email || ""), settings.security.hibpKey) };
    } catch (error) {
      return { error: String(error.message || error) };
    }
  },
  async "scam.close"(message, sender) {
    if (sender.tab) await api.tabs.remove(sender.tab.id);
    return { ok: true };
  },
  async "download.info"(message) {
    const session = await api.storage.session.get("downloads");
    const entry = (session.downloads || {})[String(message.id)];
    return entry || { error: "unknown download" };
  },
  async "download.decide"(message) {
    const id = Number(message.id);
    try {
      if (message.keep) {
        await api.downloads.resume(id).catch(() => {});
      } else {
        // A small file may have finished before the hold; the file itself goes too.
        await api.downloads.cancel(id).catch(() => {});
        await api.downloads.removeFile(id).catch(() => {});
        await api.downloads.erase({ id });
      }
    } catch (error) {
      return { error: String(error.message || error) };
    }
    if (!message.keep) await Shield.count("downloads", 1);
    return { ok: true };
  },
  async "shop.compare"(message, sender) {
    if (!allowRate("shop", sender, 6)) return { error: "too many lookups", limited: true };
    const product = message.product || {};
    if (typeof product.title !== "string" || product.title.length < 4) return { error: "no product" };
    return Shield.compareProduct({ ...product, title: product.title.slice(0, 300) });
  },
  async "shop.codes"(message, sender) {
    const settings = await Shield.loadSettings();
    const site = Shield.siteOf(Shield.hostOf(sender.url || ""));
    return { site, codes: await Shield.codesFor(site, settings) };
  },
  async "shop.seen"(message, sender) {
    const site = Shield.siteOf(Shield.hostOf(sender.url || ""));
    for (const code of (Array.isArray(message.codes) ? message.codes : []).slice(0, 10)) {
      await Shield.rememberCode(site, String(code).toUpperCase(), false);
    }
    return { ok: true };
  },
  async "shop.worked"(message, sender) {
    const site = Shield.siteOf(Shield.hostOf(sender.url || ""));
    await Shield.rememberCode(site, String(message.code || "").toUpperCase(), true);
    return { ok: true };
  },
  async "shop.saved"(message) {
    await Shield.count("saved", Math.round(Number(message.amount) || 0));
    return { ok: true };
  },
  async "shop.compared"() {
    await Shield.count("compared", 1);
    return { ok: true };
  },
  async "shop.price"(message) {
    const product = message.product || {};
    if (typeof product.url !== "string") return {};
    const summary = await Shield.recordPrice(product);
    return summary || {};
  },
  async "shop.watch"(message) {
    const product = message.product || {};
    if (typeof product.url !== "string") return { error: "no product" };
    return Shield.watchProduct(product);
  },
  async "shop.unwatch"(message) {
    return Shield.unwatchProduct(String(message.url || ""));
  },
  async "shop.watchlist"() {
    const stored = await api.storage.local.get(["watchlist", "receipts", "reminders"]);
    return { watchlist: stored.watchlist || [], receipts: stored.receipts || [], reminders: stored.reminders || [], spending: Shield.spendingByStore(stored.receipts || []) };
  },
  async "shop.checkWatchlist"() {
    await Shield.checkWatchlist(announceDrop);
    return { ok: true };
  },
  async "shop.storeCheck"(message, sender) {
    const settings = await Shield.loadSettings();
    if (settings.localOnly || !allowRate("shop", sender, 6)) return { verdict: "unknown", warnings: [], days: null };
    return Shield.storeCheck(Shield.hostOf(sender.url || ""), { tooGood: Boolean(message.tooGood), http: Boolean(message.http) });
  },
  async "shop.reddit"(message, sender) {
    const settings = await Shield.loadSettings();
    if (settings.localOnly || !allowRate("shop", sender, 6)) return { threads: [], searchUrl: null };
    return Shield.redditThreads(String(message.query || "").slice(0, 120));
  },
  async "shop.remind"(message) {
    const days = Math.min(365, Math.max(1, Number(message.days) || 1));
    const entry = await Shield.addReminder({ at: Date.now() + days * 86400000, label: message.label, url: message.url, kind: message.kind });
    return { reminder: entry };
  },
  async "shop.receipt"(message) {
    return Shield.saveReceipt(message.receipt || {});
  },
  async "shop.warranty"(message) {
    const stored = await api.storage.local.get("receipts");
    const receipts = stored.receipts || [];
    const receipt = receipts.find((entry) => entry.url === message.url);
    if (!receipt) return { error: "no such receipt" };
    const months = Math.min(120, Math.max(1, Number(message.months) || 12));
    receipt.warrantyMonths = months;
    await api.storage.local.set({ receipts });
    const ends = Date.parse(receipt.at) + months * 30 * 86400000;
    await Shield.addReminder({ at: ends - 14 * 86400000, label: "Warranty ends in two weeks: " + (receipt.title || receipt.site), url: receipt.url, kind: "warranty" });
    return { ok: true };
  },
  async "shop.quiet"(message, sender) {
    const settings = await Shield.loadSettings();
    const site = Shield.siteOf(Shield.hostOf(sender.url || ""));
    if (!site) return { error: "no site" };
    await Shield.updateSettings({ shopping: { quietSites: toggleInList(settings.shopping.quietSites, site, true) } });
    return { ok: true };
  },
  async "spaces.config"() {
    const settings = await Shield.loadSettings();
    return { spaces: settings.spaces };
  },
  async "spaces.set"(message) {
    const change = { spaces: message.change || {} };
    const settings = await Shield.updateSettings(change);
    // Broadcast to every Twitter/X tab so panels appear or vanish without a reload.
    try {
      const tabs = await api.tabs.query({ url: ["https://twitter.com/*", "https://*.twitter.com/*", "https://x.com/*", "https://*.x.com/*"] });
      for (const tab of tabs) {
        try { await api.tabs.sendMessage(tab.id, { type: "spaces.set", spaces: settings.spaces }); } catch {}
      }
    } catch {}
    return { spaces: settings.spaces };
  },
  async "persona.config"() {
    const settings = await Shield.loadSettings();
    const prompt = await Shield.loadShepherdPrompt();
    return { persona: settings.persona, prompt };
  },
  async "persona.set"(message) {
    const change = { persona: message.change || {} };
    const settings = await Shield.updateSettings(change);
    const prompt = await Shield.loadShepherdPrompt();
    try {
      const tabs = await api.tabs.query({});
      for (const tab of tabs) {
        try { await api.tabs.sendMessage(tab.id, { type: "persona.set", persona: settings.persona, prompt }); } catch {}
      }
    } catch {}
    return { persona: settings.persona };
  },
  async "persona.reseed"() {
    try {
      const tabs = await api.tabs.query({ active: true, currentWindow: true });
      if (tabs && tabs[0]) await api.tabs.sendMessage(tabs[0].id, { type: "persona.reseed" });
    } catch {}
    return { ok: true };
  },
  async "profile.config"() {
    const settings = await Shield.loadSettings();
    return { profile: settings.profileIntel };
  },
  async "profile.set"(message) {
    const change = { profileIntel: message.change || {} };
    const settings = await Shield.updateSettings(change);
    try {
      const tabs = await api.tabs.query({ url: ["https://twitter.com/*", "https://*.twitter.com/*", "https://x.com/*", "https://*.x.com/*"] });
      for (const tab of tabs) {
        try { await api.tabs.sendMessage(tab.id, { type: "profile.set", profile: settings.profileIntel }); } catch {}
      }
    } catch {}
    return { profile: settings.profileIntel };
  },
  async "inspect.enabled"(message, sender) {
    const tabId = sender.tab ? sender.tab.id : (message && message.tabId);
    if (tabId == null) return { enabled: false };
    const stored = await api.storage.session.get("inspectTabs");
    const map = stored.inspectTabs || {};
    return { enabled: Boolean(map[String(tabId)]) };
  },
  async "inspect.arm.self"(message, sender) {
    const tabId = sender.tab ? sender.tab.id : null;
    if (tabId == null) return { error: "no tab" };
    const stored = await api.storage.session.get("inspectTabs");
    const map = stored.inspectTabs || {};
    map[String(tabId)] = { at: Date.now() };
    await api.storage.session.set({ inspectTabs: map });
    return { enabled: true };
  },
  async "inspect.arm"(message) {
    const tabId = Number(message && message.tabId);
    if (!Number.isFinite(tabId)) return { error: "no tabId" };
    const stored = await api.storage.session.get("inspectTabs");
    const map = stored.inspectTabs || {};
    const on = Boolean(message.on);
    if (on) map[String(tabId)] = { at: Date.now() };
    else delete map[String(tabId)];
    await api.storage.session.set({ inspectTabs: map });
    try {
      await api.tabs.sendMessage(tabId, { type: "inspect.set", enabled: on });
    } catch {}
    return { enabled: on };
  },
  async "inspect.status"(message) {
    const tabId = Number(message && message.tabId);
    const stored = await api.storage.session.get("inspectTabs");
    const map = stored.inspectTabs || {};
    return { enabled: Boolean(map[String(tabId)]) };
  },
};

async function applyModes(settings) {
  await Shield.applyFocusHours(settings).catch((error) => console.warn("shield: focus hours", error));
  await Shield.applyLowData(settings).catch((error) => console.warn("shield: low data", error));
  await Shield.applySearchSwitch(settings).catch((error) => console.warn("shield: search", error));
  await Shield.applyParental(settings).catch((error) => console.warn("shield: parental", error));
}

// Banking mode pauses the other extensions while a banking site is in front.
api.tabs.onActivated.addListener((info) => {
  (async () => {
    const settings = await Shield.loadSettings();
    if (!settings.modes.pauseExtensionsForBanking) return;
    const tab = await api.tabs.get(info.tabId).catch(() => null);
    const site = tab ? Shield.siteOf(Shield.hostOf(tab.url || "")) : "";
    const banking = site && settings.modes.siteModes[site] === "banking";
    await Shield.pauseOtherExtensions(Boolean(banking));
  })().catch((error) => console.warn("shield: banking pause", error));
});

// Redirect hops of each tab's last navigation, for the chain viewer.
if (api.webRequest && api.webRequest.onBeforeRedirect) {
  api.webRequest.onBeforeRedirect.addListener((details) => {
    if (details.type !== "main_frame" || details.tabId < 0) return;
    Shield.recordHop(details.tabId, details.url, details.redirectUrl).catch(() => {});
  }, { urls: ["<all_urls>"] });
}
if (api.webNavigation) {
  api.webNavigation.onBeforeNavigate.addListener((details) => {
    if (details.frameId === 0 && details.parentFrameId === -1) Shield.resetHops(details.tabId, details.url).catch(() => {});
  });
}

// Downloads that expire, and the alarm that removes them.
if (api.downloads && api.downloads.onChanged) {
  api.downloads.onChanged.addListener((delta) => {
    if (!delta.state || delta.state.current !== "complete") return;
    (async () => {
      const [item] = await api.downloads.search({ id: delta.id });
      if (!item) return;
      const settings = await Shield.loadSettings();
      await Shield.scheduleDownloadExpiry(item, settings);
    })().catch(() => {});
  });
}

async function locationFor(settings, trusted) {
  const city = trusted ? null : await Shield.effectiveLocation(settings);
  if (!city) return null;
  return { lat: city.lat, lon: city.lon, tz: city.tz, locale: city.locale, city: city.name, country: city.country };
}

// Stealth: the shield's own elements carry a name a page cannot look for.
async function elementTag(settings) {
  if (!settings.privacy.stealth) return "noah-shield";
  const session = await api.storage.session.get("elementTag");
  if (session.elementTag) return session.elementTag;
  const tag = "ns-" + Shield.hex(crypto.getRandomValues(new Uint8Array(4)));
  await api.storage.session.set({ elementTag: tag });
  return tag;
}

// Runs inside the page: fills the visible sign-up fields with the decoy.
function fillDecoy(identity) {
  const map = [
    [/first.?name|given/i, identity.firstName], [/last.?name|family|surname/i, identity.lastName], [/full.?name|^name$|your name/i, identity.fullName],
    [/e-?mail/i, identity.email], [/user.?name|login|handle/i, identity.username], [/pass/i, identity.password], [/phone|tel|mobile/i, identity.phone],
    [/street|address(?!.*email)/i, identity.street], [/city|town/i, identity.city], [/zip|postal/i, identity.postal], [/birth|dob/i, identity.birthday],
  ];
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  let filled = 0;
  for (const input of document.querySelectorAll("input")) {
    if (!/^(text|email|tel|password|search|url|date|)$/.test(input.type || "") || input.disabled || input.readOnly) continue;
    const box = input.getBoundingClientRect();
    if (box.width < 10 || box.height < 10) continue;
    const words = [input.name, input.id, input.placeholder, input.getAttribute("aria-label"), input.getAttribute("autocomplete"), input.labels && input.labels[0] && input.labels[0].textContent].filter(Boolean).join(" ");
    const match = map.find(([pattern]) => pattern.test(words));
    if (!match) continue;
    setter.call(input, input.type === "date" ? identity.birthday : match[1]);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
    filled++;
  }
  return filled;
}

// Panic: every tab closed, history cleared, tunnel down.
Shield.panic = panic;
async function panic() {
  const settings = await Shield.loadSettings();
  if (settings.tunnel.enabled) await Shield.disconnectTunnel();
  const tabs = await api.tabs.query({});
  await api.tabs.create({ url: "about:blank" });
  await api.tabs.remove(tabs.map((tab) => tab.id));
  if (api.browsingData) {
    await api.browsingData.remove({ since: 0 }, { history: true, downloads: true, formData: true }).catch(() => {});
  }
  await api.storage.session.clear();
  await Shield.count("panics", 1);
}

// Every request a tab makes to somebody else is noted, blocked or not.
if (api.webRequest && api.webRequest.onBeforeRequest) {
  api.webRequest.onBeforeRequest.addListener((details) => {
    if (details.tabId < 0 || details.type === "main_frame") return;
    const host = Shield.hostOf(details.url);
    const tabSite = Shield.siteOf(Shield.hostOf(details.initiator || details.documentUrl || ""));
    if (!tabSite || Shield.siteOf(host) === tabSite) return;
    Shield.noteThirdParty(details.tabId, tabSite, host, false).catch(() => {});
  }, { urls: ["<all_urls>"] });
}

api.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (change.url) Shield.rememberTabUrl(tabId, change.url).catch(() => {});
  if (change.status === "complete" && tab && tab.url) updateMeter(tabId, tab.url).catch(() => {});
});

async function updateMeter(tabId, url) {
  const settings = await Shield.loadSettings();
  if (settings.privacy.meter !== "grade" || !/^https?:/.test(url)) return;
  const score = await Shield.privacyScore(tabId, url);
  const action = api.action || api.browserAction;
  await action.setBadgeText({ tabId, text: score.grade });
  await action.setBadgeBackgroundColor({ tabId, color: score.grade === "A" ? "#2f5f2a" : score.grade === "B" ? "#4a5a2a" : score.grade === "C" ? "#6b5a1e" : "#6b2a2a" });
}

if (api.history && api.history.onVisited) {
  api.history.onVisited.addListener((item) => {
    Shield.loadSettings().then((settings) => Shield.forgetVisit(item.url, settings)).catch(() => {});
  });
}

if (api.idle && api.idle.onStateChanged) {
  api.idle.setDetectionInterval(15 * 60);
  api.idle.onStateChanged.addListener((state) => {
    if (state === "active") return;
    Shield.loadSettings().then((settings) => (settings.privacy.logoutSites.length ? Shield.idleLogout(settings) : 0)).catch(() => {});
  });
}

if (api.commands && api.commands.onCommand) {
  api.commands.onCommand.addListener((command) => {
    if (command === "panic") panic().catch((error) => console.error("shield: panic", error));
    if (command === "rotate-fingerprint") Shield.rotateFingerprint().catch(() => {});
  });
}

if (api.contextMenus) {
  api.runtime.onInstalled.addListener(() => {
    api.contextMenus.create({ id: "copy-clean-link", title: "Copy clean link (noah shield)", contexts: ["link"] });
    api.contextMenus.create({ id: "fill-decoy", title: "Fill this form with decoy details (noah shield)", contexts: ["editable"] });
    api.contextMenus.create({ id: "throwaway", title: "Open in a throwaway window (noah shield)", contexts: ["link"] });
    api.contextMenus.create({ id: "unshorten", title: "Where does this link go? (noah shield)", contexts: ["link"] });
    if (api.contextualIdentities) api.contextMenus.create({ id: "container", title: "Open in this site's own container (noah shield)", contexts: ["link", "page"] });
  });
  api.contextMenus.onClicked.addListener((info, tab) => {
    (async () => {
      if (info.menuItemId === "copy-clean-link" && tab) {
        const answer = await api.tabs.sendMessage(tab.id, { type: "links.cleanSelection", url: info.linkUrl }).catch(() => null);
        const cleaned = (answer && answer.cleaned) || info.linkUrl;
        await api.scripting.executeScript({ target: { tabId: tab.id }, func: (text) => navigator.clipboard.writeText(text), args: [cleaned] });
      }
      if (info.menuItemId === "fill-decoy" && tab) {
        await handlers["decoy.fill"]({ tabId: tab.id, site: Shield.siteOf(Shield.hostOf(tab.url || "")) });
      }
      if (info.menuItemId === "throwaway") await handlers["tools.throwaway"]({ url: info.linkUrl });
      if (info.menuItemId === "unshorten") await api.tabs.create({ url: api.runtime.getURL("tools.html") + "?unshorten=" + encodeURIComponent(info.linkUrl) });
      if (info.menuItemId === "container") await handlers["container.open"]({ url: info.linkUrl || info.pageUrl });
    })().catch((error) => console.warn("shield: menu", error));
  });
}

if (api.storage.sync && api.storage.onChanged) {
  api.storage.onChanged.addListener((changes, area) => {
    if (area !== "sync" || !changes.meta || !changes.meta.newValue) return;
    if (changes.meta.newValue.from === api.runtime.id) return;
    handlers["sync.pull"]().catch(() => {});
  });
}

// A page whose host imitates a brand is replaced by the warning before it
// renders; the content script covers the moment in between.
if (api.webNavigation) {
  api.webNavigation.onBeforeNavigate.addListener((details) => {
    if (details.frameId !== 0 || !/^https?:/.test(details.url)) return;
    (async () => {
      const found = Shield.lookalike(Shield.hostOf(details.url));
      if (!found) return;
      const settings = await Shield.loadSettings();
      const site = Shield.siteOf(Shield.hostOf(details.url));
      if (!settings.security.lookalike || settings.security.allowedLookalikes.includes(site) || (await onceAllowed(site, "lookalike"))) return;
      const page = api.runtime.getURL("warn.html") + "?" + new URLSearchParams({ kind: "lookalike", url: details.url, brand: found.brand, real: found.real, reason: found.reason });
      await api.tabs.update(details.tabId, { url: page });
      await Shield.count("phishing", 1, details.tabId);
      await Shield.log({ kind: "phishing", site, tabId: details.tabId, text: `stopped before ${site}: it looks like ${found.brand} but is not`, detail: `${found.reason}; the real site is ${found.real}` });
    })().catch((error) => console.warn("shield: navigation guard", error));
  });
}

// Risky downloads pause until you have seen why they are risky.
if (api.downloads && api.downloads.onCreated) {
  api.downloads.onCreated.addListener((item) => {
    (async () => {
      const settings = await Shield.loadSettings();
      if (!settings.security.downloadWarn) return;
      const concern = Shield.downloadConcern(item, settings);
      if (!concern) return;
      try {
        await api.downloads.pause(item.id);
      } catch {
        // Already finished: nothing to hold, but the notice still helps.
      }
      const entry = { id: item.id, name: concern.name, url: item.url, source: concern.source, reasons: concern.reasons, virusTotal: null };
      const session = await api.storage.session.get("downloads");
      const downloads = session.downloads || {};
      downloads[String(item.id)] = entry;
      await api.storage.session.set({ downloads });
      await api.tabs.create({ url: api.runtime.getURL("warn.html") + "?kind=download&id=" + item.id });
      if (settings.security.virusTotalKey && !settings.localOnly) {
        try {
          entry.virusTotal = await Shield.virusTotalUrl(item.url, settings.security.virusTotalKey);
        } catch (error) {
          entry.virusTotal = { error: String(error.message || error) };
        }
        const again = await api.storage.session.get("downloads");
        const latest = again.downloads || {};
        latest[String(item.id)] = entry;
        await api.storage.session.set({ downloads: latest });
      }
    })().catch((error) => console.warn("shield: download guard", error));
  });
}

// The handlers by name, for the shield's own pages and tests running inside the worker.
globalThis.handlersProxy = (type, message = {}) => handlers[type]({ ...message, type }, { url: api.runtime.getURL("background.js") });

function sameSiteAsSender(url, sender) {
  const senderSite = Shield.siteOf(Shield.hostOf(sender.url || ""));
  return Boolean(senderSite) && Shield.siteOf(Shield.hostOf(String(url || ""))) === senderSite;
}

// What a page's content script may say, and about what: its own tab, its own
// site, its own addresses. Nothing it sends can reach into another tab.
function boundToSender(message, sender) {
  const site = Shield.siteOf(Shield.hostOf(sender.url || ""));
  switch (message.type) {
    case "site.score":
      return sender.tab ? { ...message, tabId: sender.tab.id, url: sender.tab.url } : null;
    case "lookalike.allow":
      return message.site === site ? message : null;
    case "shop.price":
    case "shop.watch":
      return message.product && sameSiteAsSender(message.product.url, sender) ? message : null;
    case "shop.unwatch":
    case "shop.remind":
      return sameSiteAsSender(message.url, sender) ? message : null;
    case "shop.receipt":
      return message.receipt && sameSiteAsSender(message.receipt.url, sender) ? { ...message, receipt: { ...message.receipt, site } } : null;
    default:
      return message;
  }
}

// Capture questions this worker put to a tab, awaiting the person's answer.
const pendingCaptureAsks = new Map();
function captureAskKey(sender, kind) {
  return (sender.tab ? sender.tab.id : "none") + "|" + (sender.frameId ?? 0) + "|" + kind;
}

api.runtime.onMessage.addListener((message, sender, sendResponse) => {
  // Only this extension's own scripts speak here; nothing is exposed to
  // other extensions, so a message from one is refused before it is read.
  if (!sender || sender.id !== api.runtime.id) {
    sendResponse({ error: "not from here" });
    return false;
  }
  const handler = message && handlers[message.type];
  if (!handler) {
    sendResponse({ error: "unknown message" });
    return false;
  }
  // Content scripts of a page speak only for that page; the popup and options
  // pages carry the extension's own origin.
  const fromPage = !(sender.url && sender.url.startsWith(api.runtime.getURL("")));
  const pageAllowed = new Set(["guard.config", "site.score", "media.state", "lock.check", "shop.price", "shop.watch", "shop.unwatch", "shop.storeCheck", "shop.reddit", "shop.remind", "shop.receipt", "capture.ask", "capture.decide", "capture.shot", "capture.save", "light.state", "search.state", "search.hidden", "search.peek", "log.open", "safety.event", "lookalike.check", "lookalike.allow", "password.salt", "password.seen", "password.breach", "scam.close", "shop.compare", "shop.codes", "shop.seen", "shop.worked", "shop.saved", "shop.compared", "shop.quiet", "inspect.enabled", "inspect.arm.self", "spaces.config", "profile.config", "persona.config"]);
  if (fromPage && !pageAllowed.has(message.type)) {
    sendResponse({ error: "not from here" });
    return false;
  }
  if (fromPage) {
    message = boundToSender(message, sender);
    if (!message) {
      sendResponse({ error: "not about this page" });
      return false;
    }
  }
  handler(message, sender)
    .then((result) => sendResponse(result === undefined ? { ok: true } : result))
    .catch((error) => sendResponse({ error: String(error && error.message ? error.message : error) }));
  return true;
});

applyAll("wake").catch((error) => console.error("shield: wake", error));
