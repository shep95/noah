// Per-site state: who a page talked to, the privacy score, burning a site,
// cookies that go when the last tab closes, history that forgets itself,
// idle sign-outs, the fingerprint seeds, the blend-in headers, encrypted
// settings sync, and the small generators (aliases, decoy data).
"use strict";
(() => {
const Shield = (globalThis.Shield = globalThis.Shield || {});

// ---- what each tab talked to ------------------------------------------------

// Session storage has no transactions; the writers queue behind each other
// so two requests arriving together do not overwrite each other's note.
let pendingNotes = [];
let flushTimer = null;
let flushing = Promise.resolve();
Shield.noteThirdParty = function noteThirdParty(tabId, tabSite, host, blocked) {
  if (tabId < 0 || !host || pendingNotes.length > 5000) return flushing;
  pendingNotes.push({ tabId, tabSite, host, blocked });
  if (!flushTimer) flushTimer = setTimeout(() => { flushTimer = null; flushing = flushing.then(flushNotes).catch(() => {}); }, 500);
  return flushing;
};
async function flushNotes() {
  const notes = pendingNotes;
  pendingNotes = [];
  if (!notes.length) return;
  const session = await Shield.api.storage.session.get("tabHosts");
  const tabHosts = session.tabHosts || {};
  for (const { tabId, tabSite, host, blocked } of notes) {
    const entry = tabHosts[tabId] || { site: tabSite, hosts: {} };
    if (entry.site !== tabSite) {
      entry.site = tabSite;
      entry.hosts = {};
    }
    if (!entry.hosts[host] && Object.keys(entry.hosts).length >= 400) continue;
    const record = entry.hosts[host] || { count: 0, blocked: 0 };
    record.count++;
    if (blocked) record.blocked++;
    entry.hosts[host] = record;
    tabHosts[tabId] = entry;
  }
  await Shield.api.storage.session.set({ tabHosts });
}
Shield.flushNotes = () => { if (flushTimer) { clearTimeout(flushTimer); flushTimer = null; } flushing = flushing.then(flushNotes).catch(() => {}); return flushing; };

Shield.tabHosts = async function tabHosts(tabId) {
  await Shield.flushNotes();
  const session = await Shield.api.storage.session.get("tabHosts");
  const entry = (session.tabHosts || {})[tabId];
  return entry ? entry.hosts : {};
};

Shield.forgetTab = async function forgetTab(tabId) {
  const session = await Shield.api.storage.session.get(["tabHosts", "tabUrls"]);
  const tabHosts = session.tabHosts || {};
  const tabUrls = session.tabUrls || {};
  delete tabHosts[tabId];
  delete tabUrls[tabId];
  await Shield.api.storage.session.set({ tabHosts, tabUrls });
};

// Who is watching: every third-party host of the tab, with its owner.
Shield.watchers = async function watchers(tabId, site) {
  const hosts = await Shield.tabHosts(tabId);
  const list = Object.entries(hosts)
    .filter(([host]) => Shield.siteOf(host) !== site)
    .map(([host, record]) => {
      const owner = Shield.ownerOf(host);
      return { host, count: record.count, blocked: record.blocked, owner: owner ? owner.owner : Shield.siteOf(host), kind: owner ? owner.kind : "unknown", country: owner ? owner.country : "" };
    })
    .sort((left, right) => right.blocked - left.blocked || right.count - left.count);
  return list;
};

// 100 minus what the page does to you: trackers it tried, third parties it
// talked to, plain http, a pile of cookies. A letter for the toolbar.
Shield.privacyScore = async function privacyScore(tabId, url) {
  const site = Shield.siteOf(Shield.hostOf(url));
  const watchers = await Shield.watchers(tabId, site);
  const blocked = watchers.reduce((sum, watcher) => sum + watcher.blocked, 0);
  const advertising = watchers.filter((watcher) => /advertising|session replay|identity|attribution/.test(watcher.kind)).length;
  let cookies = 0;
  try {
    cookies = (await Shield.api.cookies.getAll({ domain: site })).length;
  } catch {
    cookies = 0;
  }
  let score = 100;
  score -= Math.min(30, blocked * 3);
  score -= Math.min(20, watchers.length * 2);
  score -= Math.min(20, advertising * 5);
  if (/^http:/.test(url || "")) score -= 15;
  if (cookies > 20) score -= 5;
  score = Math.max(0, score);
  const grade = score >= 90 ? "A" : score >= 75 ? "B" : score >= 55 ? "C" : score >= 35 ? "D" : "F";
  return { score, grade, blocked, thirdParties: watchers.length, advertising, cookies, https: /^https:/.test(url || "") };
};

// ---- burning ----------------------------------------------------------------

async function removeCookiesFor(site) {
  if (!Shield.api.cookies) return 0;
  let removed = 0;
  const cookies = await Shield.api.cookies.getAll({});
  for (const cookie of cookies) {
    const domain = cookie.domain.replace(/^\./, "");
    if (Shield.siteOf(domain) !== site) continue;
    const url = (cookie.secure ? "https://" : "http://") + domain + cookie.path;
    try {
      await Shield.api.cookies.remove({ url, name: cookie.name, storeId: cookie.storeId });
      removed++;
    } catch {
      // Gone already.
    }
  }
  return removed;
}

// Everything a site keeps in this browser: cookies, cache, storage, workers.
Shield.burnSite = async function burnSite(site, extraOrigins = []) {
  if (!site) return { removed: 0 };
  const removed = await removeCookiesFor(site);
  const data = Shield.api.browsingData;
  if (data && data.remove) {
    // Origins must match exactly, port included, so every open tab of the
    // site contributes its own.
    const origins = new Set([`https://${site}`, `https://www.${site}`, `http://${site}`, `http://www.${site}`, ...extraOrigins]);
    try {
      for (const tab of await Shield.api.tabs.query({})) {
        const url = new URL(tab.url || "about:blank");
        if (/^https?:$/.test(url.protocol) && Shield.siteOf(url.hostname) === site) origins.add(url.origin);
      }
    } catch {
      // Tabs are a convenience here, not a requirement.
    }
    try {
      if (Shield.isFirefox) {
        await data.remove({ hostnames: [site, "www." + site] }, { cookies: true, localStorage: true, indexedDB: true, serviceWorkers: true, cache: true });
      } else {
        await data.remove({ origins: Array.from(origins) }, { cacheStorage: true, fileSystems: true, indexedDB: true, localStorage: true, serviceWorkers: true, webSQL: true });
        await data.remove({ origins: Array.from(origins) }, { cache: true }).catch(() => {});
      }
    } catch (error) {
      console.warn("shield: burn", error);
    }
  }
  await Shield.count("burned", 1);
  return { removed };
};

Shield.rememberTabUrl = async function rememberTabUrl(tabId, url) {
  const session = await Shield.api.storage.session.get("tabUrls");
  const tabUrls = session.tabUrls || {};
  tabUrls[tabId] = url;
  await Shield.api.storage.session.set({ tabUrls });
};

// When a site's last tab closes, its cookies go too, unless you keep it.
Shield.tabClosed = async function tabClosed(tabId, settings) {
  const session = await Shield.api.storage.session.get("tabUrls");
  const tabUrls = session.tabUrls || {};
  const url = tabUrls[tabId];
  await Shield.forgetTab(tabId);
  if (!url || !settings.privacy.burnOnClose) return;
  const site = Shield.siteOf(Shield.hostOf(url));
  if (!site || settings.privacy.keepSites.includes(site) || settings.privacy.trustedSites.includes(site)) return;
  const open = await Shield.api.tabs.query({});
  if (open.some((tab) => tab.id !== tabId && Shield.siteOf(Shield.hostOf(tab.url || "")) === site)) return;
  const removed = await removeCookiesFor(site);
  if (removed) await Shield.count("cookies", removed);
};

// ---- history and idle -------------------------------------------------------

Shield.forgetVisit = async function forgetVisit(url, settings) {
  const site = Shield.siteOf(Shield.hostOf(url));
  if (!site || !settings.privacy.forgetHistorySites.includes(site) || !Shield.api.history) return;
  try {
    await Shield.api.history.deleteUrl({ url });
  } catch (error) {
    console.warn("shield: history", error);
  }
};

Shield.idleLogout = async function idleLogout(settings) {
  let removed = 0;
  for (const site of settings.privacy.logoutSites) removed += await removeCookiesFor(site);
  if (removed) await Shield.count("cookies", removed);
  return removed;
};

// ---- permission history -------------------------------------------------------

Shield.logPermission = async function logPermission(site, kind, decision) {
  const stored = await Shield.api.storage.local.get("permissionLog");
  const log = stored.permissionLog || [];
  log.unshift({ at: new Date().toISOString(), site, kind, decision });
  await Shield.api.storage.local.set({ permissionLog: log.slice(0, 300) });
};

// ---- fingerprint seeds --------------------------------------------------------

function fnv1a(text) {
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index++) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

async function sessionKey() {
  const session = await Shield.api.storage.session.get("fingerprintKey");
  if (session.fingerprintKey) return session.fingerprintKey;
  const key = Shield.hex(crypto.getRandomValues(new Uint8Array(16)));
  await Shield.api.storage.session.set({ fingerprintKey: key });
  return key;
}

Shield.rotateFingerprint = async function rotateFingerprint() {
  const key = Shield.hex(crypto.getRandomValues(new Uint8Array(16)));
  await Shield.api.storage.session.set({ fingerprintKey: key });
  return key;
};

// Different per site, and again after the rotation period; the same within
// one so a site does not see a visitor whose hardware changes every click.
Shield.fingerprintSeed = async function fingerprintSeed(site, settings) {
  const key = await sessionKey();
  const now = new Date();
  const period = settings.privacy.rotate === "hourly" ? now.toISOString().slice(0, 13) : settings.privacy.rotate === "daily" ? now.toISOString().slice(0, 10) : "";
  const scope = settings.privacy.perSite ? site : "";
  return fnv1a(key + "|" + period + "|" + scope);
};

// ---- blend-in headers and language -------------------------------------------------

const BLEND_RULE_ID = 5000;
const LANGUAGE_RULE_ID = 5001;
const ALL_TYPES = ["main_frame", "sub_frame", "script", "image", "stylesheet", "object", "xmlhttprequest", "ping", "websocket", "media", "font", "other"];

Shield.blendUserAgent = function blendUserAgent() {
  const real = typeof navigator !== "undefined" ? navigator.userAgent : "";
  const major = (/Chrome\/(\d+)/.exec(real) || [])[1] || "141";
  return `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${major}.0.0.0 Safari/537.36`;
};

Shield.applyHeaderRules = async function applyHeaderRules(settings) {
  const rules = Shield.api.declarativeNetRequest;
  if (!rules || !rules.updateSessionRules) return;
  const addRules = [];
  if (settings.privacy.blendIn) {
    addRules.push({
      id: BLEND_RULE_ID,
      priority: 5,
      action: {
        type: "modifyHeaders",
        requestHeaders: [
          { header: "User-Agent", operation: "set", value: Shield.blendUserAgent() },
          { header: "sec-ch-ua-platform", operation: "set", value: '"Windows"' },
          { header: "sec-ch-ua-platform-version", operation: "set", value: '"15.0.0"' },
          { header: "sec-ch-ua-arch", operation: "set", value: '"x86"' },
        ],
      },
      condition: { urlFilter: "*", resourceTypes: ALL_TYPES },
    });
  }
  const location = await Shield.effectiveLocation(settings);
  if (location) {
    addRules.push({
      id: LANGUAGE_RULE_ID,
      priority: 5,
      action: { type: "modifyHeaders", requestHeaders: [{ header: "Accept-Language", operation: "set", value: `${location.locale},${location.locale.split("-")[0]};q=0.9,en;q=0.7` }] },
      condition: { urlFilter: "*", resourceTypes: ALL_TYPES },
    });
  }
  try {
    await rules.updateSessionRules({ removeRuleIds: [BLEND_RULE_ID, LANGUAGE_RULE_ID], addRules });
  } catch (error) {
    console.warn("shield: header rules", error);
  }
};

// The city every site should think you are in, or null for the truth.
Shield.effectiveLocation = async function effectiveLocation(settings) {
  const mode = settings.privacy.location.mode;
  if (mode === "fake") return Shield.cityById(settings.privacy.location.city) || Shield.CITIES[0];
  if (mode === "tunnel") {
    const status = await Shield.tunnelStatus();
    const country = (status.exit && status.exit.country) || (status.server && status.server.country) || "";
    return country ? Shield.cityForCountry(country) : null;
  }
  return null;
};

// ---- encrypted sync through the browser's own sync storage -------------------------------

async function syncKey(passphrase, salt) {
  const material = await crypto.subtle.importKey("raw", new TextEncoder().encode(passphrase), "PBKDF2", false, ["deriveKey"]);
  return crypto.subtle.deriveKey({ name: "PBKDF2", salt, iterations: 310000, hash: "SHA-256" }, material, { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
}

Shield.pushSync = async function pushSync(settings) {
  const stored = await Shield.api.storage.local.get("syncPassphrase");
  if (!settings.privacy.sync || !stored.syncPassphrase || !Shield.api.storage.sync) return;
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await syncKey(stored.syncPassphrase, salt);
  const plain = new TextEncoder().encode(JSON.stringify({ settings, at: Date.now() }));
  const cipher = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, plain));
  const text = btoa(String.fromCharCode(...cipher));
  const chunks = {};
  const size = 7000;
  for (let index = 0; index * size < text.length; index++) chunks["blob" + index] = text.slice(index * size, (index + 1) * size);
  await Shield.api.storage.sync.clear();
  await Shield.api.storage.sync.set({ meta: { salt: Shield.hex(salt), iv: Shield.hex(iv), chunks: Object.keys(chunks).length, at: Date.now(), from: Shield.api.runtime.id }, ...chunks });
};

Shield.pullSync = async function pullSync() {
  const stored = await Shield.api.storage.local.get("syncPassphrase");
  if (!stored.syncPassphrase || !Shield.api.storage.sync) return null;
  const all = await Shield.api.storage.sync.get(null);
  if (!all.meta) return null;
  let text = "";
  for (let index = 0; index < all.meta.chunks; index++) text += all["blob" + index] || "";
  const bytes = Shield.fromBase64(text);
  const key = await syncKey(stored.syncPassphrase, Shield.fromHex(all.meta.salt));
  const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: Shield.fromHex(all.meta.iv) }, key, bytes);
  return JSON.parse(new TextDecoder().decode(plain));
};

