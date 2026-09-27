// Shared by the background worker, the popup and the options page. Plain
// scripts rather than modules, because Chromium loads the background as a
// service worker (importScripts) and Firefox as an event page (a script list),
// and one file that works in both keeps the extension a single code base.
"use strict";
(() => {

const Shield = (globalThis.Shield = globalThis.Shield || {});

Shield.api = globalThis.browser ?? globalThis.chrome;
Shield.SITE = "https://noah.asherin.com";
Shield.FEED_BASE = Shield.SITE + "/shield/";
Shield.WHOAMI_URL = Shield.SITE + "/api/whoami";
// The ed25519 key noah's releases are signed with (crates/auto_update/release_public_key.txt).
// Every feed the shield reads is signed with it, so this website alone cannot
// change what the shield does.
Shield.PUBLIC_KEY_HEX = "50e21800e89b09f431a54249efdb39ef770563b87734f56142a326e2c536f5e2";

Shield.DEFAULT_SETTINGS = {
  tunnel: {
    enabled: false,
    serverId: null,
    killSwitch: true,
    webRtcGuard: true,
    bypassLocal: true,
    connectAtStartup: false,
    siteRoutes: {},
  },
  privacy: {
    trackers: true,
    thirdPartyCookies: false,
    gpc: true,
    stripParameters: true,
    geolocation: "block",
    notifications: "block",
    camera: "ask",
    microphone: "ask",
    fingerprint: true,
    adTopics: false,
    prefetch: false,
    purgeTrackerCookies: true,
    trustedSites: [],
    strictCookieSites: [],
    burnOnClose: false,
    keepSites: [],
    meter: "count",
    blendIn: false,
    perSite: true,
    rotate: "session",
    audioNoise: true,
    webglNoise: true,
    location: { mode: "deny", city: "" },
    aliasBase: "",
    simpleLoginKey: "",
    hideNames: [],
    logoutSites: [],
    logoutMinutes: 30,
    forgetHistorySites: [],
    stealth: false,
    sync: false,
    linkCleaner: true,
  },
  shopping: {
    compare: true,
    coupons: true,
    autoApply: true,
    clipCoupons: true,
    codes: [],
    quietSites: [],
  },
  capture: {
    guardScreen: true,
    guardCamera: true,
    auditExtensions: true,
    allowedSites: [],
  },
  security: {
    httpsUpgrade: true,
    lookalike: true,
    passwordReuse: true,
    passwordHttp: true,
    breachCheck: true,
    hibpKey: "",
    downloadWarn: true,
    virusTotalKey: "",
    clipboardGuard: true,
    clipboardWipe: true,
    hiddenFields: true,
    formLeak: true,
    walletGuard: true,
    scamPopups: true,
    hiddenFrames: true,
    socialLogin: false,
    referrer: true,
    mailPixels: true,
    allowedLookalikes: [],
    clipboardSites: [],
    walletSites: [],
  },
  annoyances: {
    banners: true,
    overlays: true,
    autoplay: true,
    timers: true,
    copySites: [],
    darkSites: [],
    darkEverywhere: false,
    pasteGuard: true,
    uploadStrip: true,
  },
  modes: {
    profile: "",
    siteModes: {},
    pauseExtensionsForBanking: false,
    focus: { enabled: false, sites: [], start: "09:00", end: "17:00", days: [1, 2, 3, 4, 5] },
    lowData: false,
    batterySaver: false,
    batteryMinutes: 20,
    searchEngine: "",
    parental: { enabled: false, pinHash: "", extraSites: [] },
    expireDownloads: { enabled: false, days: 7, sites: [], allSensitive: false },
    lock: { sites: [], minutes: 5, pinHash: "" },
    weeklyReport: true,
    containers: false,
  },
  quiet: false,
  localOnly: false,
};

Shield.deepMerge = function deepMerge(base, extra) {
  if (Array.isArray(base) || typeof base !== "object" || base === null) {
    return extra === undefined ? base : extra;
  }
  const merged = { ...base };
  if (extra && typeof extra === "object") {
    for (const key of Object.keys(extra)) {
      merged[key] = key in base ? deepMerge(base[key], extra[key]) : extra[key];
    }
  }
  return merged;
};

Shield.loadSettings = async function loadSettings() {
  const stored = await Shield.api.storage.local.get("settings");
  return Shield.deepMerge(Shield.DEFAULT_SETTINGS, stored.settings || {});
};

Shield.saveSettings = async function saveSettings(settings) {
  await Shield.api.storage.local.set({ settings });
};

Shield.updateSettings = async function updateSettings(change) {
  const settings = Shield.deepMerge(await Shield.loadSettings(), change);
  await Shield.saveSettings(settings);
  return settings;
};

Shield.hostOf = function hostOf(url) {
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return "";
  }
};

// "www.shop.example.co.uk" -> "example.co.uk" for the common two-part
// public suffixes; good enough to group a site's own hosts together.
const TWO_PART_SUFFIXES = new Set([
  "co.uk", "org.uk", "ac.uk", "gov.uk", "me.uk", "net.uk", "com.au", "net.au", "org.au",
  "co.nz", "co.jp", "ne.jp", "or.jp", "co.kr", "com.br", "com.mx", "com.ar", "com.tr",
  "co.in", "co.za", "com.sg", "com.hk", "com.tw", "com.cn", "com.my", "com.ph", "co.id",
  "com.ua", "com.pl", "com.eg", "com.sa", "com.pk", "com.ng", "co.il", "com.vn", "co.th",
]);
Shield.siteOf = function siteOf(host) {
  if (!host) return "";
  if (host.includes(":") || /^[\d.]+$/.test(host)) return host;
  const parts = host.split(".");
  if (parts.length <= 2) return host;
  const lastTwo = parts.slice(-2).join(".");
  if (TWO_PART_SUFFIXES.has(lastTwo) && parts.length >= 3) return parts.slice(-3).join(".");
  return lastTwo;
};

Shield.isLocalHost = function isLocalHost(host) {
  if (!host) return true;
  if (host === "localhost" || host.endsWith(".localhost") || host.endsWith(".local")) return true;
  if (!host.includes(".") && !host.includes(":")) return true;
  if (/^127\.|^10\.|^192\.168\.|^169\.254\.|^0\./.test(host)) return true;
  if (/^172\.(1[6-9]|2\d|3[01])\./.test(host)) return true;
  const bare = host.replace(/^\[|\]$/g, "");
  if (bare === "::1" || /^f[cd][0-9a-f]{2}:/i.test(bare) || /^fe80:/i.test(bare)) return true;
  return false;
};

Shield.todayKey = function todayKey() {
  return new Date().toISOString().slice(0, 10);
};

Shield.hex = function hex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
};

