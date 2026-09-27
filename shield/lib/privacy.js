// Data protection: what pages may learn about you and what they may keep.
// Trackers are stopped by the declarativeNetRequest rulesets (EasyPrivacy,
// rebuilt by script/build-shield-rules); this file drives the browser's own
// privacy switches, the per-site rules, and the cookie clean-up.
"use strict";
(() => {

const Shield = (globalThis.Shield = globalThis.Shield || {});

// Dynamic rule ids: 1..999 are per-site trust rules, 1000..1999 strict-cookie rules.
const TRUST_RULE_BASE = 1;
const STRICT_RULE_BASE = 1000;
const ALL_TYPES = [
  "main_frame", "sub_frame", "script", "image", "stylesheet", "object",
  "xmlhttprequest", "ping", "websocket", "media", "font", "other",
];

async function setContentSetting(name, value) {
  const settings = Shield.api.contentSettings;
  if (!settings || !settings[name]) return;
  try {
    await settings[name].clear({});
    if (value === "block") {
      await settings[name].set({ primaryPattern: "<all_urls>", setting: "block" });
    }
  } catch (error) {
    console.warn("shield: content setting", name, error);
  }
}

async function setPrivacy(group, name, value) {
  const privacy = Shield.api.privacy;
  const setting = privacy && privacy[group] && privacy[group][name];
  if (!setting) return;
  try {
    if (value === null) await setting.clear({});
    else await setting.set({ value });
  } catch (error) {
    console.warn("shield: privacy setting", name, error);
  }
}

// Applies the switches the browser exposes. Firefox has none of the content
// settings, so there the content script does the same work in the page.
Shield.applyPrivacy = async function applyPrivacy(settings) {
  const privacy = settings.privacy;
  await setContentSetting("location", privacy.geolocation);
  await setContentSetting("notifications", privacy.notifications);
  await setContentSetting("camera", privacy.camera);
  await setContentSetting("microphone", privacy.microphone);
  await setPrivacy("websites", "thirdPartyCookiesAllowed", privacy.thirdPartyCookies ? null : false);
  await setPrivacy("websites", "hyperlinkAuditingEnabled", false);
  await setPrivacy("websites", "topicsEnabled", privacy.adTopics ? null : false);
  await setPrivacy("websites", "adMeasurementEnabled", privacy.adTopics ? null : false);
  await setPrivacy("websites", "fledgeEnabled", privacy.adTopics ? null : false);
  await setPrivacy("websites", "relatedWebsiteSetsEnabled", privacy.adTopics ? null : false);
  await setPrivacy("network", "networkPredictionEnabled", privacy.prefetch ? null : false);
  await setPrivacy("services", "safeBrowsingExtendedReportingEnabled", false);

  const rulesets = Shield.api.declarativeNetRequest;
  if (rulesets && rulesets.updateEnabledRulesets) {
    const enable = [];
    const disable = [];
    (privacy.trackers ? enable : disable).push("trackers", "ads");
    (privacy.gpc ? enable : disable).push("headers");
    (privacy.stripParameters ? enable : disable).push("parameters");
    const security = settings.security;
    (security.httpsUpgrade || privacy.trackers ? enable : disable).push("security");
    (security.mailPixels ? enable : disable).push("mail");
    (security.referrer ? enable : disable).push("referrer");
    (security.socialLogin ? enable : disable).push("sociallogin");
    try {
      await rulesets.updateEnabledRulesets({ enableRulesetIds: enable, disableRulesetIds: disable });
    } catch (error) {
      console.warn("shield: rulesets", error);
    }
    await enableMoreAdRules(rulesets, privacy.trackers);
  }
  await Shield.applySiteRules(settings);
};

// The second ad ruleset goes beyond the browser's guaranteed rule count; it is
// turned on when the shared pool has room and left off, with a line in the
// log, when it has not.
async function enableMoreAdRules(rulesets, wanted) {
  const enabled = await rulesets.getEnabledRulesets();
  const already = enabled.includes("ads_more");
  if (!wanted) {
    if (already) await rulesets.updateEnabledRulesets({ disableRulesetIds: ["ads_more"] }).catch(() => {});
    return;
  }
  if (already) return;
  const session = await Shield.api.storage.session.get("adsMoreTried");
  if (session.adsMoreTried) return;
  await Shield.api.storage.session.set({ adsMoreTried: true });
  try {
    await rulesets.updateEnabledRulesets({ enableRulesetIds: ["ads_more"] });
    const left = rulesets.getAvailableStaticRuleCount ? await rulesets.getAvailableStaticRuleCount() : null;
    if (Shield.log) await Shield.log({ kind: "rules", text: "the second set of ad rules is on: the browser had room for it", detail: left === null ? "" : `${left} static rules of room left` });
  } catch (error) {
    if (Shield.log) await Shield.log({ kind: "rules", text: "the second set of ad rules stays off: the browser has no room for it beyond the first 30,000", detail: String(error && error.message ? error.message : error) });
  }
}

// Trusted sites: every request a page of theirs makes is let through, which
// is how a site that breaks without its analytics gets fixed by you, not us.
// Strict-cookie sites: their pages send and receive no cookies at all.
Shield.applySiteRules = function applySiteRules(settings) {
  return Shield.withRuleLock(() => applySiteRulesNow(settings));
};
async function applySiteRulesNow(settings) {
  const rulesets = Shield.api.declarativeNetRequest;
  if (!rulesets || !rulesets.updateDynamicRules) return;
  const existing = await rulesets.getDynamicRules();
  const removeRuleIds = existing.map((rule) => rule.id);
  const addRules = [];
  settings.privacy.trustedSites.slice(0, 900).forEach((site, index) => {
    addRules.push({
      id: TRUST_RULE_BASE + index,
      priority: 10,
      action: { type: "allowAllRequests" },
      condition: { requestDomains: [site], resourceTypes: ["main_frame", "sub_frame"] },
    });
  });
  settings.privacy.strictCookieSites.slice(0, 900).forEach((site, index) => {
    addRules.push({
      id: STRICT_RULE_BASE + index,
      priority: 10,
      action: {
        type: "modifyHeaders",
        requestHeaders: [{ header: "cookie", operation: "remove" }],
        responseHeaders: [{ header: "set-cookie", operation: "remove" }],
      },
      condition: { initiatorDomains: [site], resourceTypes: ALL_TYPES },
    });
    addRules.push({
      id: STRICT_RULE_BASE + 450 + index,
      priority: 10,
      action: {
        type: "modifyHeaders",
        requestHeaders: [{ header: "cookie", operation: "remove" }],
        responseHeaders: [{ header: "set-cookie", operation: "remove" }],
      },
      condition: { requestDomains: [site], resourceTypes: ["main_frame"] },
    });
  });
  try {
    await rulesets.updateDynamicRules({ removeRuleIds, addRules });
  } catch (error) {
    console.warn("shield: dynamic rules", error);
  }
}

let trackerDomainsPromise = null;
function trackerDomains() {
  if (!trackerDomainsPromise) {
    trackerDomainsPromise = fetch(Shield.api.runtime.getURL("rules/trackers.json"))
      .then((response) => response.json())
      .then((rules) => {
        const domains = new Set();
        for (const rule of rules) {
          if (rule.action.type !== "block") continue;
          const match = /^\|\|([a-z0-9.-]+)\^$/.exec(rule.condition.urlFilter || "");
          if (match && !rule.condition.initiatorDomains) domains.add(match[1]);
        }
        return domains;
      })
      .catch(() => new Set());
  }
  return trackerDomainsPromise;
}

function domainMatches(cookieDomain, trackers) {
  let host = cookieDomain.replace(/^\./, "");
  while (host) {
    if (trackers.has(host)) return true;
    const dot = host.indexOf(".");
    if (dot < 0) return false;
    host = host.slice(dot + 1);
  }
  return false;
}

// Cookies from tracking domains are deleted; the trackers cannot reach the
// page any more, but a cookie they left earlier would still identify you.
Shield.purgeTrackerCookies = async function purgeTrackerCookies(settings) {
  if (!settings.privacy.purgeTrackerCookies || !Shield.api.cookies) return 0;
  const trackers = await trackerDomains();
  const trusted = new Set(settings.privacy.trustedSites);
  let removed = 0;
  let cookies = [];
  try {
    cookies = await Shield.api.cookies.getAll({});
  } catch {
    return 0;
  }
  for (const cookie of cookies) {
    if (!domainMatches(cookie.domain, trackers)) continue;
    if (trusted.has(Shield.siteOf(cookie.domain.replace(/^\./, "")))) continue;
    const url = (cookie.secure ? "https://" : "http://") + cookie.domain.replace(/^\./, "") + cookie.path;
    try {
      await Shield.api.cookies.remove({ url, name: cookie.name, storeId: cookie.storeId });
      removed++;
    } catch {
      // A cookie that vanished between the listing and the removal is fine.
    }
  }
  if (removed) await Shield.count("cookies", removed);
  return removed;
};

// Counts of what was stopped, per day, for the popup. Nothing leaves the browser.
let countQueue = Promise.resolve();
Shield.count = function count(kind, amount = 1, tabId = null) {
  countQueue = countQueue.then(() => countNow(kind, amount, tabId)).catch(() => {});
  return countQueue;
};
async function countNow(kind, amount, tabId) {
  const stored = await Shield.api.storage.local.get("stats");
  const stats = stored.stats || {};
  const today = Shield.todayKey();
  stats[today] = stats[today] || {};
  stats[today][kind] = (stats[today][kind] || 0) + amount;
  for (const day of Object.keys(stats)) {
    if (Date.parse(day) < Date.now() - 31 * 86400000) delete stats[day];
  }
  await Shield.api.storage.local.set({ stats });
  if (tabId !== null && tabId >= 0) {
    const session = await Shield.api.storage.session.get("tabCounts");
    const tabCounts = session.tabCounts || {};
    tabCounts[tabId] = (tabCounts[tabId] || 0) + amount;
    await Shield.api.storage.session.set({ tabCounts });
    Shield.showBadge(tabId, tabCounts[tabId]);
  }
}

// The log: everything the shield did or saw, in plain words, with the site
// and the moment, so a notice that flashed by can be read again. A ring of
// the newest entries, on this device only.
const LOG_LIMIT = 3000;
let logQueue = Promise.resolve();
Shield.log = function log(entry) {
  const record = {
    at: Date.now(),
    kind: String(entry.kind || "note").slice(0, 32),
    site: String(entry.site || "").slice(0, 120),
    text: String(entry.text || "").slice(0, 400),
    detail: entry.detail === undefined ? "" : String(entry.detail).slice(0, 1200),
    tabId: Number.isInteger(entry.tabId) ? entry.tabId : null,
  };
  logQueue = logQueue.then(async () => {
    const stored = await Shield.api.storage.local.get("log");
    const entries = Array.isArray(stored.log) ? stored.log : [];
    entries.push(record);
    if (entries.length > LOG_LIMIT) entries.splice(0, entries.length - LOG_LIMIT);
    await Shield.api.storage.local.set({ log: entries });
  }).catch(() => {});
  return logQueue;
};

Shield.logEntries = async function logEntries({ kind = "", site = "", tabId = null, limit = 500 } = {}) {
  const stored = await Shield.api.storage.local.get("log");
  const entries = Array.isArray(stored.log) ? stored.log : [];
  const picked = entries.filter((entry) =>
    (!kind || entry.kind === kind) && (!site || entry.site === site) && (tabId === null || entry.tabId === tabId),
  );
  return picked.slice(-limit).reverse();
};

Shield.clearLog = function clearLog() {
  return Shield.api.storage.local.set({ log: [] });
};

// Requests stopped on a tab, kept for the popup's "on this page" list until
// the tab goes away or loads something else.
Shield.noteBlocked = function noteBlocked(tabId, url, type, ruleset) {
  logQueue = logQueue.then(async () => {
    const session = await Shield.api.storage.session.get("blockedByTab");
    const blockedByTab = session.blockedByTab || {};
    const list = blockedByTab[tabId] || [];
    list.push({ host: Shield.hostOf(url), path: url.replace(/^[a-z]+:\/\/[^/]+/, "").slice(0, 80), type, ruleset: ruleset || "", at: Date.now() });
    if (list.length > 400) list.splice(0, list.length - 400);
    blockedByTab[tabId] = list;
    await Shield.api.storage.session.set({ blockedByTab });
  }).catch(() => {});
  return logQueue;
};

Shield.blockedOnTab = async function blockedOnTab(tabId) {
  const session = await Shield.api.storage.session.get("blockedByTab");
  return ((session.blockedByTab || {})[tabId]) || [];
};

Shield.forgetBlocked = async function forgetBlocked(tabId) {
  const session = await Shield.api.storage.session.get("blockedByTab");
  const blockedByTab = session.blockedByTab || {};
  if (!blockedByTab[tabId]) return;
  delete blockedByTab[tabId];
  await Shield.api.storage.session.set({ blockedByTab });
};

Shield.showBadge = function showBadge(tabId, value) {
  const action = Shield.api.action || Shield.api.browserAction;
  if (!action) return;
  const text = value > 999 ? "999+" : value ? String(value) : "";
  action.setBadgeText({ tabId, text }).catch?.(() => {});
  action.setBadgeBackgroundColor({ tabId, color: "#2f2f2f" }).catch?.(() => {});
};

Shield.resetTabCount = async function resetTabCount(tabId) {
  const session = await Shield.api.storage.session.get("tabCounts");
  const tabCounts = session.tabCounts || {};
  delete tabCounts[tabId];
  await Shield.api.storage.session.set({ tabCounts });
  Shield.showBadge(tabId, 0);
};

Shield.tabCount = async function tabCount(tabId) {
  const session = await Shield.api.storage.session.get("tabCounts");
  return (session.tabCounts || {})[tabId] || 0;
};

Shield.statsSummary = async function statsSummary() {
  const stored = await Shield.api.storage.local.get("stats");
  const stats = stored.stats || {};
  const today = stats[Shield.todayKey()] || {};
  const month = {};
  for (const day of Object.values(stats)) {
    for (const [kind, amount] of Object.entries(day)) month[kind] = (month[kind] || 0) + amount;
  }
  return { today, month };
};
})();
