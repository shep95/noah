// The footprint page: what search engines around the world hold under a
// person's name, email or phone, drawn as a map, with a removal plan.
//
// Every request goes straight from this browser to the engine, without
// cookies or a referrer, at most four at a time. Results are parsed here
// with DOMParser; nothing is sent anywhere else. Translation, when on, uses
// Google's public translate endpoint, which is not an API Google promises
// to keep; when it fails the original text stays and a translate link is
// offered instead.
"use strict";

const byId = (id) => document.getElementById(id);
const settingsKey = "footprint";

// ---- engines ---------------------------------------------------------------
// Each engine answers with HTML a program can read. Google and Yandex answer
// programs with a puzzle, so they are not asked; their result pages are in
// the "look yourself" list instead.
const ENGINES = {
  duckduckgo: {
    name: "DuckDuckGo",
    url: (q, region) => `https://html.duckduckgo.com/html/?q=${encodeURIComponent(q)}${region ? "&kl=" + region.ddg : ""}`,
    parse(doc) {
      return Array.from(doc.querySelectorAll(".result")).map((node) => {
        const anchor = node.querySelector("a.result__a");
        if (!anchor) return null;
        let href = anchor.getAttribute("href") || "";
        try {
          const wrapped = new URL(href, "https://duckduckgo.com");
          const inner = wrapped.searchParams.get("uddg");
          if (inner) href = inner;
          else href = wrapped.href;
        } catch { return null; }
        return { url: href, title: anchor.textContent.trim(), snippet: (node.querySelector(".result__snippet") || {}).textContent || "" };
      }).filter(Boolean);
    },
  },
  bing: {
    name: "Bing",
    url: (q, region) => `https://www.bing.com/search?q=${encodeURIComponent(q)}${region ? "&cc=" + region.cc + "&setlang=" + region.lang : ""}`,
    parse(doc) {
      return Array.from(doc.querySelectorAll("#b_results > li.b_algo")).map((node) => {
        const anchor = node.querySelector("h2 a");
        if (!anchor || !anchor.href) return null;
        return { url: anchor.href, title: anchor.textContent.trim(), snippet: (node.querySelector(".b_caption p, .b_lineclamp2, .b_lineclamp3") || {}).textContent || "" };
      }).filter(Boolean);
    },
  },
  brave: {
    name: "Brave",
    url: (q, region) => `https://search.brave.com/search?q=${encodeURIComponent(q)}${region ? "&country=" + region.cc : ""}&source=web`,
    parse(doc) {
      return Array.from(doc.querySelectorAll("#results .snippet")).map((node) => {
        const anchor = node.querySelector("a[href^='http']");
        if (!anchor) return null;
        return { url: anchor.href, title: (node.querySelector(".title") || anchor).textContent.trim(), snippet: (node.querySelector(".snippet-description, .snippet-content") || {}).textContent || "" };
      }).filter(Boolean);
    },
  },
  mojeek: {
    name: "Mojeek",
    url: (q) => `https://www.mojeek.com/search?q=${encodeURIComponent(q)}`,
    parse(doc) {
      return Array.from(doc.querySelectorAll("ul.results-standard > li")).map((node) => {
        const anchor = node.querySelector("a.ob, h2 a");
        if (!anchor || !anchor.href) return null;
        return { url: anchor.href, title: anchor.textContent.trim(), snippet: (node.querySelector("p.s") || {}).textContent || "" };
      }).filter(Boolean);
    },
  },
  baidu: {
    name: "Baidu",
    url: (q) => `https://www.baidu.com/s?wd=${encodeURIComponent(q)}&ie=utf-8`,
    parse(doc) {
      return Array.from(doc.querySelectorAll(".result, .c-container")).map((node) => {
        const anchor = node.querySelector("h3 a");
        if (!anchor || !anchor.href) return null;
        // Baidu wraps every link in a redirect; the shown address is the host.
        const shown = (node.querySelector(".c-showurl, .source_1Vdff, .siteLink_9TPP3") || {}).textContent || "";
        const host = shown.trim().split(/[\s/]/)[0];
        const url = /^[\w.-]+\.[a-z]{2,}$/i.test(host) ? "https://" + host + "/" : anchor.href;
        return { url, title: anchor.textContent.trim(), snippet: (node.querySelector(".c-abstract, .content-right_2s-H4, span[class*='content-right']") || {}).textContent || "" };
      }).filter(Boolean);
    },
  },
};

// Countries the engines can be asked from. `ddg` is DuckDuckGo's region
// code, `cc` and `lang` are Bing's and Brave's.
const REGIONS = [
  { name: "United States", ddg: "us-en", cc: "US", lang: "en" },
  { name: "United Kingdom", ddg: "uk-en", cc: "GB", lang: "en" },
  { name: "Canada", ddg: "ca-en", cc: "CA", lang: "en" },
  { name: "Australia", ddg: "au-en", cc: "AU", lang: "en" },
  { name: "India", ddg: "in-en", cc: "IN", lang: "en" },
  { name: "South Africa", ddg: "za-en", cc: "ZA", lang: "en" },
  { name: "Germany", ddg: "de-de", cc: "DE", lang: "de" },
  { name: "France", ddg: "fr-fr", cc: "FR", lang: "fr" },
  { name: "Spain", ddg: "es-es", cc: "ES", lang: "es" },
  { name: "Italy", ddg: "it-it", cc: "IT", lang: "it" },
  { name: "Netherlands", ddg: "nl-nl", cc: "NL", lang: "nl" },
  { name: "Poland", ddg: "pl-pl", cc: "PL", lang: "pl" },
  { name: "Sweden", ddg: "se-sv", cc: "SE", lang: "sv" },
  { name: "Turkey", ddg: "tr-tr", cc: "TR", lang: "tr" },
  { name: "Russia", ddg: "ru-ru", cc: "RU", lang: "ru" },
  { name: "Brazil", ddg: "br-pt", cc: "BR", lang: "pt" },
  { name: "Mexico", ddg: "mx-es", cc: "MX", lang: "es" },
  { name: "Argentina", ddg: "ar-es", cc: "AR", lang: "es" },
  { name: "Japan", ddg: "jp-jp", cc: "JP", lang: "ja" },
  { name: "China", ddg: "cn-zh", cc: "CN", lang: "zh" },
  { name: "Korea", ddg: "kr-kr", cc: "KR", lang: "ko" },
  { name: "Indonesia", ddg: "id-id", cc: "ID", lang: "id" },
  { name: "Saudi Arabia", ddg: "xa-ar", cc: "SA", lang: "ar" },
  { name: "Egypt", ddg: "xa-ar", cc: "EG", lang: "ar" },
];

