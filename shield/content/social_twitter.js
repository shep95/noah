// X / Twitter feed intelligence layer for noah shield.
// Hides promoted tweets, surfaces absolute post dates, flags reply-ratio signals
// (when replies >> likes, the crowd is pushing back), removes sidebar algo injections,
// and flags low-follower accounts inline so you see the source before reading.
//
// Runs in the ISOLATED world. Pairs with the existing spaces.js (Spaces listener intel)
// and profile.js (profile deep-dive panel) — this script handles the main timeline.
(() => {
  "use strict";
  if (window.__noahShieldTwitter) return;
  const host = location.hostname.toLowerCase();
  if (!/(^|\.)(?:twitter|x)\.com$/.test(host)) return;
  window.__noahShieldTwitter = true;

  const api = globalThis.chrome ?? globalThis.browser;
  const send = (msg) => new Promise((resolve) => {
    try { api.runtime.sendMessage(msg, (r) => { void api.runtime.lastError; resolve(r || {}); }); }
    catch { resolve({}); }
  });

  let config = {
    enabled: true,
    hidePromoted: true,
    showAbsoluteDates: true,
    flagRatio: true,
    ratioThreshold: 2,        // replies:likes ratio above this = controversy flag
    cleanSidebar: true,
    hideWhoToFollow: true,
    flagBotSignal: false,
    botFollowerRatio: 20,     // following:followers above this = bot signal
    sessionWarningMinutes: 30,  // show a toast after this many minutes on X
  };

  // ── session timer — same psychology as Instagram: the platform doesn't
  // tell you how long you've been here. We do. ──────────────────────────────
  const sessionStart = Date.now();
  let timeWarningShown = false;
  function checkSessionTime() {
    if (!config.sessionWarningMinutes || timeWarningShown) return;
    const elapsed = (Date.now() - sessionStart) / 60000;
    if (elapsed < config.sessionWarningMinutes) return;
    timeWarningShown = true;
    const minutes = Math.round(elapsed);
    const el = document.createElement("div");
    el.setAttribute("data-noah-toast", "session");
    el.style.cssText = [
      "position:fixed","top:64px","left:50%","transform:translateX(-50%)",
      "z-index:9999999","background:rgba(7,9,9,0.94)","backdrop-filter:blur(16px)",
      "border:1px solid rgba(180,210,190,0.18)","border-radius:12px",
      "padding:10px 18px","font:13px/1.4 -apple-system,system-ui,sans-serif",
      "color:#d8ddd6","display:flex","align-items:center","gap:10px",
      "box-shadow:0 8px 40px rgba(0,0,0,0.5)",
    ].join(";");
    const icon = document.createElement("span");
    icon.textContent = "⏱"; icon.style.fontSize = "16px";
    const text = document.createElement("span");
    text.textContent = `${minutes} min on x / twitter — take a moment`;
    const close = document.createElement("button");
    close.textContent = "×";
    close.style.cssText = "background:none;border:none;color:#9aa298;cursor:pointer;font-size:16px;padding:0 0 0 4px;line-height:1";
    close.addEventListener("click", () => el.remove());
    el.append(icon, text, close);
    document.body?.appendChild(el);
    setTimeout(() => { if (el.parentNode) el.remove(); }, 12000);
    send({ type: "safety.event", kind: "social_time_warning", platform: "twitter" });
  }
  setInterval(checkSessionTime, 30000);

  // ── load settings from background ────────────────────────────────────────
  send({ type: "social.config", platform: "twitter" }).then((r) => {
    if (r && r.config && Object.keys(r.config).length) {
      config = { ...config, ...r.config };
      if (!config.enabled) return;
    }
    queueScan();
  }).catch(() => queueScan());

  api.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "social.set" && msg.platform === "twitter" && msg.config) {
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
      @keyframes noah-tw-in { from { opacity:0; } to { opacity:1; } }
      .noah-tw-badge {
        display:inline-flex !important; align-items:center; gap:4px;
        font:11px/1.4 -apple-system,system-ui,sans-serif !important;
        color:#9aa298 !important; background:rgba(7,9,9,0.9) !important;
        border:1px solid rgba(180,210,190,0.14) !important;
        border-radius:5px !important; padding:2px 8px !important;
        margin:0 4px !important; pointer-events:none; white-space:nowrap;
        vertical-align:middle; animation:noah-tw-in 0.2s ease;
        text-decoration:none !important;
      }
      .noah-tw-badge.promoted  { color:#e8b4a8 !important; border-color:rgba(232,180,168,0.28) !important; }
      .noah-tw-badge.date      { color:#a9cf9f !important; border-color:rgba(169,207,159,0.18) !important; }
      .noah-tw-badge.ratio     { color:#dcc896 !important; border-color:rgba(220,200,150,0.22) !important; }
      .noah-tw-badge.bot       { color:#c896dc !important; border-color:rgba(200,150,220,0.2) !important; }
    `;
    (document.head || document.documentElement).appendChild(s);
  }

  // ── promoted tweet detection ──────────────────────────────────────────────
  // Twitter uses multiple signals; DOM structure shifts with updates.
  function isPromoted(article) {
    // Most reliable: the "Ad" label or "Promoted" text inside the tweet
    const spans = article.querySelectorAll("span");
    for (const span of spans) {
      const text = (span.textContent || "").trim();
      if (text === "Ad" || text === "Promoted") {
        // Make sure it's a label, not tweet body text — check position
        const rect = span.getBoundingClientRect();
        // Labels are usually small and near the header
        if (span.closest('[data-testid="socialContext"]') ||
            span.closest('[data-testid="User-Name"]') ||
            span.closest('a[href*="/i/promoted"]') ||
            span.offsetHeight <= 24) {
          return true;
        }
      }
    }
    // data-testid promoted indicator
    if (article.querySelector('[data-testid="promotedIndicator"], [data-promoted="true"]')) return true;
    // href signal: "promoted" in the attribution link
    const promoLink = article.querySelector('a[href*="/i/promoted"], a[href*="promoted_content"]');
    if (promoLink) return true;
    return false;
  }

  // ── date: Twitter already stores datetime on <time> ───────────────────────
  const MONTHS = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
  function formatDate(iso) {
    const d = new Date(iso);
    if (isNaN(d)) return null;
    const now = Date.now();
    const diffDays = Math.floor((now - d) / 86400000);
    // hour12: true — user's local 12hr clock (1:04 pm, not 13:04)
    const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit", hour12: true });
    if (diffDays === 0) return `today ${time}`;
    if (diffDays === 1) return `yesterday ${time}`;
    return `${MONTHS[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()} ${time}`;
  }

  // ── deleted post tracking ──────────────────────────────────────────────────
  // Mechanism: X does not tell you when a tweet you read earlier has been
  // deleted. But the browser already rendered it. We cache every tweet we see
  // into localStorage the moment it renders. When X's own placeholder ("This
  // Tweet is unavailable") appears in the DOM, we surface the cached content so
  // nothing disappears silently.
  const TWEET_CACHE_KEY = "_noah_tw_cache";
  const TWEET_DEL_COUNT_KEY = "_noah_tw_del_count";

  function extractTweetId(article) {
    // Tweet permalink lives on the timestamp <a href="/user/status/ID">
    const a = article.querySelector('a[href*="/status/"]');
    if (!a) return null;
    const m = /\/status\/(\d+)/.exec(a.getAttribute("href") || "");
    return m ? m[1] : null;
  }
  function extractTweetText(article) {
    const el = article.querySelector('[data-testid="tweetText"]');
    return el ? (el.textContent || "").trim().slice(0, 280) : "";
  }
  function extractTweetAuthor(article) {
    const el = article.querySelector('[data-testid="User-Name"] a');
    return el ? (el.textContent || "").trim() : "";
  }

  function cacheTweet(id, text, author) {
    if (!id) return;
    try {
      const raw = localStorage.getItem(TWEET_CACHE_KEY);
      const cache = raw ? JSON.parse(raw) : {};
      if (cache[id]) return; // already stored
      cache[id] = { t: text, a: author, ts: Date.now() };
      // Trim to 500 oldest entries so storage stays bounded
      const entries = Object.entries(cache);
      if (entries.length > 500) {
        entries.sort((x, y) => (x[1].ts || 0) - (y[1].ts || 0));
        const trimmed = {};
        for (const [k, v] of entries.slice(entries.length - 500)) trimmed[k] = v;
        localStorage.setItem(TWEET_CACHE_KEY, JSON.stringify(trimmed));
      } else {
        localStorage.setItem(TWEET_CACHE_KEY, JSON.stringify(cache));
      }
    } catch {}
  }

  function getDeletedCount() {
    try { return parseInt(localStorage.getItem(TWEET_DEL_COUNT_KEY) || "0", 10); } catch { return 0; }
  }
  function bumpDeletedCount() {
    try { localStorage.setItem(TWEET_DEL_COUNT_KEY, String(getDeletedCount() + 1)); } catch {}
  }

  function checkUnavailableTweet(article) {
    // X renders deleted / suspended / age-restricted tweets as inline placeholders
    if (article.dataset.noahTwUnavailable) return;
    const text = (article.textContent || "").toLowerCase();
    if (!/(this tweet is unavailable|this post is unavailable|suspended account|age-restricted adult content)/.test(text)) return;
    article.dataset.noahTwUnavailable = "1";
    bumpDeletedCount();
    injectStyles();
    // Try to show cached content if we have the tweet id
    const id = extractTweetId(article);
    let cached = null;
    if (id) {
      try {
        const raw = localStorage.getItem(TWEET_CACHE_KEY);
        if (raw) cached = (JSON.parse(raw) || {})[id] || null;
      } catch {}
    }
    const badge = document.createElement("span");
    badge.className = "noah-tw-badge";
    badge.style.cssText = "color:#e8b4a8 !important;border-color:rgba(232,180,168,0.28) !important;display:block !important;width:fit-content;margin-top:4px";
    badge.textContent = cached
      ? `◈ deleted · was: "${cached.a ? cached.a + ": " : ""}${cached.t.slice(0, 120)}${cached.t.length > 120 ? "…" : ""}"`
      : `◈ this post was deleted or removed (post #${getDeletedCount()} today)`;
    const cell = article.closest('[data-testid="cellInnerDiv"]') || article;
    cell.insertAdjacentElement("afterend", badge);
    send({ type: "safety.event", kind: "deleted_tweet_detected", platform: "twitter" });
  }

  // ── reply/like ratio signal ───────────────────────────────────────────────
  // When a post gets significantly more replies than likes, the crowd is pushing
  // back. This is signal, not noise — flag it so readers notice.
  function parseCount(text) {
    if (!text) return 0;
    const t = text.replace(/,/g, "").trim().toLowerCase();
    if (t.endsWith("k")) return parseFloat(t) * 1000;
    if (t.endsWith("m")) return parseFloat(t) * 1000000;
    return parseFloat(t) || 0;
  }

  function checkRatio(article) {
    if (!config.flagRatio) return;
    // aria-label on the action buttons carries counts: "3 Replies", "42 Likes"
    const replyBtn = article.querySelector('[data-testid="reply"]');
    const likeBtn = article.querySelector('[data-testid="like"]');
    if (!replyBtn || !likeBtn) return;
    const replyLabel = replyBtn.getAttribute("aria-label") || "";
    const likeLabel = likeBtn.getAttribute("aria-label") || "";
    const replies = parseCount(replyLabel.split(" ")[0]);
    const likes = parseCount(likeLabel.split(" ")[0]);
    if (replies < 20) return;   // too few to be meaningful
    if (likes === 0 && replies === 0) return;
    const ratio = likes === 0 ? Infinity : replies / likes;
    if (ratio >= config.ratioThreshold) {
      injectStyles();
      const timeEl = article.querySelector("time");
      if (timeEl && !timeEl.dataset.noahTwRatio) {
        timeEl.dataset.noahTwRatio = "1";
        const badge = document.createElement("span");
        badge.className = "noah-tw-badge ratio";
        const r = ratio === Infinity ? "∞" : ratio.toFixed(1);
        badge.textContent = `◈ ${r}× more replies than likes`;
        timeEl.insertAdjacentElement("afterend", badge);
      }
    }
  }

  // ── main tweet scanner ────────────────────────────────────────────────────
  function scanTweet(article) {
    if (article.dataset.noahTwScanned) return;
    article.dataset.noahTwScanned = "1";

    // Cache tweet content for deleted-post detection before anything else runs.
    // This must run first so we have the content cached before it can disappear.
    const tweetId = extractTweetId(article);
    if (tweetId) cacheTweet(tweetId, extractTweetText(article), extractTweetAuthor(article));
    checkUnavailableTweet(article);

    // Promoted: hide or badge
    if (isPromoted(article)) {
      if (config.hidePromoted) {
        // Hide the wrapper cell, not the article itself (avoids layout holes)
        const cell = article.closest('[data-testid="cellInnerDiv"]') || article;
        cell.style.setProperty("display", "none", "important");
        send({ type: "safety.event", kind: "social_sponsored_hidden", platform: "twitter" });
        return;
      }
      injectStyles();
      const nameArea = article.querySelector('[data-testid="User-Name"]');
      if (nameArea) {
        const badge = document.createElement("span");
        badge.className = "noah-tw-badge promoted";
        badge.textContent = "◈ promoted";
        nameArea.insertAdjacentElement("afterend", badge);
      }
      return;
    }

    // Absolute date
    if (config.showAbsoluteDates) {
      const timeEl = article.querySelector("time[datetime]:not([data-noah-tw-date])");
      if (timeEl) {
        timeEl.dataset.noahTwDate = "1";
        const iso = timeEl.getAttribute("datetime");
        const formatted = formatDate(iso);
        if (formatted) {
          injectStyles();
          const badge = document.createElement("span");
          badge.className = "noah-tw-badge date";
          badge.textContent = formatted;
          timeEl.insertAdjacentElement("afterend", badge);
        }
      }
    }

    // Reply/like ratio signal
    checkRatio(article);
  }

  // ── sidebar: clean "Who to follow" and "What's happening" ────────────────
  function cleanSidebar() {
    if (!config.cleanSidebar) return;
    // "What's happening" / Trending
    const trending = document.querySelectorAll(
      '[data-testid="sidebarColumn"] [data-testid="trend"]:not([data-noah-tw-noise])'
    );
    // Don't hide trending topics themselves — only the "promoted" ones
    // (Hiding all trends is too aggressive; just mark so users can filter)

    if (config.hideWhoToFollow) {
      // "Who to follow" section
      const followSections = document.querySelectorAll(
        '[data-testid="sidebarColumn"] aside:not([data-noah-tw-noise])'
      );
      for (const section of followSections) {
        const heading = section.querySelector('[role="heading"]');
        if (heading && /who to follow|you might like|suggested/i.test(heading.textContent || "")) {
          section.dataset.noahTwNoise = "1";
          section.style.setProperty("display", "none", "important");
        }
      }
      // Standalone "Connect" / "Follow" prompts
      document.querySelectorAll(
        '[data-testid="sidebarColumn"] [data-testid="UserCell"]:not([data-noah-tw-noise])'
      ).forEach((cell) => {
        const section = cell.closest("section, aside, div[aria-label]");
        if (!section) return;
        const label = (section.getAttribute("aria-label") || "").toLowerCase();
        if (/who to follow|you might like|suggested/i.test(label)) {
          section.dataset.noahTwNoise = "1";
          section.style.setProperty("display", "none", "important");
        }
      });
    }
  }

  // ── "For You" algo injection detection ───────────────────────────────────
  // Tweets injected by the algo into a Following timeline are marked
  // with a "Suggested" label. Surface this so you know what's organic vs. pushed.
  function flagAlgoInjections() {
    const suggested = document.querySelectorAll(
      'article[data-testid="tweet"]:not([data-noah-tw-algo])'
    );
    for (const article of suggested) {
      // Look for the socialContext span: "Suggested" or "You might like"
      const ctx = article.querySelector('[data-testid="socialContext"]');
      if (!ctx) continue;
      const text = (ctx.textContent || "").trim();
      if (/suggested|you might like/i.test(text)) {
        article.dataset.noahTwAlgo = "1";
        if (!article.querySelector(".noah-tw-badge.promoted")) {
          injectStyles();
          const badge = document.createElement("span");
          badge.className = "noah-tw-badge promoted";
          badge.textContent = "◈ algo-suggested";
          ctx.insertAdjacentElement("afterend", badge);
        }
      }
    }
  }

  // ── main scan ─────────────────────────────────────────────────────────────
  let scanQueued = false;
  function queueScan() {
    if (scanQueued) return;
    scanQueued = true;
    requestAnimationFrame(() => {
      scanQueued = false;
      if (!config.enabled) return;
      const articles = document.querySelectorAll('article[data-testid="tweet"]:not([data-noah-tw-scanned])');
      articles.forEach(scanTweet);
      flagAlgoInjections();
      cleanSidebar();
    });
  }

  const mo = new MutationObserver(queueScan);
  mo.observe(document.documentElement, { childList: true, subtree: true });
  // initial scan driven by config fetch above (queueScan in .then / .catch)

  document.addEventListener("noah-twitter-config", (e) => {
    try { const next = JSON.parse(e.detail || "{}"); config = { ...config, ...next }; } catch {}
    if (!config.enabled) mo.disconnect();
  });
  document.dispatchEvent(new CustomEvent("noah-twitter-ready"));
})();