// ---- generators --------------------------------------------------------------------------

const FIRST = ["Alex", "Sam", "Jordan", "Taylor", "Morgan", "Casey", "Riley", "Avery", "Quinn", "Rowan", "Emery", "Harper", "Reese", "Skyler", "Dakota", "Finley"];
const LAST = ["Miller", "Carter", "Hayes", "Brooks", "Reed", "Bennett", "Cooper", "Ward", "Foster", "Hughes", "Price", "Bell", "Cole", "Fox", "Gray", "Lane"];
const STREETS = ["Oak", "Maple", "Cedar", "Pine", "Elm", "Birch", "Willow", "Ash", "Spruce", "Chestnut"];

function pick(list) {
  return list[crypto.getRandomValues(new Uint32Array(1))[0] % list.length];
}
function digits(count) {
  return Array.from(crypto.getRandomValues(new Uint8Array(count)), (byte) => String(byte % 10)).join("");
}

// A plus-address at your own mailbox, unique per site, so a leak tells you who leaked.
Shield.makeAlias = function makeAlias(base, site) {
  const at = base.indexOf("@");
  if (at < 1) return null;
  const tag = (site || "site").replace(/[^a-z0-9]/gi, "").slice(0, 16) + "-" + digits(4);
  return base.slice(0, at) + "+" + tag + base.slice(at);
};