const UI_LANGUAGES = [["en", "English"], ["es", "Español"], ["pt", "Português"], ["fr", "Français"], ["de", "Deutsch"], ["it", "Italiano"], ["nl", "Nederlands"], ["pl", "Polski"], ["tr", "Türkçe"], ["ru", "Русский"], ["ar", "العربية"], ["hi", "हिन्दी"], ["ja", "日本語"], ["ko", "한국어"], ["zh", "中文"]];

// ---- kinds of places ---------------------------------------------------------
// Where a mention lives says how bad it is: a data broker sells the whole
// profile; a document may carry an address; a forum post is what you said.
const KINDS = [
  { id: "broker", label: "people-search & data brokers", risk: "high", color: "#e8b4a8", hosts: ["spokeo.com", "whitepages.com", "beenverified.com", "intelius.com", "truthfinder.com", "instantcheckmate.com", "peoplefinders.com", "mylife.com", "radaris.com", "fastpeoplesearch.com", "truepeoplesearch.com", "peekyou.com", "usphonebook.com", "clustrmaps.com", "nuwber.com", "thatsthem.com", "pipl.com", "zoominfo.com", "rocketreach.co", "192.com", "spytox.com", "zabasearch.com", "peoplelooker.com", "checkpeople.com", "searchpeoplefree.com", "familytreenow.com", "cyberbackgroundchecks.com", "411.com", "anywho.com", "addresses.com", "yasni.com", "yasni.de", "locatefamily.com", "peoplesearchnow.com", "publicrecords.com", "backgroundalert.com", "advancedbackgroundchecks.com", "ussearch.com", "privateeye.com", "persopo.com", "signalhire.com", "lusha.com", "apollo.io", "contactout.com", "hunter.io", "snov.io", "aeroleads.com", "dehashed.com"] },
  { id: "social", label: "social profiles", risk: "medium", color: "#96becc", hosts: ["facebook.com", "instagram.com", "x.com", "twitter.com", "tiktok.com", "threads.net", "snapchat.com", "pinterest.com", "reddit.com", "youtube.com", "twitch.tv", "vk.com", "ok.ru", "weibo.com", "bsky.app", "mastodon.social", "tumblr.com", "flickr.com", "quora.com", "discord.com", "telegram.me", "t.me", "onlyfans.com", "linktr.ee"] },
  { id: "work", label: "work & professional", risk: "medium", color: "#a9cf9f", hosts: ["linkedin.com", "github.com", "gitlab.com", "stackoverflow.com", "crunchbase.com", "angel.co", "wellfound.com", "glassdoor.com", "indeed.com", "xing.com", "researchgate.net", "orcid.org", "scholar.google.com", "academia.edu", "behance.net", "dribbble.com", "medium.com", "dev.to", "substack.com", "about.me", "gravatar.com", "opencorporates.com", "sec.gov", "companieshouse.gov.uk", "bizapedia.com", "opengovus.com", "dnb.com", "manta.com"] },
  { id: "records", label: "public & court records", risk: "high", color: "#dcc896", hosts: ["courtlistener.com", "unicourt.com", "trellis.law", "pacermonitor.com", "judyrecords.com", "mugshots.com", "arrests.org", "jailbase.com", "casetext.com", "law360.com", "justia.com", "leagle.com", "casemine.com", "vinelink.com", "gov", "propertyshark.com", "zillow.com", "realtor.com", "redfin.com", "homemetry.com", "neighborwho.com", "ownerly.com", "blockshopper.com", "votersrecords.com", "voterrecords.com", "fec.gov", "opensecrets.org", "followthemoney.org"] },
  { id: "breach", label: "breaches & pastes", risk: "high", color: "#e69696", hosts: ["haveibeenpwned.com", "pastebin.com", "ghostbin.com", "throwbin.io", "doxbin.com", "doxbin.net", "leakcheck.io", "leak-lookup.com", "intelx.io", "breachforums.st", "psbdmp.ws", "scylla.sh"] },
  { id: "documents", label: "documents & files", risk: "medium", color: "#c9b7e6", hosts: ["scribd.com", "issuu.com", "docplayer.net", "pdfslide.net", "yumpu.com", "slideshare.net", "dokumen.pub", "studylib.net", "coursehero.com", "docsity.com", "archive.org", "drive.google.com", "docs.google.com", "dropbox.com"] },
  { id: "news", label: "news & articles", risk: "low", color: "#cfd6cc", hosts: ["nytimes.com", "washingtonpost.com", "bbc.co.uk", "bbc.com", "theguardian.com", "reuters.com", "apnews.com", "cnn.com", "foxnews.com", "nbcnews.com", "abcnews.go.com", "cbsnews.com", "usatoday.com", "forbes.com", "bloomberg.com", "wsj.com", "patch.com", "news-press.com", "legacy.com", "obituaries", "dignitymemorial.com", "findagrave.com", "newspapers.com"] },
  { id: "other", label: "other pages", risk: "low", color: "#9aa298", hosts: [] },
];

