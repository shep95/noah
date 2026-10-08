// LinkedIn intelligence layer for noah shield.
// Removes sponsored content and job promotion ads, surfaces actual post
// ages (LinkedIn uses vague "1 week ago"), detects engagement bait and
// hustle-porn formatting, flags job posting staleness, and strips the
// dopamine-maximizing "Reactions" noise.
//
// Runs in the ISOLATED world.
(() => {
  "use strict";
  if (window.__noahShieldLinkedIn) return;
  const host = location.hostname.toLowerCase();
  if (!/(^|\.)linkedin\.com$/.test(host)) return;
  window.__noahShieldLinkedIn = true;

  const api = globalThis.chrome ?? globalThis.browser;
  const send = (msg) => new Promise((resolve) => {
    try { api.runtime.sendMessage(msg, (r) => { void api.runtime.lastError; resolve(r || {}); }); }
    catch { resolve({}); }
  });

  let config = {
    enabled: true,
    hideSponsored: true,
    showAbsoluteDates: true,
    flagEngagementBait: true,
    hideReactions: false,
    flagStaleJobs: true,
    staleJobDays: 30,
    cleanFeed: true,
  };

  // ── load settings from background ────────────────────────────────────────
  send({ type: "social.config", platform: "linkedin" }).then((r) => {
    if (r && r.config && Object.keys(r.config).length) {
      config = { ...config, ...r.config };
      if (!config.enabled) return;
    }
    queueScan();
  }).catch(() => queueScan());

  api.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "social.set" && msg.platform === "linkedin" && msg.config) {
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
      @keyframes noah-li-in { from { opacity:0; } to { opacity:1; } }
      .noah-li-badge {
        display:inline-flex !important; align-items:center; gap:4px;
        font:11px/1.4 -apple-system,system-ui,sans-serif !important;
        color:#9aa298 !important; background:rgba(7,9,9,0.88) !important;
        border:1px solid rgba(180,210,190,0.14) !important;
        border-radius:5px !important; padding:2px 8px !important;
        margin-left:6px !important; pointer-events:none; white-space:nowrap;
        vertical-align:middle; animation: noah-li-in 0.2s ease;
      }
      .noah-li-badge.sponsored { color:#e8b4a8 !important; border-color:rgba(232,180,168,0.28) !important; }
      .noah-li-badge.bait { color:#dcc896 !important; border-color:rgba(220,200,150,0.22) !important; }
      .noah-li-badge.date { color:#a9cf9f !important; border-color:rgba(169,207,159,0.18) !important; }
      .noah-li-badge.stale { color:#e8b4a8 !important; border-color:rgba(232,180,168,0.22) !important; }
    `;
    (document.head || document.documentElement).appendChild(s);
  }

  // ── sponsored post detection ──────────────────────────────────────────────
  const SPONSORED_SELECTORS = [
    '[data-test-id="post-promoted-label"]',
    '.update-components-header__subtitle-link--promoted',
    '[data-control-name="promoted_trending_update_click"]',
    'span.update-components-actor__sub-description',
  ];
  const SPONSORED_TEXT = /^(promoted|sponsored|following)$/i;

  function isSponsored(post) {
    for (const sel of SPONSORED_SELECTORS) {
      const el = post.querySelector(sel);
      if (el && (SPONSORED_TEXT.test((el.textContent || "").trim()) || el.closest('[data-x-li-promoted]'))) return true;
    }
    // Check for "Promoted" text in the sub-description area
    const subDescs = post.querySelectorAll(
      '.update-components-actor__sub-description, .feed-shared-actor__sub-description, ' +
      '.update-components-header__subtitle'
    );
    for (const sd of subDescs) {
      if (/^promoted$/i.test((sd.textContent || "").trim())) return true;
    }
    return false;
  }

  // ── date parsing ──────────────────────────────────────────────────────────
  function parseLIDate(text) {
    text = text.trim().toLowerCase();
    // LinkedIn uses: "2h", "3d", "1w", "2mo", "1yr", or "just now"
    const now = Date.now();
    const map = { s: 1000, m: 60000, h: 3600000, d: 86400000, w: 604800000, mo: 2592000000, yr: 31536000000 };
    const m = text.match(/^(\d+)\s*(s|m|h|d|w|mo|yr)/);
    if (m) return new Date(now - parseInt(m[1]) * (map[m[2]] || 0));
    if (/just now/.test(text)) return new Date(now);
    // Full date attempt
    const d = new Date(text);
    if (!isNaN(d)) return d;
    return null;
  }

  function formatDate(d) {
    if (!d) return null;
    const months = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
    const diffDays = Math.floor((Date.now() - d) / 86400000);
    return `${months[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()} (${diffDays}d ago)`;
  }

  // ── engagement bait patterns ──────────────────────────────────────────────
  const BAIT_PATTERNS = [
    // Agree/disagree asks
    /agree or disagree\??/i,
    // Repost asks with artificial scarcity
    /repost if (you|this)/i,
    // Numbered hustle lists
    /^\d+\s+lessons?\s+(i|you|we)\s+(learned?|wish)/im,
    // "I got laid off" / "I got fired" engagement trap
    /i (got|was) (laid off|fired|let go)/i,
    // "Hot take:"
    /^hot take:/im,
    // Line-by-line formatting (one word per line = engagement bait formatting)
  ];

  // Line-by-line bait: 5+ very short lines = engagement bait formatting
  function isLineBait(text) {
    const lines = text.split("\n").map(l => l.trim()).filter(Boolean);
    if (lines.length < 6) return false;
    const shortLines = lines.filter(l => l.split(/\s+/).length <= 4);
    return shortLines.length >= lines.length * 0.7;
  }

  function checkEngagementBait(post, contentEl) {
    if (!config.flagEngagementBait || post.dataset.noahLiBait) return;
    post.dataset.noahLiBait = "1";
    const text = contentEl.textContent || "";
    for (const pat of BAIT_PATTERNS) {
      if (pat.test(text)) {
        injectStyles();
        const badge = document.createElement("span");
        badge.className = "noah-li-badge bait";
        badge.textContent = "◈ engagement bait detected";
        contentEl.prepend(badge);
        return;
      }
    }
    if (isLineBait(text)) {
      injectStyles();
      const badge = document.createElement("span");
      badge.className = "noah-li-badge bait";
      badge.textContent = "◈ hustle-bait formatting";
      contentEl.prepend(badge);
    }
  }

  // ── main post scanner ─────────────────────────────────────────────────────
  function scanPost(post) {
    if (post.dataset.noahLiScanned) return;
    post.dataset.noahLiScanned = "1";

    // Sponsored
    if (isSponsored(post)) {
      if (config.hideSponsored) {
        post.style.setProperty("display", "none", "important");
        send({ type: "safety.event", kind: "social_sponsored_hidden", platform: "linkedin" });
        return;
      }
      injectStyles();
      const actor = post.querySelector(".update-components-actor, .feed-shared-actor");
      if (actor) {
        const badge = document.createElement("span");
        badge.className = "noah-li-badge sponsored";
        badge.textContent = "◈ sponsored";
        actor.insertAdjacentElement("afterend", badge);
      }
      return;
    }

    // Absolute dates
    if (config.showAbsoluteDates) {
      const timeEls = post.querySelectorAll(
        '.update-components-actor__sub-description time, ' +
        '.feed-shared-actor__sub-description time, ' +
        '.update-v2-social-activity time, ' +
        'span[aria-label*=" ago"], a[href*="/posts/"] span'
      );
      for (const t of timeEls) {
        if (t.dataset.noahLiDate) continue;
        t.dataset.noahLiDate = "1";
        const text = t.getAttribute("datetime") || t.textContent || "";
        const parsed = parseLIDate(text);
        if (parsed) {
          injectStyles();
          const badge = document.createElement("span");
          badge.className = "noah-li-badge date";
          badge.textContent = formatDate(parsed);
          t.insertAdjacentElement("afterend", badge);
        }
      }
    }

    // Engagement bait
    const contentEl = post.querySelector(
      '.update-components-text, .feed-shared-update-v2__description, .attributed-text-segment-list'
    );
    if (contentEl) checkEngagementBait(post, contentEl);

    // Reactions noise
    if (config.hideReactions) {
      const reactions = post.querySelectorAll('.social-details-social-counts, .reactions-count');
      reactions.forEach(el => el.style.setProperty("opacity", "0.3", "important"));
    }
  }

  // ── job posting staleness ─────────────────────────────────────────────────
  function scanJobPostings() {
    if (!config.flagStaleJobs) return;
    const jobs = document.querySelectorAll(
      '.job-card-container:not([data-noah-li-job]), .jobs-unified-top-card:not([data-noah-li-job])'
    );
    for (const job of jobs) {
      job.dataset.noahLiJob = "1";
      const dateEl = job.querySelector('.job-card-container__listed-status, .jobs-unified-top-card__posted-date, [class*="listed-date"]');
      if (!dateEl) continue;
      const parsed = parseLIDate(dateEl.textContent || "");
      if (!parsed) continue;
      const diffDays = Math.floor((Date.now() - parsed) / 86400000);
      if (diffDays >= config.staleJobDays) {
        injectStyles();
        const badge = document.createElement("span");
        badge.className = "noah-li-badge stale";
        badge.textContent = `◈ ${diffDays}d old — may be filled`;
        dateEl.insertAdjacentElement("afterend", badge);
      }
    }
  }

  // ── clean feed: remove "People also viewed", "Suggested", "Trending" ──────
  function cleanFeedNoise() {
    if (!config.cleanFeed) return;
    const NOISE_SELECTORS = [
      '.scaffold-layout__aside [class*="suggested" i]',
      '.feed-follows-module',
      '.scaffold-layout__aside [class*="trending" i]',
      '[data-test-id="feed-identity-module"]',
      '.news-module',
    ];
    for (const sel of NOISE_SELECTORS) {
      document.querySelectorAll(`${sel}:not([data-noah-li-noise])`).forEach(el => {
        el.dataset.noahLiNoise = "1";
        el.style.setProperty("display", "none", "important");
      });
    }
  }

  // ── mutation observer ─────────────────────────────────────────────────────
  let scanQueued = false;
  function queueScan() {
    if (scanQueued) return;
    scanQueued = true;
    requestAnimationFrame(() => {
      scanQueued = false;
      const posts = document.querySelectorAll(
        '.feed-shared-update-v2:not([data-noah-li-scanned]), ' +
        '.update-components-wrapper:not([data-noah-li-scanned]), ' +
        '.occludable-update:not([data-noah-li-scanned])'
      );
      posts.forEach(scanPost);
      scanJobPostings();
      cleanFeedNoise();
    });
  }

  const mo = new MutationObserver(queueScan);
  mo.observe(document.documentElement, { childList: true, subtree: true });
  // initial scan driven by config fetch above (queueScan in .then / .catch)

  document.addEventListener("noah-linkedin-config", (e) => {
    try { const next = JSON.parse(e.detail || "{}"); config = { ...config, ...next }; } catch {}
    if (!config.enabled) mo.disconnect();
  });
  document.dispatchEvent(new CustomEvent("noah-linkedin-ready"));
})();
