// Links, cleaned before you follow or copy them: redirect wrappers unwrapped
// (google, facebook, youtube, outlook safelinks, proofpoint), tracking
// parameters removed, click-tracking attributes dropped. Also blurs your own
// name where you asked it to (anti-dox).
(() => {
  "use strict";
  const api = globalThis.chrome ?? globalThis.browser;

  const TRACKING = new Set(["utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content", "utm_id", "utm_name", "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid", "msclkid", "yclid", "twclid", "ttclid", "igshid", "igsh", "mc_cid", "mc_eid", "mkt_tok", "_hsenc", "_hsmi", "vero_id", "oly_anon_id", "oly_enc_id", "wickedid", "ef_id", "s_kwcid", "_openstat", "ref_src", "ref_url", "rb_clickid", "srsltid", "_ga", "_gl", "_kx", "ncid", "cmpid", "spm", "scm", "trk", "trkCampaign", "sc_campaign", "sc_channel", "sc_content", "sc_medium", "pk_campaign", "pk_kwd", "pk_source", "pk_medium", "at_medium", "at_campaign", "soc_src", "soc_trk", "si", "feature", "ved", "usg", "ei", "sa", "sxsrf"]);
  const WRAPPERS = [
    [/^https?:\/\/(www\.)?google\.[a-z.]+\/url\?/i, ["q", "url"]],
    [/^https?:\/\/(l|lm|m)\.facebook\.com\/l\.php\?/i, ["u"]],
    [/^https?:\/\/l\.instagram\.com\/\?/i, ["u"]],
    [/^https?:\/\/(www\.)?youtube\.com\/redirect\?/i, ["q"]],
    [/^https?:\/\/t\.umblr\.com\/redirect\?/i, ["z"]],
    [/^https?:\/\/out\.reddit\.com\/\?/i, ["url"]],
    [/^https?:\/\/[a-z0-9.-]*safelinks\.protection\.outlook\.com\/\?/i, ["url"]],
    [/^https?:\/\/(www\.)?linkedin\.com\/safety\/go\?/i, ["url"]],
    [/^https?:\/\/slack-redir\.net\/link\?/i, ["url"]],
    [/^https?:\/\/(www\.)?bing\.com\/ck\/a\?/i, ["u"]],
    [/^https?:\/\/duckduckgo\.com\/l\/\?/i, ["uddg"]],
    [/^https?:\/\/(www\.)?steamcommunity\.com\/linkfilter\/\?/i, ["url", "u"]],
    [/^https?:\/\/(www\.)?deviantart\.com\/users\/outgoing\?/i, [null]],
    [/^https?:\/\/exit\.sc\/\?/i, ["url"]],
    [/^https?:\/\/(www\.)?vk\.com\/away\.php\?/i, ["to"]],
    [/^https?:\/\/(www\.)?awstrack\.me\/L0\//i, [null]],
  ];

  function decodeBase64Url(text) {
    try {
      return atob(text.replace(/-/g, "+").replace(/_/g, "/"));
    } catch {
      return "";
    }
  }

  function unwrap(href) {
    for (const [pattern, keys] of WRAPPERS) {
      if (!pattern.test(href)) continue;
      let url;
      try {
        url = new URL(href);
      } catch {
        return href;
      }
      for (const key of keys) {
        if (key === null) {
          const rest = href.replace(pattern, "");
          if (/^https?:\/\//.test(rest)) return decodeURIComponent(rest);
          continue;
        }
        const value = url.searchParams.get(key);
        if (value && /^https?:\/\//i.test(value)) return value;
      }
    }
    // Proofpoint: https://urldefense.com/v3/__https://real.example__;!!...
    const proofpoint = /^https?:\/\/urldefense\.(com|proofpoint\.com)\/v3\/__(https?:\/\/[^_]+)__/i.exec(href);
    if (proofpoint) return proofpoint[2];
    // Bing's "u" carries a1 + base64.
    const bing = /^https?:\/\/(www\.)?bing\.com\/ck\/a\?.*[?&]u=a1([A-Za-z0-9_-]+)/i.exec(href);
    if (bing) {
      const decoded = decodeBase64Url(bing[2]);
      if (/^https?:\/\//.test(decoded)) return decoded;
    }
    return href;
  }

  function clean(href) {
    let out = unwrap(href);
    if (out !== href) out = unwrap(out);
    try {
      const url = new URL(out);
      let changed = false;
      for (const key of Array.from(url.searchParams.keys())) {
        if (TRACKING.has(key) || /^utm_/i.test(key)) {
          url.searchParams.delete(key);
          changed = true;
        }
      }
      if (changed) out = url.toString().replace(/\?$/, "");
    } catch {
      return out;
    }
    return out;
  }

  function cleanAnchor(anchor) {
    if (!anchor || !anchor.href || !/^https?:/i.test(anchor.href)) return;
    const cleaned = clean(anchor.href);
    if (cleaned !== anchor.href) {
      anchor.href = cleaned;
    }
    if (anchor.hasAttribute("ping")) anchor.removeAttribute("ping");
    // Google, Bing and others swap the href on mousedown; the copies they keep are dropped.
    for (const attribute of ["data-jsarwt", "onmousedown", "data-saferedirecturl", "data-ctorig", "jsaction"]) {
      if (anchor.hasAttribute(attribute) && /mousedown|url|redirect|ctorig/i.test(anchor.getAttribute(attribute) || attribute)) anchor.removeAttribute(attribute);
    }
  }

  function onPointer(event) {
    const anchor = event.target && event.target.closest ? event.target.closest("a[href]") : null;
    if (anchor) cleanAnchor(anchor);
  }

  function sweep() {
    for (const anchor of document.querySelectorAll("a[href][ping], a[href*='/url?'], a[href*='l.php?'], a[href*='safelinks'], a[href*='utm_'], a[href*='fbclid'], a[href*='gclid'], a[href*='redirect?']")) {
      cleanAnchor(anchor);
    }
  }

  // Copying a link: the clipboard gets the clean one.
  function onCopy(event) {
    const selection = String(getSelection() || "").trim();
    if (!/^https?:\/\/\S+$/i.test(selection)) return;
    const cleaned = clean(selection);
    if (cleaned === selection) return;
    event.clipboardData.setData("text/plain", cleaned);
    event.preventDefault();
  }

  // ---- anti-dox: your name blurred wherever it appears -------------------------
  const blurred = new WeakSet();
  function blurNames(names) {
    if (!names.length) return;
    const pattern = new RegExp(names.map((name) => name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|"), "i");
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const hits = [];
    let node;
    const alreadyBlurred = (element) => {
      for (let current = element; current; current = current.parentElement) if (blurred.has(current)) return true;
      return false;
    };
    while ((node = walker.nextNode())) {
      if (pattern.test(node.nodeValue) && node.parentElement && !alreadyBlurred(node.parentElement)) hits.push(node.parentElement);
    }
    for (const element of hits) {
      blurred.add(element);
      element.style.setProperty("filter", "blur(6px)", "important");
      element.title = "noah shield blurred your name here; hover in the shield popup to show it";
    }
  }

  function waitForConfig() {
    return new Promise((resolve) => {
      if (window.__noahShieldSiteConfig) return resolve(window.__noahShieldSiteConfig);
      document.addEventListener("noah-shield:site-config", () => resolve(window.__noahShieldSiteConfig), { once: true });
      setTimeout(() => resolve(window.__noahShieldSiteConfig || null), 4000);
    });
  }

  waitForConfig().then((config) => {
    if (!config) return;
    if (config.linkCleaner !== false && !config.trusted) {
      document.addEventListener("mousedown", onPointer, true);
      document.addEventListener("click", onPointer, true);
      document.addEventListener("auxclick", onPointer, true);
      document.addEventListener("keydown", (event) => { if (event.key === "Enter") onPointer(event); }, true);
      document.addEventListener("copy", onCopy, true);
      const run = () => sweep();
      if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", run, { once: true });
      else run();
      let pending = null;
      new MutationObserver(() => {
        if (pending) return;
        pending = setTimeout(() => { pending = null; sweep(); }, 1000);
      }).observe(document.documentElement, { childList: true, subtree: true });
    }
    const names = Array.isArray(config.hideNames) ? config.hideNames.filter((name) => name && name.length >= 3) : [];
    if (names.length) {
      const run = () => blurNames(names);
      if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", run, { once: true });
      else run();
      setInterval(run, 3000);
    }
  });

  api.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message && message.type === "links.cleanSelection") {
      sendResponse({ cleaned: clean(String(message.url || "")) });
    }
    return false;
  });
})();