const REMOVAL = {
  // Search engines: not the source, but the index; each has a form.
  google: { name: "Google (results about you)", url: "https://myactivity.google.com/results-about-you", note: "removes results that show your home address, phone or email from Google's index" },
  bing: { name: "Bing (content removal)", url: "https://www.bing.com/webmasters/tools/contentremoval", note: "removes a page from Bing's index once the page itself is gone" },
  yandex: { name: "Yandex (removal)", url: "https://yandex.com/support/webmaster/controlling-robot/delete-page.html", note: "Yandex's removal path" },
  // Where the site is the source.
  brokers: {
    "spokeo.com": "https://www.spokeo.com/optout", "whitepages.com": "https://www.whitepages.com/suppression-requests", "beenverified.com": "https://www.beenverified.com/app/optout/search", "intelius.com": "https://www.intelius.com/opt-out/submit/", "truthfinder.com": "https://www.truthfinder.com/opt-out/", "instantcheckmate.com": "https://www.instantcheckmate.com/opt-out/", "peoplefinders.com": "https://www.peoplefinders.com/opt-out", "mylife.com": "https://www.mylife.com/ccpa/index.pubview", "radaris.com": "https://radaris.com/control/privacy", "fastpeoplesearch.com": "https://www.fastpeoplesearch.com/removal", "truepeoplesearch.com": "https://www.truepeoplesearch.com/removal", "peekyou.com": "https://www.peekyou.com/about/contact/optout/", "usphonebook.com": "https://www.usphonebook.com/opt-out", "clustrmaps.com": "https://clustrmaps.com/bl/opt-out", "nuwber.com": "https://nuwber.com/removal/link", "thatsthem.com": "https://thatsthem.com/optout", "pipl.com": "https://pipl.com/personal-information-removal-request", "zoominfo.com": "https://www.zoominfo.com/privacy-center/update/profile", "rocketreach.co": "https://rocketreach.co/claim-profile", "192.com": "https://www.192.com/c01/removal", "zabasearch.com": "https://www.zabasearch.com/block_records/", "peoplelooker.com": "https://www.peoplelooker.com/f/optout/search", "checkpeople.com": "https://www.checkpeople.com/opt-out", "searchpeoplefree.com": "https://www.searchpeoplefree.com/opt-out", "familytreenow.com": "https://www.familytreenow.com/optout", "cyberbackgroundchecks.com": "https://www.cyberbackgroundchecks.com/removal", "411.com": "https://www.411.com/privacy/manage", "anywho.com": "https://www.anywho.com/optout", "yasni.com": "https://www.yasni.com/service/optout", "spytox.com": "https://www.spytox.com/opt-out", "locatefamily.com": "https://www.locatefamily.com/removal.html", "ussearch.com": "https://www.ussearch.com/opt-out/submit/", "signalhire.com": "https://www.signalhire.com/privacy-request", "lusha.com": "https://www.lusha.com/privacy-request/", "apollo.io": "https://www.apollo.io/privacy-policy/remove", "contactout.com": "https://contactout.com/optout", "hunter.io": "https://hunter.io/claim", "dehashed.com": "https://www.dehashed.com/remove", "mugshots.com": "https://mugshots.com/remove", "arrests.org": "https://arrests.org/opt-out/", "propertyshark.com": "https://www.propertyshark.com/mason/info/contact.html", "opencorporates.com": "https://opencorporates.com/info/privacy_policy",
  },
  social: {
    "facebook.com": "https://www.facebook.com/settings/?tab=privacy", "instagram.com": "https://www.instagram.com/accounts/privacy_and_security/", "x.com": "https://x.com/settings/privacy_and_safety", "twitter.com": "https://x.com/settings/privacy_and_safety", "tiktok.com": "https://www.tiktok.com/setting/privacy", "linkedin.com": "https://www.linkedin.com/psettings/profile-visibility", "reddit.com": "https://www.reddit.com/settings/privacy", "youtube.com": "https://www.youtube.com/account_privacy", "pinterest.com": "https://www.pinterest.com/settings/privacy", "github.com": "https://github.com/settings/profile", "snapchat.com": "https://accounts.snapchat.com/accounts/privacy", "quora.com": "https://www.quora.com/settings/privacy", "vk.com": "https://vk.com/settings?act=privacy",
  },
};

// ---- state -------------------------------------------------------------------
let identity = null;
let results = [];
let hosts = new Map();
let stopping = false;
let filed = {};
let colors = {};
let translateCache = new Map();

// ---- helpers -----------------------------------------------------------------
function split(text) {
  return String(text || "").split(/[,;\n]/).map((part) => part.trim()).filter(Boolean);
}

function hostOf(url) {
  try { return new URL(url).hostname.toLowerCase().replace(/^www\./, ""); } catch { return ""; }
}

function siteOf(host) {
  const parts = host.split(".");
  if (parts.length <= 2) return host;
  const second = parts[parts.length - 2];
  // co.uk, com.au, com.br and the like keep three labels.
  if (/^(co|com|org|net|gov|edu|ac)$/.test(second) && parts[parts.length - 1].length === 2) return parts.slice(-3).join(".");
  return parts.slice(-2).join(".");
}

function kindOf(host, url, text) {
  const site = siteOf(host);
  for (const kind of KINDS) {
    if (kind.hosts.some((known) => site === known || host.endsWith("." + known) || (known === "gov" && /\.gov(\.[a-z]{2})?$/.test(host)))) return kind;
  }
  if (/\.(pdf|docx?|xlsx?|pptx?|csv)(\?|$)/i.test(url)) return KINDS.find((kind) => kind.id === "documents");
  if (/\b(address|phone|age \d\d|relatives|background check|public records|people search)\b/i.test(text)) return KINDS.find((kind) => kind.id === "broker");
  if (/\b(news|times|post|herald|gazette|tribune|journal|daily|press)\b/.test(site)) return KINDS.find((kind) => kind.id === "news");
  return KINDS.find((kind) => kind.id === "other");
}

// A crude script check is enough to know a result is not in the reader's
// language before the translator (which may be unavailable) says more.
function scriptOf(text) {
  if (/[぀-ヿ]/.test(text)) return "ja";
  if (/[가-힯]/.test(text)) return "ko";
  if (/[一-鿿]/.test(text)) return "zh";
  if (/[؀-ۿ]/.test(text)) return "ar";
  if (/[Ѐ-ӿ]/.test(text)) return "ru";
  if (/[֐-׿]/.test(text)) return "he";
  if (/[฀-๿]/.test(text)) return "th";
  if (/[ऀ-ॿ]/.test(text)) return "hi";
  return "";
}

function fetchWithTimeout(url, ms, options = {}) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), ms);
  return fetch(url, { credentials: "omit", referrerPolicy: "no-referrer", redirect: "follow", cache: "no-store", signal: controller.signal, ...options }).finally(() => clearTimeout(timer));
}

async function translate(text, to) {
  const key = to + "\n" + text;
  if (translateCache.has(key)) return translateCache.get(key);
  const url = `https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl=${encodeURIComponent(to)}&dt=t&q=${encodeURIComponent(text.slice(0, 900))}`;
  try {
    const response = await fetchWithTimeout(url, 8000);
    if (!response.ok) throw new Error("answered " + response.status);
    const data = await response.json();
    const translated = (data[0] || []).map((part) => part[0]).join("");
    const detected = String(data[2] || "");
    const answer = { text: translated, from: detected };
    translateCache.set(key, answer);
    return answer;
  } catch {
    translateCache.set(key, null);
    return null;
  }
}

