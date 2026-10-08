// Reddit intelligence layer for noah shield.
// Removes promoted posts, adds post age clarity (Reddit buries timestamps),
// surfaces mod transparency on controversial subs, detects karma-farming
// patterns, and flags reposts. Reads only the DOM already rendered.
//
// Runs in the ISOLATED world.
(() => {
  "use strict";
  if (window.__noahShieldReddit) return;
  const host = location.hostname.toLowerCase();
  if (!/(^|\.)reddit\.com$/.test(host)) return;
  window.__noahShieldReddit = true;

  const api = globalThis.chrome ?? globalThis.browser;
  const send = (msg) => new Promise((resolve) => {
    try { api.runtime.sendMessage(msg, (r) => { void api.runtime.lastError; resolve(r || {}); }); }
    catch { resolve({}); }
  });

  let config = {
    enabled: true,
    hidePromoted: true,
    clearDates: true,
    showModInfo: true,
    flagKarmaFarming: true,
    hideAwards: true,
    compactMode: false,
  };

  // ── load settings from background ────────────────────────────────────────
  send({ type: "social.config", platform: "reddit" }).then((r) => {
    if (r && r.config && Object.keys(r.config).length) {
      config = { ...config, ...r.config };
      if (!config.enabled) return;
    }
    queueScan();
  }).catch(() => queueScan());

  api.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "social.set" && msg.platform === "reddit" && msg.config) {
      config = { ...config, ...msg.config };
      if (!config.enabled) mo.disconnect();
    }
  });

  // ── styles ────────────────────────────────────────────────────────────────
  let _styled = false;
  function injectStyles() {
    if (_styled) return;
    _styled = true;
    const s = document.createElement("style");
    s.textContent = `
      @keyframes noah-reddit-in { from { opacity:0; } to { opacity:1; } }
      [data-noah-reddit-badge] {
        display:inline-flex !important; align-items:center; gap:4px;
        font:11px/1.4 -apple-system,system-ui,sans-serif !important;
        color:#9aa298 !important; background:rgba(7,9,9,0.82) !important;
        border:1px solid rgba(180,210,190,0.14) !important;
        border-radius:5px !important; padding:2px 7px !important;
        margin-left:6px !important; pointer-events:none; white-space:nowrap;
        vertical-align:middle;
      }
      [data-noah-reddit-badge="promoted"] { color:#e8b4a8 !important; border-color:rgba(232,180,168,0.28) !important; }
      [data-noah-reddit-badge="farm"] { color:#dcc896 !important; border-color:rgba(220,200,150,0.22) !important; }
      [data-noah-reddit-badge="age"] { color:#a9cf9f !important; border-color:rgba(169,207,159,0.18) !important; }
      /* Reddit new UI overrides */
      [data-noah-awards-hidden] faceplate-tracker[noun="award"] { display:none !important; }
      [data-noah-awards-hidden] shreddit-award-button { display:none !important; }
      [data-noah-awards-hidden] [class*="award" i] { display:none !important; }
    `;
    (document.head || document.documentElement).appendChild(s);
  }

  // ── promoted post detection ───────────────────────────────────────────────
  // Reddit new UI (shreddit): promoted posts carry a specific attribute/slot
  // Reddit old UI: .promoted class, or a "Promoted" flair span
  const PROMOTED_SELECTORS = [
    'shreddit-ad-post',
    '[data-testid*="promoted" i]',
    '[promoted="true"]',
    '.promotedlink',
    '[data-click-id="background"][href*="/r/"]', // old reddit promoted
  ];
  const PROMOTED_TEXT = /^(promoted|advertisement|sponsored)$/i;

  function isPromoted(el) {
    for (const sel of PROMOTED_SELECTORS) {
      if (el.matches?.(sel) || el.closest?.(sel)) return true;
    }
    // Text check on flair elements
    const flairs = el.querySelectorAll('[id*="post-type-link-flair" i], .flairTag, [data-testid*="flair"], span[class*="flair" i]');
    for (const f of flairs) {
      if (PROMOTED_TEXT.test((f.textContent || "").trim())) return true;
    }
    return false;
  }

  // ── age clarity: parse reddit's relative timestamps into absolute ─────────
  function parseRelativeTime(text) {
    text = text.trim().toLowerCase();
    const now = Date.now();
    const patterns = [
      [/(\d+)\s*second/, 1000],
      [/(\d+)\s*minute/, 60000],
      [/(\d+)\s*hour/, 3600000],
      [/(\d+)\s*day/, 86400000],
      [/(\d+)\s*week/, 604800000],
      [/(\d+)\s*month/, 2592000000],
      [/(\d+)\s*year/, 31536000000],
    ];
    for (const [pat, ms] of patterns) {
      const m = text.match(pat);
      if (m) return new Date(now - parseInt(m[1]) * ms);
    }
    return null;
  }

  function formatAbsolute(date) {
    if (!date) return null;
    const d = date;
    const months = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
    return `${months[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()}`;
  }

  function addAbsoluteDate(el) {
    if (el.dataset.noahDateDone) return;
    el.dataset.noahDateDone = "1";

    // new reddit: <time> element with datetime attr is most reliable
    const timeEl = el.querySelector("time[datetime]");
    if (timeEl) {
      try {
        const d = new Date(timeEl.getAttribute("datetime"));
        if (!isNaN(d)) {
          const badge = document.createElement("span");
          badge.setAttribute("data-noah-reddit-badge", "age");
          badge.textContent = formatAbsolute(d);
          timeEl.insertAdjacentElement("afterend", badge);
          return;
        }
      } catch {}
    }

    // Fallback: relative text nodes
    const relTimeEl = el.querySelector('[data-testid*="time"], [id*="post-timestamp"], ._3yx4Dn-');
    if (relTimeEl) {
      const text = (relTimeEl.textContent || "").trim();
      const parsed = parseRelativeTime(text);
      if (parsed) {
        const badge = document.createElement("span");
        badge.setAttribute("data-noah-reddit-badge", "age");
        badge.textContent = formatAbsolute(parsed);
        relTimeEl.insertAdjacentElement("afterend", badge);
      }
    }
  }

  // ── karma farming signals ─────────────────────────────────────────────────
  // Pattern: repost-looking title patterns, crosspost labels, no engagement
  const FARM_PATTERNS = [
    /\[oc\].*\(\d+\s*(up)?votes?\)/i,
    /found this gem/i,
    /not sure if (this has been|already)/i,
    /couldn't find the original/i,
    /^\[\d+\]\s+/,   // numbered lists with karma-bait titles
  ];

  function checkKarmaFarm(post, titleText) {
    if (!config.flagKarmaFarming || post.dataset.noahFarmChecked) return;
    post.dataset.noahFarmChecked = "1";
    for (const pattern of FARM_PATTERNS) {
      if (pattern.test(titleText)) {
        const titleEl = post.querySelector('h3, [slot="title"], [data-testid="post-content"] h3');
        if (titleEl) {
          const badge = document.createElement("span");
          badge.setAttribute("data-noah-reddit-badge", "farm");
          badge.textContent = "◈ potential repost";
          titleEl.insertAdjacentElement("afterend", badge);
        }
        break;
      }
    }
  }

  // ── hide Reddit awards (noise reduction) ─────────────────────────────────
  function hideAwards(post) {
    if (!config.hideAwards || post.dataset.noahAwardsHidden) return;
    post.dataset.noahAwardsHidden = "1";
    post.setAttribute("data-noah-awards-hidden", "1");
  }

  // ── main post scanner ─────────────────────────────────────────────────────
  function scanPost(post) {
    if (post.dataset.noahRedditScanned) return;
    post.dataset.noahRedditScanned = "1";
    injectStyles();

    // Promoted
    if (isPromoted(post)) {
      if (config.hidePromoted) {
        post.style.setProperty("display", "none", "important");
        send({ type: "safety.event", kind: "social_promoted_hidden", platform: "reddit" });
        return;
      }
      const titleEl = post.querySelector("h3, [slot='title']");
      if (titleEl) {
        const badge = document.createElement("span");
        badge.setAttribute("data-noah-reddit-badge", "promoted");
        badge.textContent = "◈ promoted";
        titleEl.prepend(badge);
      }
      return;
    }

    // Clear dates
    if (config.clearDates) addAbsoluteDate(post);

    // Karma farming check
    const titleEl = post.querySelector("h3, [slot='title'], [data-testid='post-content'] h3");
    if (titleEl) checkKarmaFarm(post, titleEl.textContent || "");

    // Hide awards
    hideAwards(post);
  }

  // ── subreddit mod banner (shown once per sub visit) ───────────────────────
  let modBannerShown = false;
  function showModBanner() {
    if (!config.showModInfo || modBannerShown) return;
    const subMatch = location.pathname.match(/\/r\/([^/]+)/);
    if (!subMatch) return;
    const modEl = document.querySelector('[data-testid="moderator-list"], .moderator-list, [id*="moderators"]');
    if (!modEl) return;
    modBannerShown = true;
    const sub = subMatch[1];
    const modLinks = modEl.querySelectorAll("a");
    const modCount = modLinks.length;
    if (!modCount) return;
    const banner = document.createElement("div");
    banner.style.cssText = [
      "background:rgba(7,9,9,0.88)", "border:1px solid rgba(180,210,190,0.12)",
      "border-radius:10px", "padding:8px 14px", "margin:8px 0",
      "font:12px/1.5 -apple-system,system-ui,sans-serif", "color:#9aa298",
    ].join(";");
    banner.innerHTML = `<span style="color:#a9cf9f">◈</span> r/${sub} has ${modCount} moderator${modCount !== 1 ? "s" : ""}`;
    modEl.insertAdjacentElement("beforebegin", banner);
  }

  // ── mutation observer ─────────────────────────────────────────────────────
  let scanQueued = false;
  function queueScan() {
    if (scanQueued) return;
    scanQueued = true;
    requestAnimationFrame(() => {
      scanQueued = false;
      // New Reddit (shreddit) and old Reddit selectors
      const posts = document.querySelectorAll(
        'shreddit-post:not([data-noah-reddit-scanned]), ' +
        '.Post:not([data-noah-reddit-scanned]), ' +
        '[data-testid="post-container"]:not([data-noah-reddit-scanned]), ' +
        '.thing.link:not([data-noah-reddit-scanned])'
      );
      posts.forEach(scanPost);
      showModBanner();
    });
  }

  const mo = new MutationObserver(queueScan);
  mo.observe(document.documentElement, { childList: true, subtree: true });
  // initial scan driven by config fetch above (queueScan in .then / .catch)

  document.addEventListener("noah-reddit-config", (e) => {
    try { const next = JSON.parse(e.detail || "{}"); config = { ...config, ...next }; } catch {}
    if (!config.enabled) mo.disconnect();
  });
  document.dispatchEvent(new CustomEvent("noah-reddit-ready"));
})();
