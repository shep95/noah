// Tools and modes: profiles, focus hours, low data, battery saver, the search
// switcher, parental mode, the weekly report, download expiry, the notes
// vault, the unshortener, the data-broker list and the request generators,
// and the policy reader.
"use strict";
(() => {
const Shield = (globalThis.Shield = globalThis.Shield || {});

// Dynamic rule ranges (1..1999 are per-site trust and strict-cookie rules).
const FOCUS_RULE_BASE = 3000;
const LOW_DATA_RULE_BASE = 4000;
const SEARCH_RULE_BASE = 4100;
const PARENTAL_RULE_BASE = 6000;

// ---- profiles ----------------------------------------------------------------------

Shield.PROFILES = {
  browsing: {
    label: "browsing: most private",
    change: { privacy: { blendIn: true, perSite: true, fingerprint: true, thirdPartyCookies: false, stripParameters: true, referrer: true }, security: { referrer: true }, shopping: { compare: false, coupons: false, autoApply: false } },
  },
  shopping: {
    label: "shopping: prices and codes on",
    change: { privacy: { blendIn: false, perSite: true, fingerprint: true }, shopping: { compare: true, coupons: true, autoApply: true, clipCoupons: true } },
  },
  banking: {
    label: "banking: strict, plain and quiet",
    change: { privacy: { blendIn: false, fingerprint: false, location: { mode: "deny" } }, security: { clipboardGuard: true, formLeak: true, walletGuard: true, passwordReuse: true, hiddenFields: true }, shopping: { compare: false, coupons: false, autoApply: false } },
  },
};

Shield.applyProfile = async function applyProfile(name) {
  const profile = Shield.PROFILES[name];
  if (!profile) throw new Error("unknown profile");
  const settings = await Shield.updateSettings(Shield.deepMerge(profile.change, { modes: { profile: name } }));
  return settings;
};

// Banking mode's strongest tool: no other extension touches the page while a
// banking site is in front. They are put back when you leave.
Shield.pauseOtherExtensions = async function pauseOtherExtensions(pause) {
  const management = Shield.api.management;
  if (!management || !management.getAll) return { paused: [] };
  const session = await Shield.api.storage.session.get("pausedExtensions");
  if (pause) {
    if (session.pausedExtensions && session.pausedExtensions.length) return { paused: session.pausedExtensions };
    const all = await management.getAll();
    const paused = [];
    for (const extension of all) {
      if (extension.id === Shield.api.runtime.id || extension.type !== "extension" || !extension.enabled || !extension.mayDisable) continue;
      try {
        await management.setEnabled(extension.id, false);
        paused.push(extension.id);
      } catch {
        // An extension policy forbids touching stays as it is.
      }
    }
    await Shield.api.storage.session.set({ pausedExtensions: paused });
    return { paused };
  }
  for (const id of session.pausedExtensions || []) {
    try { await management.setEnabled(id, true); } catch { /* gone */ }
  }
  await Shield.api.storage.session.set({ pausedExtensions: [] });
  return { paused: [] };
};

// ---- focus hours -----------------------------------------------------------------------

function nowInWindow(focus) {
  const now = new Date();
  if (!focus.days.includes(now.getDay())) return false;
  const [startHour, startMinute] = focus.start.split(":").map(Number);
  const [endHour, endMinute] = focus.end.split(":").map(Number);
  const minutes = now.getHours() * 60 + now.getMinutes();
  const start = startHour * 60 + startMinute;
  const end = endHour * 60 + endMinute;
  return start <= end ? minutes >= start && minutes < end : minutes >= start || minutes < end;
}

Shield.applyFocusHours = function applyFocusHours(settings) {
  return Shield.withRuleLock(() => applyFocusHoursNow(settings));
};
async function applyFocusHoursNow(settings) {
  const rules = Shield.api.declarativeNetRequest;
  if (!rules) return false;
  const focus = settings.modes.focus;
  const active = focus.enabled && focus.sites.length && nowInWindow(focus);
  const existing = (await rules.getDynamicRules()).filter((rule) => rule.id >= FOCUS_RULE_BASE && rule.id < FOCUS_RULE_BASE + 500).map((rule) => rule.id);
  const addRules = active ? focus.sites.slice(0, 400).map((site, index) => ({
    id: FOCUS_RULE_BASE + index,
    priority: 20,
    action: { type: "redirect", redirect: { url: Shield.api.runtime.getURL("warn.html?kind=focus&site=" + encodeURIComponent(site)) } },
    condition: { requestDomains: [site], resourceTypes: ["main_frame"] },
  })) : [];
  await rules.updateDynamicRules({ removeRuleIds: existing, addRules });
  return Boolean(active);
}

// ---- low data --------------------------------------------------------------------------

Shield.applyLowData = function applyLowData(settings) {
  return Shield.withRuleLock(() => applyLowDataNow(settings));
};
async function applyLowDataNow(settings) {
  const rules = Shield.api.declarativeNetRequest;
  if (!rules) return;
  const existing = (await rules.getDynamicRules()).filter((rule) => rule.id >= LOW_DATA_RULE_BASE && rule.id < LOW_DATA_RULE_BASE + 10).map((rule) => rule.id);
  const addRules = settings.modes.lowData ? [
    { id: LOW_DATA_RULE_BASE, priority: 3, action: { type: "block" }, condition: { urlFilter: "*", resourceTypes: ["media", "font"] } },
    { id: LOW_DATA_RULE_BASE + 1, priority: 3, action: { type: "block" }, condition: { urlFilter: "*", domainType: "thirdParty", resourceTypes: ["image"] } },
  ] : [];
  await rules.updateDynamicRules({ removeRuleIds: existing, addRules });
}

// ---- battery saver ------------------------------------------------------------------------

Shield.discardIdleTabs = async function discardIdleTabs(settings) {
  if (!settings.modes.batterySaver || !Shield.api.tabs.discard) return 0;
  const tabs = await Shield.api.tabs.query({ active: false, audible: false, pinned: false, discarded: false });
  let discarded = 0;
  const cutoff = Date.now() - settings.modes.batteryMinutes * 60000;
  for (const tab of tabs) {
    if (tab.lastAccessed && tab.lastAccessed > cutoff) continue;
    try {
      await Shield.api.tabs.discard(tab.id);
      discarded++;
    } catch {
      // Some tabs refuse; fine.
    }
  }
  return discarded;
};

// ---- private search -------------------------------------------------------------------------

const ENGINES = {
  duckduckgo: "https://duckduckgo.com/?q=\\1",
  brave: "https://search.brave.com/search?q=\\1",
  startpage: "https://www.startpage.com/do/search?q=\\1",
  qwant: "https://www.qwant.com/?q=\\1",
  ecosia: "https://www.ecosia.org/search?q=\\1",
};

Shield.applySearchSwitch = function applySearchSwitch(settings) {
  return Shield.withRuleLock(() => applySearchSwitchNow(settings));
};
async function applySearchSwitchNow(settings) {
  const rules = Shield.api.declarativeNetRequest;
  if (!rules) return;
  const existing = (await rules.getDynamicRules()).filter((rule) => rule.id >= SEARCH_RULE_BASE && rule.id < SEARCH_RULE_BASE + 20).map((rule) => rule.id);
  const engine = ENGINES[settings.modes.searchEngine];
  const addRules = engine ? [
    { id: SEARCH_RULE_BASE, priority: 4, action: { type: "redirect", redirect: { regexSubstitution: engine } }, condition: { regexFilter: "^https?://(?:www\\.)?google\\.[a-z.]+/search\\?(?:.*&)?q=([^&#]+)", resourceTypes: ["main_frame"] } },
    { id: SEARCH_RULE_BASE + 1, priority: 4, action: { type: "redirect", redirect: { regexSubstitution: engine } }, condition: { regexFilter: "^https?://(?:www\\.)?bing\\.com/search\\?(?:.*&)?q=([^&#]+)", resourceTypes: ["main_frame"] } },
    { id: SEARCH_RULE_BASE + 2, priority: 4, action: { type: "redirect", redirect: { regexSubstitution: engine } }, condition: { regexFilter: "^https?://search\\.yahoo\\.com/search\\?(?:.*&)?p=([^&#]+)", resourceTypes: ["main_frame"] } },
  ] : [];
  await rules.updateDynamicRules({ removeRuleIds: existing, addRules });
}

// ---- parental mode --------------------------------------------------------------------------

const ADULT_DOMAINS = [
  "pornhub.com", "xvideos.com", "xnxx.com", "xhamster.com", "redtube.com", "youporn.com", "tube8.com", "spankbang.com", "eporner.com", "porntrex.com", "hqporner.com", "beeg.com",
  "chaturbate.com", "stripchat.com", "livejasmin.com", "bongacams.com", "cam4.com", "myfreecams.com", "camsoda.com", "onlyfans.com", "fansly.com", "brazzers.com", "bangbros.com",
  "realitykings.com", "naughtyamerica.com", "mofos.com", "digitalplayground.com", "xhamsterlive.com", "rule34.xxx", "e621.net", "nhentai.net", "hentaihaven.xxx", "hanime.tv",
  "fapello.com", "motherless.com", "heavy-r.com", "thisvid.com", "porn.com", "pornone.com", "4tube.com", "porndig.com", "ixxx.com", "drtuber.com", "sunporno.com", "tnaflix.com",
  "empflix.com", "youjizz.com", "porntube.com", "pornhat.com", "xmoviesforyou.com", "erome.com", "coomer.su", "kemono.su", "simpcity.su", "adultfriendfinder.com", "ashleymadison.com",
];
const ADULT_KEYWORDS = "(porn|xxx|hentai|xvideo|xhamster|sexcam|camgirl|escort|milf|nsfw|onlyfans|fleshlight|dildo)";

Shield.applyParental = function applyParental(settings) {
  return Shield.withRuleLock(() => applyParentalNow(settings));
};
async function applyParentalNow(settings) {
  const rules = Shield.api.declarativeNetRequest;
  if (!rules) return;
  const existing = (await rules.getDynamicRules()).filter((rule) => rule.id >= PARENTAL_RULE_BASE && rule.id < PARENTAL_RULE_BASE + 500).map((rule) => rule.id);
  const parental = settings.modes.parental;
  const addRules = [];
  if (parental.enabled) {
    const page = Shield.api.runtime.getURL("warn.html?kind=parental");
    addRules.push({ id: PARENTAL_RULE_BASE, priority: 30, action: { type: "redirect", redirect: { url: page } }, condition: { requestDomains: ADULT_DOMAINS.concat(parental.extraSites || []), resourceTypes: ["main_frame", "sub_frame"] } });
    addRules.push({ id: PARENTAL_RULE_BASE + 1, priority: 30, action: { type: "redirect", redirect: { url: page } }, condition: { regexFilter: "^https?://[^/]*" + ADULT_KEYWORDS + "[^/]*/", resourceTypes: ["main_frame"], isUrlFilterCaseSensitive: false } });
    addRules.push({ id: PARENTAL_RULE_BASE + 2, priority: 30, action: { type: "redirect", redirect: { transform: { queryTransform: { addOrReplaceParams: [{ key: "safe", value: "active" }] } } } }, condition: { regexFilter: "^https?://(www\\.)?google\\.[a-z.]+/search\\?", resourceTypes: ["main_frame"] } });
    addRules.push({ id: PARENTAL_RULE_BASE + 3, priority: 30, action: { type: "redirect", redirect: { transform: { queryTransform: { addOrReplaceParams: [{ key: "adlt", value: "strict" }] } } } }, condition: { regexFilter: "^https?://(www\\.)?bing\\.com/(search|images|videos)", resourceTypes: ["main_frame"] } });
    addRules.push({ id: PARENTAL_RULE_BASE + 4, priority: 30, action: { type: "redirect", redirect: { transform: { queryTransform: { addOrReplaceParams: [{ key: "kp", value: "1" }] } } } }, condition: { regexFilter: "^https?://(html\\.|lite\\.)?duckduckgo\\.com/(\\?|html)", resourceTypes: ["main_frame"] } });
    addRules.push({ id: PARENTAL_RULE_BASE + 5, priority: 30, action: { type: "modifyHeaders", requestHeaders: [{ header: "YouTube-Restrict", operation: "set", value: "Strict" }] }, condition: { requestDomains: ["youtube.com", "www.youtube.com", "m.youtube.com", "youtubei.googleapis.com"], resourceTypes: ["main_frame", "sub_frame", "xmlhttprequest"] } });
    addRules.push({ id: PARENTAL_RULE_BASE + 6, priority: 30, action: { type: "modifyHeaders", requestHeaders: [{ header: "Prefer-SafeSearch", operation: "set", value: "strict" }] }, condition: { urlFilter: "*", resourceTypes: ["main_frame"] } });
  }
  await rules.updateDynamicRules({ removeRuleIds: existing, addRules });
}

Shield.hashPin = async function hashPin(pin) {
  const salt = await Shield.installSalt();
  const material = await crypto.subtle.importKey("raw", new TextEncoder().encode(pin), "PBKDF2", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", salt: new TextEncoder().encode(salt + "|pin"), iterations: 200000, hash: "SHA-256" }, material, 256);
  return Shield.hex(new Uint8Array(bits));
};

// ---- weekly report ----------------------------------------------------------------------------------

Shield.weeklyReport = async function weeklyReport() {
  const stored = await Shield.api.storage.local.get("stats");
  const stats = stored.stats || {};
  const week = {};
  const cutoff = Date.now() - 7 * 86400000;
  for (const [day, counts] of Object.entries(stats)) {
    if (Date.parse(day) < cutoff) continue;
    for (const [kind, amount] of Object.entries(counts)) week[kind] = (week[kind] || 0) + amount;
  }
  const lines = [];
  if (week.trackers) lines.push(`${week.trackers} tracker requests stopped`);
  if (week.cookies) lines.push(`${week.cookies} tracking cookies deleted`);
  if (week.phishing) lines.push(`${week.phishing} lookalike pages caught`);
  if (week.leaks) lines.push(`${week.leaks} form leaks stopped`);
  if (week.popups) lines.push(`${week.popups} scam pages broken`);
  if (week.coupons) lines.push(`${week.coupons} coupon codes that worked`);
  if (week.saved) lines.push(`${week.saved} saved on orders`);
  if (week.banners) lines.push(`${week.banners} cookie banners answered for you`);
  if (week.overlays) lines.push(`${week.overlays} overlays removed`);
  return { week, lines, text: lines.length ? lines.join(", ") + "." : "A quiet week: nothing needed stopping." };
};

// ---- downloads that expire ------------------------------------------------------------------------------

Shield.scheduleDownloadExpiry = async function scheduleDownloadExpiry(item, settings) {
  const expiry = settings.modes.expireDownloads;
  if (!expiry.enabled) return false;
  const source = Shield.siteOf(Shield.hostOf(item.referrer || item.url || ""));
  const name = String(item.filename || "").split(/[\\/]/).pop();
  const sensitive = /\.(pdf|csv|xlsx?|docx?|ofx|qfx|qif|json|zip)$/i.test(name);
  if (!(expiry.sites.includes(source) || (expiry.allSensitive && sensitive))) return false;
  const stored = await Shield.api.storage.local.get("expiring");
  const expiring = stored.expiring || {};
  expiring[String(item.id)] = { name, at: Date.now() + expiry.days * 86400000, source };
  await Shield.api.storage.local.set({ expiring });
  await Shield.api.alarms.create("expire-" + item.id, { when: Date.now() + expiry.days * 86400000 });
  return true;
};

Shield.expireDownload = async function expireDownload(id) {
  const stored = await Shield.api.storage.local.get("expiring");
  const expiring = stored.expiring || {};
  const entry = expiring[String(id)];
  delete expiring[String(id)];
  await Shield.api.storage.local.set({ expiring });
  if (!entry) return null;
  try {
    await Shield.api.downloads.removeFile(Number(id));
    await Shield.api.downloads.erase({ id: Number(id) });
  } catch {
    // Already gone.
  }
  return entry;
};

// ---- the notes vault -----------------------------------------------------------------------------------

async function vaultKey(passphrase, salt) {
  const material = await crypto.subtle.importKey("raw", new TextEncoder().encode(passphrase), "PBKDF2", false, ["deriveKey"]);
  return crypto.subtle.deriveKey({ name: "PBKDF2", salt, iterations: 310000, hash: "SHA-256" }, material, { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
}

Shield.vaultSave = async function vaultSave(passphrase, notes) {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await vaultKey(passphrase, salt);
  const cipher = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, new TextEncoder().encode(JSON.stringify(notes))));
  await Shield.api.storage.local.set({ vault: { salt: Shield.hex(salt), iv: Shield.hex(iv), data: btoa(String.fromCharCode(...cipher)), at: new Date().toISOString() } });
};

Shield.vaultOpen = async function vaultOpen(passphrase) {
  const stored = await Shield.api.storage.local.get("vault");
  if (!stored.vault) return [];
  const key = await vaultKey(passphrase, Shield.fromHex(stored.vault.salt));
  const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: Shield.fromHex(stored.vault.iv) }, key, Shield.fromBase64(stored.vault.data));
  return JSON.parse(new TextDecoder().decode(plain));
};

// ---- links: where they really go -----------------------------------------------------------------------------

Shield.SHORTENERS = /(^|\.)(bit\.ly|t\.co|tinyurl\.com|goo\.gl|ow\.ly|is\.gd|buff\.ly|rebrand\.ly|cutt\.ly|shorturl\.at|t\.ly|lnkd\.in|rb\.gy|tiny\.cc|bl\.ink|s\.id|v\.gd|qr\.ae|amzn\.to|amzn\.eu|fb\.me|youtu\.be|w\.wiki|trib\.al|dlvr\.it|ift\.tt|snip\.ly|shor\.by|short\.io|clck\.ru|u\.to|x\.co|po\.st|wp\.me|adf\.ly|linktr\.ee)$/i;

// The final address behind a link. One request follows the redirects, so the
// shortener counts a click; nothing else is sent.
Shield.unshorten = async function unshorten(url) {
  if (Shield.isLocalHost(Shield.hostOf(url))) return { final: url, hops: [url], error: "local addresses are not followed" };
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 10000);
  const hops = [url];
  // A chain that ends on this computer or the local network is reported and
  // not read: a shortener must not become a way onto a router's admin page.
  const landed = (response) => {
    if (response.url && response.url !== url) hops.push(response.url);
    if (Shield.isLocalHost(Shield.hostOf(response.url || url))) return { final: response.url || url, hops, error: "the chain ends at a local address, which is not followed" };
    return { final: response.url || url, hops, status: response.status, redirected: response.redirected };
  };
  try {
    return landed(await fetch(url, { method: "HEAD", redirect: "follow", credentials: "omit", signal: controller.signal }));
  } catch (error) {
    try {
      return landed(await fetch(url, { method: "GET", redirect: "follow", credentials: "omit", signal: controller.signal }));
    } catch (again) {
      return { final: url, hops, error: String(again.message || again) };
    }
  } finally {
    clearTimeout(timer);
  }
};