// ---- the search --------------------------------------------------------------
function queriesFor(id) {
  const quoted = (value) => `"${value.replace(/"/g, "")}"`;
  const queries = [];
  const primary = id.name ? quoted(id.name) : null;
  if (primary) {
    queries.push({ q: primary, about: "name" });
    for (const context of id.context) queries.push({ q: `${primary} ${quoted(context)}`, about: "name + " + context });
    queries.push({ q: `${primary} (address OR phone OR "date of birth" OR relatives)`, about: "name with personal details" });
    queries.push({ q: `${primary} (filetype:pdf OR filetype:doc OR filetype:xls)`, about: "documents naming you" });
    queries.push({ q: `${primary} (site:linkedin.com OR site:facebook.com OR site:instagram.com OR site:x.com OR site:tiktok.com OR site:github.com OR site:reddit.com)`, about: "profiles" });
    queries.push({ q: `${primary} (site:spokeo.com OR site:whitepages.com OR site:fastpeoplesearch.com OR site:truepeoplesearch.com OR site:radaris.com OR site:mylife.com OR site:beenverified.com)`, about: "people-search sites" });
  }
  for (const alias of id.aliases) queries.push({ q: quoted(alias), about: "alias " + alias });
  for (const email of id.emails) queries.push({ q: quoted(email), about: "email " + email });
  for (const phone of id.phones) {
    const digits = phone.replace(/\D/g, "");
    queries.push({ q: quoted(phone), about: "phone " + phone });
    if (digits.length >= 10) queries.push({ q: quoted(digits.slice(-10).replace(/(\d{3})(\d{3})(\d{4})/, "$1-$2-$3")), about: "phone " + phone });
  }
  return queries;
}

function planFor(id, world) {
  const plan = [];
  const queries = queriesFor(id);
  for (const query of queries) {
    for (const engineId of ["duckduckgo", "bing", "brave", "mojeek"]) plan.push({ engineId, query, region: null });
  }
  if (world) {
    const primary = queries.find((query) => query.about === "name") || queries[0];
    if (primary) {
      for (const region of REGIONS) {
        plan.push({ engineId: "duckduckgo", query: primary, region });
        plan.push({ engineId: "bing", query: primary, region });
      }
      plan.push({ engineId: "baidu", query: primary, region: REGIONS.find((region) => region.cc === "CN") });
    }
  }
  return plan;
}

async function runOne(step) {
  const engine = ENGINES[step.engineId];
  const url = engine.url(step.query.q, step.region);
  const response = await fetchWithTimeout(url, 12000, { headers: { accept: "text/html,application/xhtml+xml;q=0.9,*/*;q=0.5" } });
  if (!response.ok) throw new Error(engine.name + " answered " + response.status);
  const html = await response.text();
  if (/captcha|unusual traffic|verify you are human/i.test(html.slice(0, 20000)) && !/result/.test(html)) throw new Error(engine.name + " asked for a puzzle");
  const doc = new DOMParser().parseFromString(html, "text/html");
  return engine.parse(doc).map((item) => ({ ...item, engine: engine.name, region: step.region ? step.region.name : "your country", about: step.query.about }));
}

function add(item) {
  let url;
  try { url = new URL(item.url); } catch { return; }
  if (!/^https?:$/.test(url.protocol)) return;
  url.hash = "";
  const key = url.href.replace(/\/$/, "");
  const host = hostOf(key);
  if (!host || /duckduckgo\.com|bing\.com|brave\.com|mojeek\.com|baidu\.com/.test(host)) return;
  let existing = results.find((result) => result.key === key);
  if (existing) {
    if (!existing.seen.includes(item.engine + " · " + item.region)) existing.seen.push(item.engine + " · " + item.region);
    if (!existing.about.includes(item.about)) existing.about.push(item.about);
    return;
  }
  const text = (item.title + " " + item.snippet).replace(/\s+/g, " ").trim();
  const kind = kindOf(host, key, text);
  existing = { key, url: key, host, site: siteOf(host), title: item.title || key, snippet: (item.snippet || "").replace(/\s+/g, " ").trim().slice(0, 300), kind: kind.id, risk: kind.risk, lang: scriptOf(text), seen: [item.engine + " · " + item.region], about: [item.about], translated: null };
  results.push(existing);
  let entry = hosts.get(existing.site);
  if (!entry) { entry = { site: existing.site, kind: kind.id, risk: kind.risk, results: [] }; hosts.set(existing.site, entry); }
  entry.results.push(existing);
}

async function search() {
  identity = {
    name: byId("f-name").value.trim(),
    aliases: split(byId("f-aliases").value),
    emails: split(byId("f-emails").value),
    phones: split(byId("f-phones").value),
    context: split(byId("f-context").value),
    lang: byId("f-lang").value,
  };
  if (!identity.name && !identity.aliases.length && !identity.emails.length && !identity.phones.length) {
    byId("f-status").textContent = "give it at least a name, a handle, an email or a phone number";
    return;
  }
  if (byId("f-remember").checked) await Shield.api.storage.local.set({ [settingsKey + ".identity"]: identity });
  else await Shield.api.storage.local.remove(settingsKey + ".identity");

  results = [];
  hosts = new Map();
  stopping = false;
  byId("f-go").disabled = true;
  byId("f-stop").hidden = false;
  byId("f-progress").hidden = false;
  const plan = planFor(identity, byId("f-world").checked);
  let done = 0;
  const failures = [];
  const status = () => {
    byId("f-status").textContent = `${done} of ${plan.length} searches · ${results.length} pages on ${hosts.size} sites` + (failures.length ? ` · ${failures.length} engines did not answer` : "");
    byId("f-progress").firstElementChild.style.width = Math.round((done / plan.length) * 100) + "%";
  };
  status();
  // Four at a time keeps the engines from seeing a flood from one address.
  const queue = plan.slice();
  await Promise.all(Array.from({ length: 4 }, async () => {
    while (queue.length && !stopping) {
      const step = queue.shift();
      try {
        for (const item of await runOne(step)) add(item);
      } catch (error) {
        failures.push(ENGINES[step.engineId].name + ": " + String(error.message || error));
      }
      done++;
      status();
      if (done % 6 === 0) draw();
    }
  }));
  byId("f-go").disabled = false;
  byId("f-stop").hidden = true;
  byId("f-progress").hidden = true;
  if (byId("f-translate").checked) await translateAll();
  finish(failures);
}

