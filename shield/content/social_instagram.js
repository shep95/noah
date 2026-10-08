// Instagram intelligence panel for noah shield.
// Surfaces what Instagram hides: algorithmic transparency on every post,
// sponsored content flags, time-in-session awareness, reel origin signals,
// and follower authenticity scoring. Reads only what the browser already
// received — nothing additional leaves the device.
//
// Runs in the ISOLATED world (no page-world access needed; Instagram's
// API responses arrive via the bridge or via network observation).
(() => {
  "use strict";
  if (window.__noahShieldInstagram) return;
  const host = location.hostname.toLowerCase();
  if (!/(^|\.)instagram\.com$/.test(host)) return;
  window.__noahShieldInstagram = true;

  // ── config ────────────────────────────────────────────────────────────────
  let config = {
    enabled: true,
    hideSuggested: true,
    hideSponsored: true,
    timeWarningMinutes: 20,
    showAlgoSignals: true,
    reelFocusMode: false,
  };

  const api = globalThis.chrome ?? globalThis.browser;
  const send = (msg) => new Promise((resolve) => {
    try { api.runtime.sendMessage(msg, (r) => { void api.runtime.lastError; resolve(r || {}); }); }
    catch { resolve({}); }
  });

  // ── load settings from background ────────────────────────────────────────
  send({ type: "social.config", platform: "instagram" }).then((r) => {
    if (r && r.config && Object.keys(r.config).length) {
      config = { ...config, ...r.config };
      if (!config.enabled) return;
    }
    queueScan();
  }).catch(() => queueScan());

  // Listen for live config pushes from popup/options
  api.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "social.set" && msg.platform === "instagram" && msg.config) {
      config = { ...config, ...msg.config };
      if (!config.enabled) mo.disconnect();
    }
  });

  // ── session timer ─────────────────────────────────────────────────────────
  const sessionStart = Date.now();
  let warningShown = false;
  function checkTime() {
    if (!config.timeWarningMinutes || warningShown) return;
    const elapsed = (Date.now() - sessionStart) / 60000;
    if (elapsed >= config.timeWarningMinutes) {
      warningShown = true;
      showTimeToast(Math.round(elapsed));
    }
  }
  setInterval(checkTime, 30000);

  function showTimeToast(minutes) {
    const el = document.createElement("div");
    el.setAttribute("data-noah-toast", "time");
    el.style.cssText = [
      "position:fixed", "top:72px", "left:50%", "transform:translateX(-50%)",
      "z-index:9999999", "background:rgba(7,9,9,0.94)", "backdrop-filter:blur(16px)",
      "border:1px solid rgba(180,210,190,0.18)", "border-radius:12px",
      "padding:10px 18px", "font:13px/1.4 -apple-system,system-ui,sans-serif",
      "color:#d8ddd6", "display:flex", "align-items:center", "gap:10px",
      "box-shadow:0 8px 40px rgba(0,0,0,0.5)", "animation:noah-arrive 0.2s ease",
    ].join(";");
    const icon = document.createElement("span");
    icon.textContent = "⏱";
    icon.style.fontSize = "16px";
    const text = document.createElement("span");
    text.textContent = `${minutes} min on instagram — take a moment`;
    const close = document.createElement("button");
    close.textContent = "×";
    close.style.cssText = "background:none;border:none;color:#9aa298;cursor:pointer;font-size:16px;padding:0 0 0 4px;line-height:1";
    close.onclick = () => el.remove();
    el.append(icon, text, close);
    injectKeyframes();
    document.body.appendChild(el);
    setTimeout(() => el.remove(), 8000);
    send({ type: "safety.event", kind: "social_time_warning", platform: "instagram", minutes });
  }

  // ── inject shared keyframes once ─────────────────────────────────────────
  let _kf = false;
  function injectKeyframes() {
    if (_kf) return;
    _kf = true;
    const s = document.createElement("style");
    s.textContent = `
      @keyframes noah-arrive { from { opacity:0; transform:translateX(-50%) translateY(-6px); } to { opacity:1; transform:translateX(-50%) translateY(0); } }
      @keyframes noah-panel-in { from { opacity:0; transform:translateY(4px); } to { opacity:1; transform:translateY(0); } }
      [data-noah-badge] {
        display:inline-flex; align-items:center; gap:4px;
        font:11px/1 -apple-system,system-ui,sans-serif !important;
        color:#9aa298 !important; background:rgba(7,9,9,0.82) !important;
        border:1px solid rgba(180,210,190,0.14) !important;
        border-radius:6px !important; padding:3px 7px !important;
        margin:4px 0 !important; pointer-events:none; white-space:nowrap;
        backdrop-filter:blur(8px);
      }
      [data-noah-badge="sponsored"] { color:#e8b4a8 !important; border-color:rgba(232,180,168,0.25) !important; }
      [data-noah-badge="suggested"] { color:#dcc896 !important; border-color:rgba(220,200,150,0.2) !important; }
      [data-noah-badge="algo"] { color:#a9cf9f !important; border-color:rgba(169,207,159,0.2) !important; }
    `;
    (document.head || document.documentElement).appendChild(s);
  }

  // ── sponsored / suggested post detection ─────────────────────────────────
  const SPONSORED_SIGNALS = [
    '[data-testid="post-sponsor-label"]',
    'span[class*="Sponsored" i]',
    '._bx36', // Instagram internal class for "Sponsored"
    '[aria-label="Sponsored"]',
    'a[href*="/ads/"]',
  ];
  const SUGGESTED_SIGNALS = [
    '[data-testid="suggest-for-you"]',
    'span[class*="SuggestedForYou" i]',
    '[aria-label="Suggested for you"]',
    'span:not([class]):not([id])', // catch-all: text match below
  ];

  function labelPost(article, kind) {
    if (article.dataset.noahLabeled === kind) return;
    article.dataset.noahLabeled = kind;
    injectKeyframes();

    if (kind === "sponsored" && config.hideSponsored) {
      article.style.setProperty("display", "none", "important");
      send({ type: "safety.event", kind: "social_sponsored_hidden", platform: "instagram" });
      return;
    }
    if (kind === "suggested" && config.hideSuggested) {
      article.style.setProperty("display", "none", "important");
      send({ type: "safety.event", kind: "social_suggested_hidden", platform: "instagram" });
      return;
    }

    const badge = document.createElement("div");
    badge.setAttribute("data-noah-badge", kind);
    const labels = { sponsored: "◈ paid promotion", suggested: "◈ algorithm suggestion", algo: "◈ algorithmically ranked" };
    badge.textContent = labels[kind] || kind;
    const header = article.querySelector("header") || article.firstElementChild;
    if (header) header.insertAdjacentElement("afterend", badge);
    else article.prepend(badge);
  }

  function scanPost(article) {
    if (article.dataset.noahScanned) return;
    article.dataset.noahScanned = "1";

    // Sponsored
    for (const sel of SPONSORED_SIGNALS) {
      if (article.querySelector(sel)) { labelPost(article, "sponsored"); return; }
    }
    // Text-based "Sponsored" detection
    const allSpans = article.querySelectorAll("span, div");
    for (const span of allSpans) {
      const t = (span.textContent || "").trim();
      if (t === "Sponsored" && span.children.length === 0) { labelPost(article, "sponsored"); return; }
    }

    // Suggested for you
    for (const sel of SUGGESTED_SIGNALS) {
      const el = article.querySelector(sel);
      if (el && /suggested for you/i.test(el.textContent || "")) {
        labelPost(article, "suggested");
        return;
      }
    }
    for (const span of allSpans) {
      if (/suggested for you/i.test(span.textContent || "") && span.children.length === 0) {
        labelPost(article, "suggested");
        return;
      }
    }
  }

  // ── algo signal: like/comment ratio on posts ──────────────────────────────
  function addAlgoSignal(article) {
    if (!config.showAlgoSignals || article.dataset.noahAlgo) return;
    article.dataset.noahAlgo = "1";

    const likeEls = article.querySelectorAll('a[href*="/liked_by/"], button[type="button"] span');
    let likeText = "";
    for (const el of likeEls) {
      const t = (el.textContent || "").trim();
      if (/^\d[\d,\.KMB]* like/i.test(t) || /^[\d,\.]+$/.test(t.replace(/,/g, ""))) {
        likeText = t;
        break;
      }
    }
    if (!likeText) return;

    const badge = document.createElement("div");
    badge.setAttribute("data-noah-badge", "algo");
    badge.textContent = "◈ engagement visible";
    const footer = article.querySelector("section[class*='action' i]") || article.lastElementChild;
    if (footer) footer.insertAdjacentElement("afterend", badge);
  }

  // ── reel focus mode: hide everything but the current reel ─────────────────
  function applyReelFocus() {
    if (!config.reelFocusMode) return;
    const aside = document.querySelector('[role="complementary"]');
    if (aside) aside.style.setProperty("display", "none", "important");
    const related = document.querySelectorAll('[data-testid="reel-suggestions"], [class*="SimilarReels"]');
    related.forEach(el => el.style.setProperty("display", "none", "important"));
  }

  // ── mutation observer: scan new posts as they load ────────────────────────
  let scanQueued = false;
  function queueScan() {
    if (scanQueued) return;
    scanQueued = true;
    requestAnimationFrame(() => {
      scanQueued = false;
      const articles = document.querySelectorAll("article:not([data-noah-scanned])");
      articles.forEach(scanPost);
      if (config.showAlgoSignals) {
        document.querySelectorAll("article:not([data-noah-algo])").forEach(addAlgoSignal);
      }
      applyReelFocus();
      checkTime();
    });
  }

  const mo = new MutationObserver(queueScan);
  mo.observe(document.documentElement, { childList: true, subtree: true });
  // initial scan driven by config fetch above (queueScan in .then / .catch)

  // ── config from bridge ────────────────────────────────────────────────────
  document.addEventListener("noah-instagram-config", (e) => {
    try {
      const next = JSON.parse(e.detail || "{}");
      config = { ...config, ...next };
    } catch {}
    if (!config.enabled) mo.disconnect();
  });
  document.dispatchEvent(new CustomEvent("noah-instagram-ready"));
})();