// Every hop the browser actually took for a tab's last navigation.
Shield.recordHop = async function recordHop(tabId, from, to) {
  const session = await Shield.api.storage.session.get("hops");
  const hops = session.hops || {};
  const chain = hops[tabId] || [];
  if (!chain.length || chain[chain.length - 1] !== from) chain.push(from);
  chain.push(to);
  hops[tabId] = chain.slice(-20);
  await Shield.api.storage.session.set({ hops });
};

Shield.hopsFor = async function hopsFor(tabId) {
  const session = await Shield.api.storage.session.get("hops");
  return (session.hops || {})[tabId] || [];
};

Shield.resetHops = async function resetHops(tabId, url) {
  const session = await Shield.api.storage.session.get("hops");
  const hops = session.hops || {};
  hops[tabId] = [url];
  await Shield.api.storage.session.set({ hops });
};

// ---- people-search sites and the letters ------------------------------------------------------------------------

Shield.BROKERS = [
  { name: "Spokeo", optOut: "https://www.spokeo.com/optout" },
  { name: "Whitepages", optOut: "https://www.whitepages.com/suppression-requests" },
  { name: "BeenVerified", optOut: "https://www.beenverified.com/app/optout/search" },
  { name: "Intelius", optOut: "https://www.intelius.com/opt-out/submit/" },
  { name: "TruthFinder", optOut: "https://www.truthfinder.com/opt-out/" },
  { name: "Instant Checkmate", optOut: "https://www.instantcheckmate.com/opt-out/" },
  { name: "PeopleFinders", optOut: "https://www.peoplefinders.com/opt-out" },
  { name: "MyLife", optOut: "https://www.mylife.com/ccpa/index.pubview" },
  { name: "Radaris", optOut: "https://radaris.com/control/privacy" },
  { name: "FastPeopleSearch", optOut: "https://www.fastpeoplesearch.com/removal" },
  { name: "TruePeopleSearch", optOut: "https://www.truepeoplesearch.com/removal" },
  { name: "PeekYou", optOut: "https://www.peekyou.com/about/contact/optout/" },
  { name: "USPhoneBook", optOut: "https://www.usphonebook.com/opt-out" },
  { name: "ClustrMaps", optOut: "https://clustrmaps.com/bl/opt-out" },
  { name: "Nuwber", optOut: "https://nuwber.com/removal/link" },
  { name: "ThatsThem", optOut: "https://thatsthem.com/optout" },
  { name: "Acxiom", optOut: "https://isapps.acxiom.com/optout/optout.aspx" },
  { name: "Oracle (Datalogix)", optOut: "https://www.oracle.com/legal/privacy/privacy-choices.html" },
  { name: "Epsilon", optOut: "https://www.epsilon.com/us/consumer-information" },
  { name: "LexisNexis", optOut: "https://consumer.risk.lexisnexis.com/request" },
  { name: "Experian Marketing", optOut: "https://www.experian.com/privacy/opting_out.html" },
  { name: "192.com (UK)", optOut: "https://www.192.com/c01/removal" },
  { name: "Pipl", optOut: "https://pipl.com/personal-information-removal-request" },
  { name: "ZoomInfo", optOut: "https://www.zoominfo.com/privacy-center/update/profile" },
  { name: "RocketReach", optOut: "https://rocketreach.co/claim-profile" },
];