async function translateAll() {
  const to = identity.lang;
  const foreign = results.filter((result) => result.lang && result.lang !== to);
  let count = 0;
  for (const result of foreign) {
    if (stopping) break;
    byId("f-status").textContent = `translating ${++count} of ${foreign.length}…`;
    const answer = await translate(result.title + "\n" + result.snippet, to);
    if (answer && answer.text) {
      result.translated = answer.text;
      if (answer.from) result.lang = answer.from;
    }
  }
}

function finish(failures) {
  const high = results.filter((result) => result.risk === "high").length;
  byId("f-status").textContent = `${results.length} pages on ${hosts.size} sites` + (high ? `, ${high} that carry personal details` : "") + (failures.length ? ` · did not answer: ${Array.from(new Set(failures.map((line) => line.split(":")[0]))).join(", ")}` : "");
  byId("map-section").hidden = false;
  byId("removal").hidden = false;
  byId("checks").hidden = false;
  buildGraph();
  draw();
  renderRemoval();
  renderChecks();
  Shield.api.storage.local.set({ [settingsKey + ".last"]: { at: Date.now(), results: results.slice(0, 500) } }).catch(() => {});
}

// ---- the map -------------------------------------------------------------------
const canvas = byId("map");
const context = canvas.getContext("2d");
let nodes = [];
let edges = [];
let dragging = null;
let hovered = null;
let animation = null;
let pinned = new Set();

function buildGraph() {
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  const centre = { id: "you", label: identity.name || identity.aliases[0] || identity.emails[0] || "you", type: "you", x: width / 2, y: height / 2, r: 22 };
  nodes = [centre];
  edges = [];
  const used = KINDS.filter((kind) => Array.from(hosts.values()).some((entry) => entry.kind === kind.id));
  used.forEach((kind, index) => {
    const angle = (index / used.length) * Math.PI * 2 - Math.PI / 2;
    const node = { id: "kind:" + kind.id, label: kind.label, type: "kind", kind: kind.id, x: width / 2 + Math.cos(angle) * Math.min(width, height) * 0.24, y: height / 2 + Math.sin(angle) * Math.min(width, height) * 0.24, r: 12 };
    nodes.push(node);
    edges.push({ from: centre, to: node, length: Math.min(width, height) * 0.24 });
    const members = Array.from(hosts.values()).filter((entry) => entry.kind === kind.id);
    members.forEach((entry, position) => {
      const spread = angle + ((position + 0.5) / members.length - 0.5) * (Math.PI * 2 / used.length) * 0.9;
      const distance = Math.min(width, height) * (0.38 + (position % 3) * 0.05);
      const leaf = { id: "site:" + entry.site, label: entry.site, type: "site", kind: kind.id, entry, x: width / 2 + Math.cos(spread) * distance, y: height / 2 + Math.sin(spread) * distance, r: Math.min(18, 6 + Math.sqrt(entry.results.length) * 3) };
      nodes.push(leaf);
      edges.push({ from: node, to: leaf, length: Math.min(width, height) * 0.16 });
    });
  });
  pinned = new Set(["you"]);
  renderLegend();
  byId("map-summary").textContent = `${used.length} kinds of place, ${hosts.size} sites`;
  settle(240);
}

function colorFor(kindId) {
  return colors[kindId] || (KINDS.find((kind) => kind.id === kindId) || KINDS[KINDS.length - 1]).color;
}

function renderLegend() {
  const legend = byId("legend");
  legend.replaceChildren();
  for (const kind of KINDS) {
    const members = Array.from(hosts.values()).filter((entry) => entry.kind === kind.id);
    if (!members.length) continue;
    const label = document.createElement("label");
    const input = document.createElement("input");
    input.type = "color";
    input.value = colorFor(kind.id);
    input.addEventListener("input", () => {
      colors[kind.id] = input.value;
      Shield.api.storage.local.set({ [settingsKey + ".colors"]: colors }).catch(() => {});
      draw();
    });
    const count = document.createElement("span");
    count.className = "count";
    count.textContent = `${members.length} · ${members.reduce((sum, entry) => sum + entry.results.length, 0)} pages`;
    label.append(input, kind.label, count);
    label.addEventListener("click", (event) => { if (event.target !== input) select(nodes.find((node) => node.id === "kind:" + kind.id)); });
    legend.append(label);
  }
}

// A few rounds of springs and repulsion, run when the graph is built or
// untangled, and one round per frame while something is dragged.
function step() {
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  for (const node of nodes) { node.vx = node.vx || 0; node.vy = node.vy || 0; }
  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      const a = nodes[i], b = nodes[j];
      let dx = b.x - a.x, dy = b.y - a.y;
      let distance = Math.hypot(dx, dy) || 0.01;
      const minimum = a.r + b.r + 26;
      if (distance < minimum * 2.2) {
        const push = ((minimum * 2.2 - distance) / distance) * 0.08;
        dx *= push; dy *= push;
        a.vx -= dx; a.vy -= dy; b.vx += dx; b.vy += dy;
      }
    }
  }
  for (const edge of edges) {
    const dx = edge.to.x - edge.from.x, dy = edge.to.y - edge.from.y;
    const distance = Math.hypot(dx, dy) || 0.01;
    const pull = ((distance - edge.length) / distance) * 0.04;
    edge.from.vx += dx * pull; edge.from.vy += dy * pull;
    edge.to.vx -= dx * pull; edge.to.vy -= dy * pull;
  }
  for (const node of nodes) {
    if (pinned.has(node.id) || node === dragging) { node.vx = node.vy = 0; continue; }
    node.vx *= 0.82; node.vy *= 0.82;
    node.x = Math.max(node.r + 4, Math.min(width - node.r - 4, node.x + node.vx));
    node.y = Math.max(node.r + 4, Math.min(height - node.r - 4, node.y + node.vy));
  }
}

function settle(rounds) {
  cancelAnimationFrame(animation);
  let left = rounds;
  const tick = () => {
    step();
    draw();
    if (--left > 0) animation = requestAnimationFrame(tick);
  };
  animation = requestAnimationFrame(tick);
}