Shield.fromHex = function fromHex(text) {
  const bytes = new Uint8Array(text.length / 2);
  for (let index = 0; index < bytes.length; index++) {
    bytes[index] = parseInt(text.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
};

Shield.fromBase64 = function fromBase64(text) {
  const binary = atob(text.trim());
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index);
  return bytes;
};

Shield.compareVersions = function compareVersions(left, right) {
  const a = String(left).split(".").map((part) => parseInt(part, 10) || 0);
  const b = String(right).split(".").map((part) => parseInt(part, 10) || 0);
  for (let index = 0; index < Math.max(a.length, b.length); index++) {
    const difference = (a[index] || 0) - (b[index] || 0);
    if (difference !== 0) return difference;
  }
  return 0;
};

// One request to the background at a time per name; the popup and content
// scripts use this so a slow answer never piles up.
Shield.send = function send(message) {
  return new Promise((resolve) => {
    try {
      if (globalThis.browser && globalThis.browser.runtime) {
        globalThis.browser.runtime.sendMessage(message).then(resolve, (error) => resolve({ error: String(error) }));
        return;
      }
      globalThis.chrome.runtime.sendMessage(message, (response) => {
        if (globalThis.chrome.runtime.lastError) resolve({ error: globalThis.chrome.runtime.lastError.message });
        else resolve(response);
      });
    } catch (error) {
      resolve({ error: String(error) });
    }
  });
};

// declarativeNetRequest dynamic rules are read-modify-write; the writers
// take turns so two of them never add the same ids at once.
let ruleQueue = Promise.resolve();
Shield.withRuleLock = function withRuleLock(work) {
  const run = ruleQueue.then(work, work);
  ruleQueue = run.catch(() => {});
  return run;
};

Shield.isFirefox = typeof navigator !== "undefined" && /Firefox\//.test(navigator.userAgent);
Shield.isSafari =
  typeof navigator !== "undefined" &&
  /Safari\//.test(navigator.userAgent) &&
  !/Chrom(e|ium)\//.test(navigator.userAgent) &&
  !/Firefox\//.test(navigator.userAgent);
})();