Shield.simpleLoginAlias = async function simpleLoginAlias(key, note) {
  const response = await fetch("https://app.simplelogin.io/api/alias/random/new?mode=word", {
    method: "POST",
    headers: { Authentication: key, "content-type": "application/json" },
    body: JSON.stringify({ note: note || "made by noah shield" }),
    credentials: "omit",
  });
  if (!response.ok) throw new Error("SimpleLogin answered " + response.status);
  const alias = await response.json();
  return alias.alias || alias.email;
};

// Plausible but fictional details for forms that demand them.
Shield.decoyIdentity = function decoyIdentity(emailBase, site) {
  const first = pick(FIRST);
  const last = pick(LAST);
  const email = emailBase ? Shield.makeAlias(emailBase, site) : `${first}.${last}${digits(3)}@example.com`.toLowerCase();
  const year = 1975 + (crypto.getRandomValues(new Uint8Array(1))[0] % 25);
  return {
    firstName: first,
    lastName: last,
    fullName: first + " " + last,
    email,
    username: (first + last + digits(3)).toLowerCase(),
    password: Shield.hex(crypto.getRandomValues(new Uint8Array(9))) + "!A",
    phone: "555-01" + digits(2) + "-" + digits(4),
    street: (100 + (crypto.getRandomValues(new Uint8Array(1))[0] % 800)) + " " + pick(STREETS) + " Street",
    city: "Springfield",
    postal: "0" + digits(4),
    birthday: `${year}-${String(1 + (crypto.getRandomValues(new Uint8Array(1))[0] % 12)).padStart(2, "0")}-${String(1 + (crypto.getRandomValues(new Uint8Array(1))[0] % 28)).padStart(2, "0")}`,
  };
};
})();