function draw() {
  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth, height = canvas.clientHeight;
  if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) {
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
  }
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.clearRect(0, 0, width, height);
  if (!nodes.length) return;
  context.lineWidth = 1;
  for (const edge of edges) {
    context.strokeStyle = colorFor(edge.to.kind || edge.from.kind) + "55";
    context.beginPath();
    context.moveTo(edge.from.x, edge.from.y);
    context.lineTo(edge.to.x, edge.to.y);
    context.stroke();
  }
  for (const node of nodes) {
    const color = node.type === "you" ? "#f1f4ef" : colorFor(node.kind);
    context.beginPath();
    context.arc(node.x, node.y, node.r, 0, Math.PI * 2);
    context.fillStyle = node.type === "site" ? color + "cc" : color;
    context.fill();
    if (node === hovered) { context.strokeStyle = "#f1f4ef"; context.lineWidth = 2; context.stroke(); context.lineWidth = 1; }
    if (node.type === "site" && node.entry.risk === "high") { context.strokeStyle = "#e8b4a8"; context.stroke(); }
    context.fillStyle = node.type === "you" ? "#070909" : "#d8ddd6";
    context.font = (node.type === "you" ? "600 13px" : node.type === "kind" ? "12px" : "11px") + " -apple-system, 'Segoe UI', system-ui, sans-serif";
    context.textAlign = "center";
    context.textBaseline = node.type === "you" ? "middle" : "top";
    const label = node.label.length > 28 ? node.label.slice(0, 26) + "…" : node.label;
    if (node.type === "you") context.fillText(label, node.x, node.y);
    else context.fillText(label, node.x, node.y + node.r + 4);
  }
}

function nodeAt(x, y) {
  for (let i = nodes.length - 1; i >= 0; i--) {
    const node = nodes[i];
    if (Math.hypot(node.x - x, node.y - y) <= node.r + 6) return node;
  }
  return null;
}

function pointOf(event) {
  const rect = canvas.getBoundingClientRect();
  const source = event.touches ? event.touches[0] : event;
  return { x: source.clientX - rect.left, y: source.clientY - rect.top };
}

function showTip(node, point) {
  const tip = byId("tip");
  if (!node) { tip.hidden = true; return; }
  tip.hidden = false;
  let body;
  if (node.type === "you") body = "you, as the engines were asked about";
  else if (node.type === "kind") {
    const kind = KINDS.find((kind) => kind.id === node.kind);
    const count = Array.from(hosts.values()).filter((entry) => entry.kind === kind.id).length;
    body = `${count} sites · ${kind.risk === "high" ? "these carry personal details" : kind.risk === "medium" ? "these carry what you posted or listed" : "mentions"}`;
  } else {
    const entry = node.entry;
    body = `${entry.results.length} page${entry.results.length === 1 ? "" : "s"} · seen by ${Array.from(new Set(entry.results.flatMap((result) => result.seen))).length} engine-country pairs · ${entry.risk === "high" ? "carries personal details" : entry.risk === "medium" ? "something you posted or listed" : "a mention"}` + (REMOVAL.brokers[entry.site] ? " · has an opt-out form" : "");
  }
  tip.innerHTML = "";
  const title = document.createElement("b");
  title.textContent = node.label;
  tip.append(title, body);
  const wrap = canvas.parentElement.getBoundingClientRect();
  tip.style.left = Math.min(point.x + 14, wrap.width - tip.offsetWidth - 8) + "px";
  tip.style.top = Math.min(point.y + 14, wrap.height - tip.offsetHeight - 8) + "px";
}

canvas.addEventListener("mousemove", (event) => {
  const point = pointOf(event);
  if (dragging) { dragging.x = point.x; dragging.y = point.y; step(); draw(); return; }
  const node = nodeAt(point.x, point.y);
  if (node !== hovered) { hovered = node; draw(); }
  showTip(node, point);
});
canvas.addEventListener("mouseleave", () => { hovered = null; showTip(null); draw(); });
canvas.addEventListener("mousedown", (event) => {
  const point = pointOf(event);
  dragging = nodeAt(point.x, point.y);
  if (dragging) { canvas.classList.add("dragging"); dragging.moved = false; }
});
window.addEventListener("mouseup", (event) => {
  if (!dragging) return;
  const point = pointOf(event);
  const node = dragging;
  dragging = null;
  canvas.classList.remove("dragging");
  pinned.add(node.id);
  if (Math.hypot(node.x - point.x, node.y - point.y) < 4) select(node);
  settle(40);
});
canvas.addEventListener("touchstart", (event) => {
  const point = pointOf(event);
  dragging = nodeAt(point.x, point.y);
  if (dragging) event.preventDefault();
}, { passive: false });
canvas.addEventListener("touchmove", (event) => {
  if (!dragging) return;
  event.preventDefault();
  const point = pointOf(event);
  dragging.x = point.x; dragging.y = point.y;
  step(); draw();
}, { passive: false });
canvas.addEventListener("touchend", () => {
  if (!dragging) return;
  const node = dragging;
  dragging = null;
  pinned.add(node.id);
  select(node);
  settle(40);
});
canvas.addEventListener("click", (event) => {
  const point = pointOf(event);
  const node = nodeAt(point.x, point.y);
  if (node && !dragging) select(node);
});
new ResizeObserver(() => { if (nodes.length) { draw(); } }).observe(canvas);
byId("map-reset").addEventListener("click", () => { pinned = new Set(["you"]); buildGraph(); });
byId("map-png").addEventListener("click", () => {
  const link = document.createElement("a");
  link.download = "noah-footprint.png";
  link.href = canvas.toDataURL("image/png");
  link.click();
});