Shield.deletionLetter = function deletionLetter({ name, email, company, law }) {
  const basis = law === "gdpr"
    ? "Under Article 17 of the General Data Protection Regulation (GDPR), I request the erasure of all personal data you hold about me. Under Article 15, I also request confirmation of what data you held, where it came from, and whom you shared it with. You have one month to respond."
    : law === "ccpa"
      ? "Under the California Consumer Privacy Act (CCPA / CPRA), I request that you delete all personal information you have collected about me, and that you direct your service providers and any third parties you sold or shared it with to do the same. Please confirm within 45 days. I also request that you do not sell or share my personal information."
      : "I request that you delete all personal data you hold about me and confirm in writing when it is done, and that you stop selling or sharing it. Please tell me what data you held and where it came from.";
  return `Subject: Request to delete my personal data

To ${company || "whom it may concern"},

${basis}

The data can be identified by the name ${name || "[your name]"} and the email address ${email || "[your email]"}. Please do not use this information for any purpose other than locating and deleting my records. If you need more identifying details, tell me exactly which and why.

Please reply to this address with confirmation of the deletion.

Regards,
${name || "[your name]"}`;
};

// ---- policy reader: what a long policy actually says --------------------------------------------------------------------

const POLICY_FLAGS = [
  { key: "sells", pattern: /\b(sell|sale of|selling)\b[^.]{0,80}\b(personal|your) (data|information)/i, line: "They may sell your personal information." },
  { key: "shares", pattern: /\b(share|disclose|provide)\b[^.]{0,80}\b(third[- ]part|partners|advertisers|affiliates)/i, line: "They share data with third parties, partners or advertisers." },
  { key: "ads", pattern: /\b(targeted|interest[- ]based|personali[sz]ed|behavio(u)?ral) (advertising|ads|marketing)/i, line: "They run targeted advertising on your behaviour." },
  { key: "location", pattern: /\b(precise|geo)?location (data|information)\b/i, line: "They collect location data." },
  { key: "biometric", pattern: /\b(biometric|facial recognition|voiceprint|fingerprint)\b/i, line: "They mention biometric data." },
  { key: "retention", pattern: /\b(retain|keep|store)[^.]{0,60}\b(as long as|indefinitely|for the duration|until)/i, line: "Retention is open-ended: kept as long as they see fit." },
  { key: "arbitration", pattern: /\b(binding )?arbitration\b/i, line: "Disputes go to arbitration: you cannot sue in court." },
  { key: "classAction", pattern: /class[- ]action (waiver|lawsuit|proceeding)/i, line: "You waive class-action rights." },
  { key: "autoRenew", pattern: /\b(auto(matic(ally)?)?[- ]?renew|automatically (charged|billed)|recurring (billing|charge))/i, line: "Subscriptions renew and charge automatically." },
  { key: "changes", pattern: /\b(we may|reserve the right to) (change|modify|update|amend)[^.]{0,60}\b(at any time|without notice)/i, line: "Terms can change at any time without notice." },
  { key: "children", pattern: /\b(children|minors)\b[^.]{0,60}\b(under|below) (13|16|18)/i, line: "They address data from children." },
  { key: "tracking", pattern: /\b(cookies|pixels|web beacons|device identifiers|fingerprint)/i, line: "They track you with cookies, pixels or device identifiers." },
  { key: "training", pattern: /\b(train|improve)[^.]{0,60}\b(machine learning|artificial intelligence|AI|models)\b/i, line: "Your content may train their AI models." },
  { key: "rights", pattern: /\b(right to (access|erasure|delete|deletion|portability|opt[- ]out)|do not sell)/i, line: "They describe rights to access, delete or opt out (good: use them).", good: true },
  { key: "encryption", pattern: /\b(encrypt(ed|ion)|end[- ]to[- ]end)\b/i, line: "They mention encryption.", good: true },
  { key: "noSell", pattern: /\b(we do not|never|will not) sell\b[^.]{0,40}\b(personal|your) (data|information)/i, line: "They say they do not sell your data.", good: true },
];

Shield.readPolicy = function readPolicy(text) {
  const words = String(text || "").replace(/\s+/g, " ");
  const found = [];
  for (const flag of POLICY_FLAGS) {
    const match = flag.pattern.exec(words);
    if (match) found.push({ ...flag, quote: words.slice(Math.max(0, match.index - 60), match.index + 140).trim() });
  }
  const bad = found.filter((flag) => !flag.good);
  const good = found.filter((flag) => flag.good);
  const minutes = Math.max(1, Math.round(words.split(" ").length / 220));
  const grade = bad.length >= 7 ? "F" : bad.length >= 5 ? "D" : bad.length >= 3 ? "C" : bad.length >= 1 ? "B" : "A";
  return { grade, minutes, lines: [...bad.slice(0, 5), ...good.slice(0, 2)], count: found.length, wordCount: words.split(" ").length };
};
})();
