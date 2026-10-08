// Security: lookalike domains, password hygiene, breach checks, downloads.
"use strict";
(() => {
const Shield = (globalThis.Shield = globalThis.Shield || {});

// Names that are also ordinary words only count with a homoglyph or a
// login-ish word beside them, or "office.example" would be a false alarm.
const GENERIC = new Set(["target", "office", "apple", "chase", "wise", "ups", "att", "irs", "booking", "steam", "zoom", "slack", "discord", "reddit", "uber", "wordpress", "cloudflare", "intuit", "okta", "norton", "block", "square", "wise", "monzo", "kraken", "ledger", "claude"]);
const LOGIN_WORDS = /(^|[-.])(login|log-in|signin|sign-in|secure|security|verify|verification|account|accounts|update|support|billing|confirm|auth|wallet|recover|unlock|service|help|id)([-.]|$)/;
const HOMOGLYPHS = [[/0/g, "o"], [/1/g, "l"], [/3/g, "e"], [/4/g, "a"], [/5/g, "s"], [/7/g, "t"], [/8/g, "b"], [/9/g, "g"], [/rn/g, "m"], [/vv/g, "w"], [/cl/g, "d"], [/ı/g, "i"]];

function normalize(label) {
  let out = label.toLowerCase();
  // Punycode hosts are their own warning: xn-- prefixes never belong to these brands.
  for (const [pattern, letter] of HOMOGLYPHS) out = out.replace(pattern, letter);
  return out.replace(/[^a-z]/g, "");
}

function levenshtein(left, right) {
  if (Math.abs(left.length - right.length) > 1) return 2;
  const rows = Array.from({ length: left.length + 1 }, (_, index) => [index]);
  for (let column = 1; column <= right.length; column++) rows[0][column] = column;
  for (let row = 1; row <= left.length; row++) {
    for (let column = 1; column <= right.length; column++) {
      rows[row][column] = Math.min(
        rows[row - 1][column] + 1,
        rows[row][column - 1] + 1,
        rows[row - 1][column - 1] + (left[row - 1] === right[column - 1] ? 0 : 1),
      );
    }
  }
  return rows[left.length][right.length];
}

function belongsTo(host, brand) {
  return brand.domains.some((domain) => host === domain || host.endsWith("." + domain));
}

// Why a host looks like a brand it is not, or null.
Shield.lookalike = function lookalike(host) {
  host = String(host || "").toLowerCase();
  if (!host || Shield.isLocalHost(host)) return null;
  const site = Shield.siteOf(host);
  const labels = host.split(".");
  const siteLabel = site.split(".")[0];
  const subdomainLabels = labels.slice(0, Math.max(0, labels.length - site.split(".").length));
  const punycode = labels.some((label) => label.startsWith("xn--"));
  for (const brand of Shield.BRANDS) {
    if (belongsTo(host, brand)) return null;
  }
  for (const brand of Shield.BRANDS) {
    const core = brand.name;
    const generic = GENERIC.has(core);
    const real = brand.domains[0];
    const normalizedSite = normalize(siteLabel);
    const usedHomoglyph = normalizedSite !== siteLabel.toLowerCase().replace(/[^a-z]/g, "");
    if (normalizedSite === core) {
      if (!generic || usedHomoglyph || punycode) return { brand: core, real, reason: usedHomoglyph ? "letters swapped for lookalikes" : "same name, different domain" };
    }
    if (core.length >= 5 && !generic && levenshtein(siteLabel.toLowerCase(), core) === 1) {
      return { brand: core, real, reason: "one letter away from " + core };
    }
    const loginish = LOGIN_WORDS.test(host);
    const inLabel = new RegExp("(^|-)" + core + "(-|$)").test(siteLabel.toLowerCase()) && siteLabel.toLowerCase() !== core;
    if (inLabel && (!generic || loginish)) return { brand: core, real, reason: core + " inside the domain name" };
    if (subdomainLabels.some((label) => normalize(label) === core) && core.length >= 4 && (!generic || loginish)) {
      return { brand: core, real, reason: core + " as a subdomain of " + site };
    }
  }
  return null;
};

// A per-install secret; password hashes are HMACs under it, so the store
// says nothing about the passwords even if it were copied.
async function installKey() {
  const stored = await Shield.api.storage.local.get("installSalt");
  if (stored.installSalt) return stored.installSalt;
  const salt = Shield.hex(crypto.getRandomValues(new Uint8Array(32)));
  await Shield.api.storage.local.set({ installSalt: salt });
  return salt;
}
Shield.installSalt = installKey;

// Records that a password hash was used on a site; answers with the other
// sites it was used on before, which is what a phishing page looks like.
Shield.passwordSeen = async function passwordSeen(hash, site) {
  if (!/^[0-9a-f]{16,64}$/.test(hash) || !site) return { reused: [] };
  const stored = await Shield.api.storage.local.get("passwordSites");
  const map = stored.passwordSites || {};
  const sites = map[hash] || [];
  const reused = sites.filter((known) => known !== site && !sameFamily(known, site));
  if (!sites.includes(site)) {
    sites.push(site);
    map[hash] = sites.slice(-8);
    const keys = Object.keys(map);
    if (keys.length > 500) {
      // Evict the hash associated with the fewest sites (least useful to keep).
      let minKey = keys[0];
      let minLen = (map[keys[0]] || []).length;
      for (const k of keys) {
        if ((map[k] || []).length < minLen) { minKey = k; minLen = (map[k] || []).length; }
      }
      delete map[minKey];
    }
    await Shield.api.storage.local.set({ passwordSites: map });
  }
  return { reused };
};

function sameFamily(left, right) {
  for (const brand of Shield.BRANDS) {
    if (belongsTo(left, brand) && belongsTo(right, brand)) return true;
  }
  return false;
}

Shield.forgetPasswords = async function forgetPasswords() {
  await Shield.api.storage.local.remove("passwordSites");
};

// Have I Been Pwned's range API: five hex characters of the SHA-1 go out,
// the matching suffixes come back, the match is made here.
Shield.breachCount = async function breachCount(prefix, suffix) {
  if (!/^[0-9A-F]{5}$/.test(prefix) || !/^[0-9A-F]{35}$/.test(suffix)) throw new Error("bad hash");
  const response = await fetch("https://api.pwnedpasswords.com/range/" + prefix, { headers: { "Add-Padding": "true" }, credentials: "omit", cache: "no-store" });
  if (!response.ok) throw new Error("breach service answered " + response.status);
  const text = await response.text();
  for (const line of text.split("\n")) {
    const [rest, count] = line.trim().split(":");
    if (rest === suffix) return parseInt(count, 10) || 0;
  }
  return 0;
};

// Have I Been Pwned's public list of breaches, asked by domain (no key, no
// account). Kept for a week per site; the site's own name is all that leaves.
const BREACH_HISTORY_TTL = 7 * 24 * 60 * 60 * 1000;
Shield.breachHistory = async function breachHistory(site) {
  const stored = await Shield.api.storage.local.get("breachHistory");
  const history = stored.breachHistory || {};
  const cached = history[site];
  if (cached && Date.now() - cached.at < BREACH_HISTORY_TTL) return cached.breaches;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 8000);
  try {
    // PRIVACY: site domain is sent to HIBP to check for known breaches. See privacy settings to disable.
    const response = await fetch("https://haveibeenpwned.com/api/v3/breaches?domain=" + encodeURIComponent(site), {
      headers: { "user-agent": "noah-shield" },
      credentials: "omit",
      cache: "no-store",
      signal: controller.signal,
    });
    if (!response.ok) throw new Error("the breach list answered " + response.status);
    const breaches = Shield.summarizeBreaches(await response.json());
    // Only the newest 200 sites are kept, so the cache cannot grow without end.
    const entries = Object.entries(history).sort((left, right) => right[1].at - left[1].at).slice(0, 199);
    const next = Object.fromEntries(entries);
    next[site] = { at: Date.now(), breaches };
    await Shield.api.storage.local.set({ breachHistory: next });
    return breaches;
  } finally {
    clearTimeout(timer);
  }
};

Shield.summarizeBreaches = function summarizeBreaches(list) {
  if (!Array.isArray(list)) return [];
  return list
    .map((breach) => ({
      name: String(breach.Name || breach.Title || ""),
      date: String(breach.BreachDate || ""),
      count: Number(breach.PwnCount) || 0,
      classes: Array.isArray(breach.DataClasses) ? breach.DataClasses.map(String).slice(0, 8) : [],
      verified: breach.IsVerified !== false,
    }))
    .filter((breach) => breach.name)
    .sort((left, right) => right.date.localeCompare(left.date))
    .slice(0, 12);
};

Shield.breachedEmail = async function breachedEmail(email, key) {
  if (!key) throw new Error("an HIBP API key is needed for email checks (haveibeenpwned.com/API/Key)");
  const response = await fetch("https://haveibeenpwned.com/api/v3/breachedaccount/" + encodeURIComponent(email) + "?truncateResponse=true", {
    headers: { "hibp-api-key": key, "user-agent": "noah-shield" },
    credentials: "omit",
  });
  if (response.status === 404) return [];
  if (!response.ok) throw new Error("HIBP answered " + response.status);
  const breaches = await response.json();
  return breaches.map((breach) => breach.Name);
};

const RISKY_EXTENSIONS = /\.(exe|msi|msix|scr|bat|cmd|com|pif|ps1|psm1|vbs|vbe|js|jse|wsf|wsh|hta|jar|apk|dmg|pkg|app|deb|rpm|run|sh|iso|img|lnk|reg|dll|cpl|msc|gadget|inf|docm|xlsm|pptm|xll|url|chm|7z|rar|zip|gz|tgz)$/i;
const ARCHIVE_EXTENSIONS = /\.(7z|rar|zip|gz|tgz|iso|img)$/i;

// Why a download deserves a second look, or null.
Shield.downloadConcern = function downloadConcern(item, settings) {
  const name = String(item.filename || item.url || "").split(/[\\/]/).pop();
  const match = RISKY_EXTENSIONS.exec(name);
  if (!match) return null;
  const source = Shield.siteOf(Shield.hostOf(item.referrer || item.url || ""));
  if (settings.privacy.trustedSites.includes(source)) return null;
  const reasons = [];
  if (ARCHIVE_EXTENSIONS.test(name)) reasons.push("an archive can hide any kind of file");
  else reasons.push("a file of this kind runs as a program when opened");
  if (/^http:/.test(item.url || "")) reasons.push("it came over plain http, so anyone on the way could have swapped it");
  if (item.mime && /text\/html|application\/octet-stream/.test(item.mime) && !ARCHIVE_EXTENSIONS.test(name)) reasons.push("the server did not say what kind of file it is");
  if (/\.(pdf|doc|docx|jpg|jpeg|png|mp4|mp3|txt)\.(exe|scr|com|bat|js|vbs|hta|lnk)$/i.test(name)) reasons.push("a double extension: it pretends to be a document");
  const lookalike = Shield.lookalike(Shield.hostOf(item.url || ""));
  if (lookalike) reasons.push("the site looks like " + lookalike.brand + " but is not");
  return { name, source, reasons };
};

// VirusTotal, with the key you give it: the URL is submitted and its latest
// analysis fetched. Nothing is sent without a key.
Shield.virusTotalUrl = async function virusTotalUrl(url, key) {
  if (!key) return { skipped: "no VirusTotal key" };
  const submit = await fetch("https://www.virustotal.com/api/v3/urls", {
    method: "POST",
    headers: { "x-apikey": key, "content-type": "application/x-www-form-urlencoded" },
    body: "url=" + encodeURIComponent(url),
    credentials: "omit",
  });
  if (!submit.ok) throw new Error("VirusTotal answered " + submit.status);
  const submitted = await submit.json();
  const id = submitted.data && submitted.data.id;
  for (let attempt = 0; attempt < 6; attempt++) {
    await new Promise((resolve) => setTimeout(resolve, 4000));
    const response = await fetch("https://www.virustotal.com/api/v3/analyses/" + id, { headers: { "x-apikey": key }, credentials: "omit" });
    if (!response.ok) continue;
    const analysis = await response.json();
    const attributes = analysis.data && analysis.data.attributes;
    if (attributes && attributes.status === "completed") return { stats: attributes.stats };
  }
  return { pending: true };
};
})();