// ---- detail ----------------------------------------------------------------------
function select(node) {
  if (!node) return;
  const section = byId("detail");
  section.hidden = false;
  const table = byId("detail-results");
  const actions = byId("detail-actions");
  table.replaceChildren();
  actions.replaceChildren();
  let list;
  if (node.type === "you") {
    byId("detail-title").textContent = "everything found";
    byId("detail-lede").textContent = `${results.length} pages on ${hosts.size} sites. Worst first.`;
    list = results.slice();
  } else if (node.type === "kind") {
    const kind = KINDS.find((kind) => kind.id === node.kind);
    byId("detail-title").textContent = kind.label;
    byId("detail-lede").textContent = kind.risk === "high" ? "Sites that collect and publish personal details. Each has a removal path in the plan below." : kind.risk === "medium" ? "Profiles and posts under your own control: tighten their privacy settings, or delete what you no longer want public." : "Mentions in passing; the page's author decides.";
    list = results.filter((result) => result.kind === kind.id);
  } else {
    const entry = node.entry;
    byId("detail-title").textContent = entry.site;
    byId("detail-lede").textContent = `${entry.results.length} page${entry.results.length === 1 ? "" : "s"} · ${Array.from(new Set(entry.results.flatMap((result) => result.seen))).join(", ")}`;
    list = entry.results.slice();
    const removal = removalFor(entry.site);
    if (removal) {
      const link = document.createElement("a");
      link.href = removal.url;
      link.target = "_blank";
      link.rel = "noopener noreferrer";
      link.className = "ghost";
      link.textContent = removal.label;
      actions.append(link);
    }
    const mail = document.createElement("button");
    mail.className = "ghost";
    mail.textContent = "write a deletion letter";
    mail.addEventListener("click", () => copyLetter(entry.site));
    actions.append(mail);
  }
  const order = { high: 0, medium: 1, low: 2 };
  list.sort((a, b) => order[a.risk] - order[b.risk] || a.site.localeCompare(b.site));
  const head = document.createElement("tr");
  head.innerHTML = "<th>page</th><th>why it matters</th><th>seen by</th>";
  table.append(head);
  for (const result of list.slice(0, 200)) {
    const row = document.createElement("tr");
    const page = document.createElement("td");
    page.className = "url";
    const link = document.createElement("a");
    link.href = result.url;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = result.title;
    page.append(link);
    if (result.lang && result.lang !== identity.lang) {
      const tag = document.createElement("span");
      tag.className = "lang";
      tag.textContent = result.lang;
      page.append(tag);
      if (!result.translated) {
        const translateLink = document.createElement("a");
        translateLink.href = `https://translate.google.com/translate?sl=auto&tl=${encodeURIComponent(identity.lang)}&u=${encodeURIComponent(result.url)}`;
        translateLink.target = "_blank";
        translateLink.rel = "noopener noreferrer";
        translateLink.textContent = " translate";
        translateLink.className = "muted";
        page.append(translateLink);
      }
    }
    const snippet = document.createElement("div");
    snippet.className = result.translated ? "translated" : "muted";
    snippet.textContent = result.translated || result.snippet;
    page.append(snippet);
    const why = document.createElement("td");
    why.className = "risk-" + result.risk;
    why.textContent = (KINDS.find((kind) => kind.id === result.kind) || {}).label + " · found under " + result.about.join(", ");
    const seen = document.createElement("td");
    seen.className = "muted";
    seen.textContent = result.seen.slice(0, 4).join(", ") + (result.seen.length > 4 ? ` +${result.seen.length - 4}` : "");
    row.append(page, why, seen);
    table.append(row);
  }
  section.scrollIntoView({ behavior: "smooth", block: "start" });
}

function removalFor(site) {
  if (REMOVAL.brokers[site]) return { url: REMOVAL.brokers[site], label: "open " + site + "'s removal form", kind: "form" };
  if (REMOVAL.social[site]) return { url: REMOVAL.social[site], label: "open your " + site + " privacy settings", kind: "settings" };
  return null;
}

// ---- removal plan -----------------------------------------------------------------
function plan() {
  const rows = [];
  const seen = new Set();
  for (const entry of Array.from(hosts.values()).sort((a, b) => ({ high: 0, medium: 1, low: 2 })[a.risk] - ({ high: 0, medium: 1, low: 2 })[b.risk])) {
    const removal = removalFor(entry.site);
    if (removal) rows.push({ site: entry.site, url: removal.url, kind: removal.kind, pages: entry.results.length, how: removal.kind === "form" ? "fill the form with your name, the page's address and your email; some send a confirmation link" : "sign in and turn off public visibility, or delete the posts" });
    else if (entry.risk === "high") rows.push({ site: entry.site, url: "mailto:privacy@" + entry.site, kind: "letter", pages: entry.results.length, how: "no form; send the deletion letter to privacy@ or dpo@ the site" });
    seen.add(entry.site);
  }
  rows.push({ site: "google.com", url: REMOVAL.google.url, kind: "index", pages: 0, how: REMOVAL.google.note });
  rows.push({ site: "bing.com", url: REMOVAL.bing.url, kind: "index", pages: 0, how: REMOVAL.bing.note });
  return rows;
}

function renderRemoval() {
  const table = byId("removal-table");
  table.replaceChildren();
  const head = document.createElement("tr");
  head.innerHTML = "<th>site</th><th>what to do</th><th>filed</th>";
  table.append(head);
  for (const row of plan()) {
    const tr = document.createElement("tr");
    const site = document.createElement("td");
    site.textContent = row.site + (row.pages ? ` (${row.pages})` : "");
    const how = document.createElement("td");
    const link = document.createElement("a");
    link.href = row.url;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = row.kind === "letter" ? "email " + row.url.replace("mailto:", "") : row.url.replace(/^https?:\/\/(www\.)?/, "").slice(0, 60);
    how.append(link, document.createTextNode(" · " + row.how));
    const done = document.createElement("td");
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = Boolean(filed[row.site]);
    box.addEventListener("change", async () => {
      if (box.checked) filed[row.site] = Date.now(); else delete filed[row.site];
      done.className = box.checked ? "done" : "";
      await Shield.api.storage.local.set({ [settingsKey + ".filed"]: filed });
    });
    done.className = box.checked ? "done" : "";
    done.append(box, filed[row.site] ? " " + new Date(filed[row.site]).toLocaleDateString() : "");
    tr.append(site, how, done);
    table.append(tr);
  }
}

function letterFor(site) {
  const who = identity.name || identity.aliases[0] || "";
  const email = identity.emails[0] || "";
  return `Subject: Request to delete my personal data

To ${site},

Under Article 17 of the GDPR and the CCPA/CPRA, and any other data-protection law that applies to you, I request that you delete all personal data you hold about me and stop selling or sharing it, and that you confirm in writing when it is done.

The data can be identified by the name ${who || "[your name]"}${email ? " and the email address " + email : ""}. The pages concerned are listed below.

${(hosts.get(site) || { results: [] }).results.map((result) => "- " + result.url).join("\n")}

Please do not use this information for any purpose other than carrying out this request. If you need to verify my identity, tell me what you require; I will not send more than the law entitles you to ask for.

${who}`;
}

