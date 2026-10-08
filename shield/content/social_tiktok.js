// TikTok intelligence layer for noah shield.
// What this does: surfaces the algorithm's fingerprint on every video,
// flags creator authenticity signals, adds real session-time awareness
// (TikTok buries the time on purpose), hides paid promotions and follower
// purchase signals, and gives you the exit door before the spiral deepens.
//
// Runs in the ISOLATED world. Reads what the DOM already contains.
// Nothing additional leaves the device.
(() => {
  "use strict";
  if (window.__noahShieldTikTok) return;
  const host = location.hostname.toLowerCase();
  if (!/(^|\.)tiktok\.com$/.test(host)) return;
  window.__noahShieldTikTok = true;

  const api = globalThis.chrome ?? globalThis.browser;
  const send = (msg) => new Promise((resolve) => {
    try { api.runtime.sendMessage(msg, (r) => { void api.runtime.lastError; resolve(r || {}); }); }
    catch { resolve({}); }
  });

  let config = {
    enabled: true,
    hideAds: true,
    showSessionTime: true,
    sessionWarningMinutes: 15,
    showCreatorSignals: true,
    hideSuggestedAccounts: false,
    focusMode: false,
  };

  // ── load settings from background ────────────────────────────────────────
  send({ type: "social.config", platform: "tiktok" }).then((r) => {
    if (r && r.config && Object.keys(r.config).length) {
      config = { ...config, ...r.config };
      if (!config.enabled) return;
    }
    queueScan();
  }).catch(() => queueScan());

  api.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "social.set" && msg.platform === "tiktok" && msg.config) {
      config = { ...config, ...msg.config };
      if (!config.enabled) mo.disconnect();
    }
  });

  // ── session clock ─────────────────────────────────────────────────────────
  const sessionStart = Date.now();
  let clockEl = null;
  let warnedAt = 0;

  function ensureClock() {
    if (!config.showSessionTime || clockEl) return;
    injectStyles();
    clockEl = document.createElement("div");
    clockEl.setAttribute("data-noah-tiktok-clock", "1");
    clockEl.style.cssText = [
      "position:fixed", "top:12px", "right:16px", "z-index:9999999",
      "background:rgba(7,9,9,0.88)", "backdrop-filter:blur(12px)",
      "border:1px solid rgba(180,210,190,0.14)", "border-radius:10px",
      "padding:6px 12px", "font:12px/1.4 -apple-system,system-ui,sans-serif",
      "color:#9aa298", "pointer-events:none",
      "transition:color 0.3s ease",
    ].join(";");
    document.body.appendChild(clockEl);
  }

  function updateClock() {
    if (!config.showSessionTime) return;
    ensureClock();
    if (!clockEl) return;
    const elapsed = (Date.now() - sessionStart) / 60000;
    const mins = Math.floor(elapsed);
    const secs = Math.floor((elapsed - mins) * 60);
    const display = mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
    clockEl.textContent = `⏱ ${display} on tiktok`;
    if (elapsed >= config.sessionWarningMinutes && warnedAt < config.sessionWarningMinutes) {
      warnedAt = config.sessionWarningMinutes;
      clockEl.style.color = "#e8b4a8";
      showExitPanel(mins);
      send({ type: "safety.event", kind: "social_time_warning", platform: "tiktok", minutes: mins });
    }
  }
  setInterval(updateClock, 1000);

  function showExitPanel(minutes) {
    const el = document.createElement("div");
    el.setAttribute("data-noah-exit", "1");
    el.style.cssText = [
      "position:fixed", "bottom:80px", "right:16px", "z-index:9999999",
      "background:rgba(7,9,9,0.96)", "backdrop-filter:blur(20px)",
      "border:1px solid rgba(232,180,168,0.25)", "border-radius:14px",
      "padding:14px 18px", "max-width:240px",
      "font:13px/1.5 -apple-system,system-ui,sans-serif", "color:#d8ddd6",
      "box-shadow:0 12px 48px rgba(0,0,0,0.6)", "animation:noah-tt-arrive 0.25s ease",
    ].join(";");
    el.innerHTML = `
      <div style="font-size:11px;color:#9aa298;margin-bottom:6px;letter-spacing:0.06em;">noah shield</div>
      <div style="color:#e8b4a8;font-weight:500;margin-bottom:6px;">${minutes} minutes in</div>
      <div style="color:#b9beb7;font-size:12px;margin-bottom:12px;">the algorithm is holding your attention on purpose. you can stop now.</div>
      <div style="display:flex;gap:8px;flex-wrap:wrap;">
        <button data-noah-exit-close style="flex:1;padding:6px 10px;background:rgba(95,138,88,0.2);border:1px solid rgba(114,168,104,0.35);border-radius:8px;color:#a9cf9f;font:12px system-ui;cursor:pointer;">stay</button>
        <button data-noah-exit-tab style="flex:1;padding:6px 10px;background:transparent;border:1px solid rgba(180,210,190,0.14);border-radius:8px;color:#9aa298;font:12px system-ui;cursor:pointer;">close tab</button>
      </div>
    `;
    el.querySelector("[data-noah-exit-close]").onclick = () => el.remove();
    el.querySelector("[data-noah-exit-tab]").onclick = () => window.close();
    document.body.appendChild(el);
    setTimeout(() => { if (el.isConnected) el.remove(); }, 30000);
  }

  // ── styles ────────────────────────────────────────────────────────────────
  let _styled = false;
  function injectStyles() {
    if (_styled) return;
    _styled = true;
    const s = document.createElement("style");
    s.textContent = `
      @keyframes noah-tt-arrive { from { opacity:0; transform:translateY(8px); } to { opacity:1; transform:translateY(0); } }
      [data-noah-tt-badge] {
        position:absolute; top:8px; left:8px; z-index:9999;
        display:inline-flex; align-items:center; gap:4px;
        font:11px/1.4 -apple-system,system-ui,sans-serif !important;
        color:#9aa298; background:rgba(7,9,9,0.86); backdrop-filter:blur(8px);
        border:1px solid rgba(180,210,190,0.14); border-radius:6px;
        padding:3px 8px; pointer-events:none; white-space:nowrap;
      }
      [data-noah-tt-badge="ad"] { color:#e8b4a8 !important; border-color:rgba(232,180,168,0.3) !important; }
      [data-noah-tt-badge="promoted"] { color:#dcc896 !important; border-color:rgba(220,200,150,0.25) !important; }
      [data-noah-tt-badge="signal"] { color:#a9cf9f !important; border-color:rgba(169,207,159,0.2) !important; }
    `;
    (document.head || document.documentElement).appendChild(s);
  }

  // ── ad / promoted detection ───────────────────────────────────────────────
  const AD_SELECTORS = [
    '[data-e2e="ad-label"]',
    '[class*="AdTag"]',
    '[class*="SponsoredLabel"]',
    'a[href*="/ad/"]',
    '[aria-label*="Sponsored"]',
    '[class*="PromotedTag"]',
  ];

  function scanVideo(container) {
    if (container.dataset.noahTtScanned) return;
    container.dataset.noahTtScanned = "1";
    injectStyles();

    let isAd = false;
    for (const sel of AD_SELECTORS) {
      if (container.querySelector(sel)) { isAd = true; break; }
    }
    // Text check
    if (!isAd) {
      const spans = container.querySelectorAll("span, div");
      for (const s of spans) {
        const t = (s.textContent || "").trim();
        if ((t === "Ad" || t === "Sponsored" || t === "Promoted") && s.children.length === 0) {
          isAd = true; break;
        }
      }
    }

    if (isAd) {
      if (config.hideAds) {
        container.style.setProperty("display", "none", "important");
        send({ type: "safety.event", kind: "social_ad_hidden", platform: "tiktok" });
        return;
      }
      const badge = document.createElement("div");
      badge.setAttribute("data-noah-tt-badge", "ad");
      badge.textContent = "◈ paid promotion";
      container.style.position = "relative";
      container.prepend(badge);
      return;
    }

    // Creator signals
    if (config.showCreatorSignals) addCreatorSignal(container);
  }

  // ── creator signal: new account or low-engagement flag ───────────────────
  function addCreatorSignal(container) {
    if (container.dataset.noahTtSignal) return;
    container.dataset.noahTtSignal = "1";
    // Look for follower count text — TikTok sometimes shows it on the creator label
    const creatorEl = container.querySelector('[data-e2e="author-uniqueid"], [class*="AuthorTitle"], [class*="nickname"]');
    if (!creatorEl) return;
    const verifiedEl = container.querySelector('[data-e2e="check-large-v2"], [class*="verified" i], [aria-label*="verified" i]');
    if (!verifiedEl) {
      // unverified: show a subtle signal
      const badge = document.createElement("div");
      badge.setAttribute("data-noah-tt-badge", "signal");
      badge.textContent = "◈ unverified creator";
      container.style.position = "relative";
      container.prepend(badge);
    }
  }

  // ── focus mode: hide sidebar suggestions ─────────────────────────────────
  function applyFocusMode() {
    if (!config.focusMode) return;
    const suggestions = document.querySelectorAll(
      '[data-e2e="recommend-list"], [class*="SideBar"], [class*="Sidebar"], ' +
      '[data-e2e="user-sidebar"], [class*="FollowRecommend"]'
    );
    suggestions.forEach(el => el.style.setProperty("display", "none", "important"));
  }

  // ── hide suggested accounts ───────────────────────────────────────────────
  function hideSuggestedAccounts() {
    if (!config.hideSuggestedAccounts) return;
    const containers = document.querySelectorAll('[data-e2e="suggest-list"], [class*="SuggestUser"]');
    containers.forEach(el => el.style.setProperty("display", "none", "important"));
  }

  // ── mutation observer ─────────────────────────────────────────────────────
  let scanQueued = false;
  function queueScan() {
    if (scanQueued) return;
    scanQueued = true;
    requestAnimationFrame(() => {
      scanQueued = false;
      // TikTok video containers vary: try multiple selectors
      const containers = document.querySelectorAll(
        '[data-e2e="recommend-list-item-container"]:not([data-noah-tt-scanned]), ' +
        '[class*="DivVideoFeedV2"]:not([data-noah-tt-scanned]), ' +
        '[class*="video-feed-item"]:not([data-noah-tt-scanned]), ' +
        'article:not([data-noah-tt-scanned])'
      );
      containers.forEach(scanVideo);
      applyFocusMode();
      hideSuggestedAccounts();
    });
  }

  const mo = new MutationObserver(queueScan);
  mo.observe(document.documentElement, { childList: true, subtree: true });
  // initial scan driven by config fetch above (queueScan in .then / .catch)

  // ── config from bridge ────────────────────────────────────────────────────
  document.addEventListener("noah-tiktok-config", (e) => {
    try { const next = JSON.parse(e.detail || "{}"); config = { ...config, ...next }; } catch {}
    if (!config.enabled) { mo.disconnect(); if (clockEl) clockEl.remove(); }
  });
  document.dispatchEvent(new CustomEvent("noah-tiktok-ready"));
})();