async function copyLetter(site) {
  await navigator.clipboard.writeText(letterFor(site));
  byId("removal-status").textContent = "letter copied for " + site;
}

function shepherdPrompt() {
  const rows = plan().filter((row) => !filed[row.site]);
  const details = [
    identity.name ? "full name: " + identity.name : null,
    identity.aliases.length ? "also known as: " + identity.aliases.join(", ") : null,
    identity.emails.length ? "email: " + identity.emails.join(", ") : null,
    identity.phones.length ? "phone: " + identity.phones.join(", ") : null,
    identity.context.length ? "context: " + identity.context.join(", ") : null,
  ].filter(Boolean).join("\n");
  return `Please file removal requests for my personal data. Use the browser tool: open each page below in the browser room, fill the form with my details and the page addresses listed, and stop before pressing the final submit so I can check it (unless I have turned on auto-approve). If a page needs a captcha or a code from my email, tell me and move to the next one. Keep a short list at the end: filed, needs me, could not.

My details:
${details}

Removal forms and settings, worst first:
${rows.map((row) => `- ${row.site}: ${row.url}\n  ${row.how}${(hosts.get(row.site) || { results: [] }).results.slice(0, 5).map((result) => "\n  page: " + result.url).join("")}`).join("\n")}

For sites with no form, send this letter to privacy@ or dpo@ the site from my email client, or draft it for me:

${letterFor("[site]")}`;
}

byId("removal-shepherd").addEventListener("click", async () => {
  const prompt = shepherdPrompt();
  byId("removal-status").textContent = "handing the plan to noah…";
  const answer = await Shield.host({ type: "agent.prompt", prompt });
  if (answer.error) {
    await navigator.clipboard.writeText(prompt).catch(() => {});
    byId("removal-status").textContent = answer.error + ". The plan is on your clipboard; paste it to shepherd, or to any assistant with a browser.";
    return;
  }
  byId("removal-status").textContent = "noah opened with the plan in shepherd's composer. Press send there; it asks before each form is submitted.";
});
byId("removal-copy").addEventListener("click", async () => {
  await navigator.clipboard.writeText(shepherdPrompt());
  byId("removal-status").textContent = "plan copied";
});
byId("removal-letter").addEventListener("click", () => copyLetter("[company]"));

// ---- places to check by hand ------------------------------------------------------
function renderChecks() {
  const list = byId("checks-list");
  list.replaceChildren();
  const name = identity.name || identity.aliases[0] || "";
  const q = encodeURIComponent(`"${name}"`);
  const links = [
    ["Google, with a quoted name", `https://www.google.com/search?q=${q}`],
    ["Google images of that name", `https://www.google.com/search?q=${q}&tbm=isch`],
    ["Yandex (strong on faces and eastern Europe)", `https://yandex.com/search/?text=${q}`],
    ["Yandex images", `https://yandex.com/images/search?text=${q}`],
    ["Naver (Korea)", `https://search.naver.com/search.naver?query=${q}`],
    ["Wayback Machine: pages about you that were taken down", `https://web.archive.org/web/*/${encodeURIComponent(name)}*`],
    ["archive.org texts", `https://archive.org/search?query=${q}`],
    ["Facebook people", `https://www.facebook.com/search/people/?q=${encodeURIComponent(name)}`],
    ["LinkedIn people", `https://www.linkedin.com/search/results/people/?keywords=${encodeURIComponent(name)}`],
    ["X (Twitter)", `https://x.com/search?q=${q}&f=user`],
    ["TikTok", `https://www.tiktok.com/search/user?q=${encodeURIComponent(name)}`],
    ["Reddit", `https://www.reddit.com/search/?q=${q}`],
    ["TruePeopleSearch (US)", `https://www.truepeoplesearch.com/results?name=${encodeURIComponent(name)}`],
    ["FastPeopleSearch (US)", `https://www.fastpeoplesearch.com/name/${encodeURIComponent(name.replace(/\s+/g, "-"))}`],
    ["192.com (UK)", `https://www.192.com/people/search/?q=${encodeURIComponent(name)}`],
    ["Yasni (Germany)", `https://www.yasni.de/${encodeURIComponent(name.replace(/\s+/g, "+"))}/person+information`],
  ];
  for (const email of identity.emails) {
    links.push([`Have I Been Pwned: ${email}`, `https://haveibeenpwned.com/account/${encodeURIComponent(email)}`]);
    links.push([`Gravatar for ${email}`, `https://gravatar.com/site/check/`]);
  }
  for (const phone of identity.phones) {
    links.push([`Google for ${phone}`, `https://www.google.com/search?q=${encodeURIComponent('"' + phone + '"')}`]);
  }
  for (const [label, url] of links) {
    const item = document.createElement("li");
    const link = document.createElement("a");
    link.href = url;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = label;
    item.append(link);
    list.append(item);
  }
}

// ---- start ---------------------------------------------------------------------------
async function start() {
  const select = byId("f-lang");
  for (const [code, label] of UI_LANGUAGES) {
    const option = document.createElement("option");
    option.value = code;
    option.textContent = label;
    select.append(option);
  }
  const preferred = (navigator.language || "en").slice(0, 2).toLowerCase();
  select.value = UI_LANGUAGES.some(([code]) => code === preferred) ? preferred : "en";
  const stored = await Shield.api.storage.local.get([settingsKey + ".identity", settingsKey + ".filed", settingsKey + ".colors"]);
  filed = stored[settingsKey + ".filed"] || {};
  colors = stored[settingsKey + ".colors"] || {};
  const remembered = stored[settingsKey + ".identity"];
  if (remembered) {
    byId("f-name").value = remembered.name || "";
    byId("f-aliases").value = (remembered.aliases || []).join(", ");
    byId("f-emails").value = (remembered.emails || []).join(", ");
    byId("f-phones").value = (remembered.phones || []).join(", ");
    byId("f-context").value = (remembered.context || []).join(", ");
    byId("f-remember").checked = true;
    if (remembered.lang) select.value = remembered.lang;
  }
  const fromQuery = new URLSearchParams(location.search).get("name");
  if (fromQuery) byId("f-name").value = fromQuery;
  byId("f-go").addEventListener("click", () => { search().catch((error) => { byId("f-status").textContent = String(error.message || error); byId("f-go").disabled = false; }); });
  byId("f-stop").addEventListener("click", () => { stopping = true; });
}

start();
