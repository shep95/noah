// YouTube intelligence layer for noah shield.
// Cleans the recommendation loop: removes the sidebar autoplay trap,
// hides homepage distractions, surfaces actual upload dates (YouTube
// shows relative time to maximize urgency), flags sponsored segments
// in descriptions, and detects clickbait engagement signals.
//
// Runs in the ISOLATED world.
(() => {
  "use strict";
  if (window.__noahShieldYouTube) return;
  const host = location.hostname.toLowerCase();
  if (!/(^|\.)youtube\.com$/.test(host)) return;
  window.__noahShieldYouTube = true;

  const api = globalThis.chrome ?? globalThis.browser;
  const send = (msg) => new Promise((resolve) => {
    try { api.runtime.sendMessage(msg, (r) => { void api.runtime.lastError; resolve(r || {}); }); }
    catch { resolve({}); }
  });

  let config = {
    enabled: true,
    cleanSidebar: true,
    cleanHome: false,
    showUploadDate: true,
    flagSponsoredSegments: true,
    flagClickbait: true,
    hideShortsTab: false,
    autoplayGuard: true,
    cleanEndscreen: true,
    skipAds: true,        // seek-to-end stealth skip — appears to YouTube as "ad watched"
  };

  // ── load settings from background ────────────────────────────────────────
  send({ type: "social.config", platform: "youtube" }).then((r) => {
    if (r && r.config && Object.keys(r.config).length) {
      config = { ...config, ...r.config };
      if (!config.enabled) return;
    }
    queueScan();
  }).catch(() => queueScan());

  api.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "social.set" && msg.platform === "youtube" && msg.config) {
      config = { ...config, ...msg.config };
      if (!config.enabled) mo.disconnect();
    }
  });

  // ── styles ────────────────────────────────────────────────────────────────
  let _styled = false;
  let _styleEl = null;
  function injectStyles() {
    if (_styled) return;
    _styled = true;
    _styleEl = document.createElement("style");
    _styleEl.id = "noah-yt-styles";
    _styleEl.textContent = `
      @keyframes noah-yt-in { from { opacity:0; transform:translateY(3px); } to { opacity:1; transform:none; } }
      .noah-yt-badge {
        display:inline-flex !important; align-items:center; gap:4px;
        font:11px/1.4 -apple-system,system-ui,sans-serif !important;
        color:#9aa298 !important; background:rgba(7,9,9,0.9) !important;
        border:1px solid rgba(180,210,190,0.14) !important;
        border-radius:5px !important; padding:2px 7px !important;
        margin-top:4px !important; pointer-events:none;
        white-space:nowrap; display:block !important;
      }
      .noah-yt-badge.date { color:#a9cf9f !important; border-color:rgba(169,207,159,0.2) !important; }
      .noah-yt-badge.sponsor { color:#dcc896 !important; border-color:rgba(220,200,150,0.22) !important; }
      .noah-yt-badge.clickbait { color:#e8b4a8 !important; border-color:rgba(232,180,168,0.25) !important; }

      /* Clean sidebar: recommendation trap off */
      html.noah-yt-sidebar-hidden ytd-watch-next-secondary-results-renderer { display:none !important; }

      /* Clean endscreen */
      .noah-yt-endscreen-hidden .ytp-ce-element { display:none !important; }

      /* Autoplay guard */
      .noah-yt-autoplay-guard .ytp-autonav-endscreen-upnext-button { display:none !important; }
    `;
    (document.head || document.documentElement).appendChild(_styleEl);
  }

  // ── sidebar cleaning ──────────────────────────────────────────────────────
  // Bug fix: previously set document.documentElement.id = "noah-yt-sidebar-hidden"
  // which overwrote YouTube's own <html> id. Use classList instead — YT never
  // sets a class on the root, and removing the class is clean when disabled.
  function applySidebarClean() {
    if (!config.cleanSidebar) {
      document.documentElement.classList.remove("noah-yt-sidebar-hidden");
      return;
    }
    document.documentElement.classList.add("noah-yt-sidebar-hidden");
  }

  // ── endscreen cleaning ────────────────────────────────────────────────────
  // Bug fix: previously set player.id = "noah-yt-endscreen-hidden" which
  // overwrote #movie_player — YouTube's own JS looks up the player by that id.
  // Use classList on the player element instead.
  function applyEndscreenClean() {
    if (!config.cleanEndscreen) return;
    const player = document.querySelector("#movie_player");
    if (player) player.classList.add("noah-yt-endscreen-hidden");
  }

  // ── autoplay guard ────────────────────────────────────────────────────────
  // Guard: use a dataset flag so we click at most once per page load.
  // Without it the observer could re-fire before aria-checked updates and
  // toggle the button back on.
  function applyAutoplayGuard() {
    if (!config.autoplayGuard) return;
    if (document.documentElement.dataset.noahYtAutoplay) return;
    const autoplayToggle = document.querySelector(".ytp-autonav-toggle-button[aria-checked='true']");
    if (autoplayToggle) {
      document.documentElement.dataset.noahYtAutoplay = "1";
      autoplayToggle.click();
      send({ type: "safety.event", kind: "yt_autoplay_disabled", platform: "youtube" });
    }
  }

  // ── upload date: parse YouTube's relative timestamps ─────────────────────
  function parseYTDate(text) {
    text = text.trim().toLowerCase();
    // YouTube sometimes gives "streamed X ago" or "X ago"
    text = text.replace(/^streamed\s+/i, "").replace(/\s+ago$/, "");
    const now = Date.now();
    const units = {
      second: 1000, minute: 60000, hour: 3600000,
      day: 86400000, week: 604800000, month: 2592000000, year: 31536000000
    };
    const m = text.match(/^(\d+)\s+(second|minute|hour|day|week|month|year)s?/);
    if (m) return new Date(now - parseInt(m[1]) * (units[m[2]] || 0));
    // Full dates like "Nov 3, 2021"
    const d = new Date(text);
    if (!isNaN(d)) return d;
    return null;
  }

  function formatDate(d) {
    if (!d) return null;
    const months = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
    const now = new Date();
    const diffDays = Math.floor((now - d) / 86400000);
    const age = diffDays < 7 ? `${diffDays}d old`
      : diffDays < 30 ? `${Math.floor(diffDays / 7)}w old`
      : diffDays < 365 ? `${Math.floor(diffDays / 30)}mo old`
      : `${Math.floor(diffDays / 365)}y old`;
    return `${months[d.getMonth()]} ${d.getDate()}, ${d.getFullYear()} · ${age}`;
  }

  // Bug fix: previously set dataset.noahYtDate on the primaryInfo element once.
  // On SPA navigation (yt-navigate-finish) YouTube re-uses the same DOM element
  // for the next video, so the old flag would block the date from updating.
  // Now we stamp the current href on the element — a different href means re-scan.
  function addUploadDate(videoEl) {
    if (!config.showUploadDate) return;
    const currentHref = location.pathname + location.search;
    if (videoEl.dataset.noahYtDate === currentHref) return;
    // Remove any old badge before adding a fresh one
    videoEl.querySelectorAll(".noah-yt-badge.date").forEach((b) => b.remove());
    videoEl.dataset.noahYtDate = currentHref;
    injectStyles();

    // Try the <ytd-video-primary-info-renderer> info row
    const dateSpans = videoEl.querySelectorAll(
      '#info-strings yt-formatted-string, #info .style-scope.ytd-video-primary-info-renderer, ' +
      '#published-date span, .style-scope.ytd-video-primary-info-renderer span'
    );
    for (const span of dateSpans) {
      const text = (span.textContent || "").trim();
      if (!text || text.length > 40) continue;
      const parsed = parseYTDate(text);
      if (!parsed) continue;
      const formatted = formatDate(parsed);
      if (!formatted) continue;
      const badge = document.createElement("div");
      badge.className = "noah-yt-badge date";
      badge.textContent = `◈ ${formatted}`;
      span.insertAdjacentElement("afterend", badge);
      break;
    }
  }

  // ── sponsored segment detection in description ────────────────────────────
  const SPONSOR_PATTERNS = [
    /sponsored by/i, /this video is sponsored/i, /thanks to .{3,40} for sponsoring/i,
    /use code .{2,20} for \d+% off/i, /affiliate link/i, /ad:/, /\[ad\]/i,
    /check out our sponsor/i, /brought to you by/i,
  ];

  function flagSponsoredDescription() {
    if (!config.flagSponsoredSegments) return;
    const desc = document.querySelector(
      '#description ytd-text-inline-expander, #description-inner, ytd-video-secondary-info-renderer #description'
    );
    if (!desc || desc.dataset.noahYtSponsor) return;
    desc.dataset.noahYtSponsor = "1";
    const text = desc.textContent || "";
    for (const pat of SPONSOR_PATTERNS) {
      if (pat.test(text)) {
        injectStyles();
        const badge = document.createElement("div");
        badge.className = "noah-yt-badge sponsor";
        badge.textContent = "◈ description contains sponsored segment disclosure";
        desc.prepend(badge);
        break;
      }
    }
  }

  // ── clickbait detection ───────────────────────────────────────────────────
  const CLICKBAIT_PATTERNS = [
    /you won'?t believe/i, /gone wrong/i, /gone sexual/i, /exposed/i,
    /secret (they|the \w+ doesn'?t)/i, /they don'?t want you to (know|see)/i,
    /must see/i, /\*\*\*/,
    /i (quit|got fired|left|walked out)/i,
    /\d+\s*(tricks?|hacks?|tips?|secrets?|things?) (that|to|you)/i,
  ];

  function checkClickbait(titleEl, videoCard) {
    if (!config.flagClickbait || videoCard.dataset.noahYtClickbait) return;
    videoCard.dataset.noahYtClickbait = "1";
    const title = titleEl.textContent || "";
    for (const pat of CLICKBAIT_PATTERNS) {
      if (pat.test(title)) {
        injectStyles();
        const badge = document.createElement("div");
        badge.className = "noah-yt-badge clickbait";
        badge.textContent = "◈ clickbait pattern detected";
        titleEl.insertAdjacentElement("afterend", badge);
        break;
      }
    }
  }

  // ── homepage video cards ──────────────────────────────────────────────────
  function scanVideoCards() {
    const cards = document.querySelectorAll(
      'ytd-rich-item-renderer:not([data-noah-yt-scanned]), ' +
      'ytd-video-renderer:not([data-noah-yt-scanned]), ' +
      'ytd-compact-video-renderer:not([data-noah-yt-scanned])'
    );
    for (const card of cards) {
      card.dataset.noahYtScanned = "1";
      // Clickbait check on recommendation cards
      const titleEl = card.querySelector("#video-title, h3 a, yt-formatted-string#video-title");
      if (titleEl && config.flagClickbait) checkClickbait(titleEl, card);
    }
  }

  // ── hide Shorts tab ───────────────────────────────────────────────────────
  function hideShortsTab() {
    if (!config.hideShortsTab) return;
    const tabs = document.querySelectorAll(
      'ytd-guide-entry-renderer:not([data-noah-shorts-checked])'
    );
    for (const tab of tabs) {
      tab.dataset.noahShortsChecked = "1";
      const link = tab.querySelector("a");
      if (link && /shorts/i.test(link.getAttribute("title") || link.textContent || "")) {
        tab.style.setProperty("display", "none", "important");
      }
    }
  }

  // ── watch page scan (fires when navigating to a video) ───────────────────
  function scanWatchPage() {
    const isPrimary = document.querySelector("ytd-watch-flexy, ytd-watch-metadata");
    if (!isPrimary) return;

    applySidebarClean();
    applyEndscreenClean();
    applyAutoplayGuard();
    flagSponsoredDescription();

    const primaryInfo = document.querySelector("ytd-video-primary-info-renderer, ytd-watch-metadata");
    if (primaryInfo) addUploadDate(primaryInfo);
  }

  // ── ad skip: seek-to-end stealth approach ────────────────────────────────
  // How YouTube ads work at the mechanism level:
  //   1. The player switches its <video> src to an ad stream URL.
  //   2. It monitors video.currentTime; when currentTime >= duration, the
  //      ad "completed" callback fires and it loads the main content.
  //   3. YouTube's anti-adblock scanner checks whether ad network requests
  //      completed, whether ad DOM elements were removed, and whether JS
  //      ad objects were deleted.
  //
  // Stealth approach: we do NOT block the ad network request. We do NOT remove
  // ad DOM elements. We ONLY advance the playhead to duration. From YouTube's
  // perspective this is indistinguishable from the user watching and it ending.
  // The skip button (if present) is also clicked as a natural-looking fallback.
  //
  // Anti-detection hardening:
  //   - Wait one rAF cycle before acting so we don't fire inside YouTube's own
  //     event handler (which could show up in stack traces their monitoring reads)
  //   - Use a small setTimeout (80–180ms) to mimic human reaction latency
  //   - Never remove the .video-ads container or mutation-protected ad elements
  //   - If YouTube's skip button appears, prefer clicking it (cleanest signal)
  let _adSkipArmed = false;
  function trySkipAd() {
    if (!config.skipAds) return;

    // Priority 1: click the skip button if it exists and is clickable.
    const skipBtn = document.querySelector(
      ".ytp-skip-ad-button:not([disabled]), " +
      ".ytp-ad-skip-button:not([disabled]), " +
      ".ytp-ad-skip-button-modern:not([disabled])"
    );
    if (skipBtn) {
      skipBtn.click();
      send({ type: "safety.event", kind: "yt_ad_skip_button", platform: "youtube" });
      return;
    }

    // Priority 2: detect ad playing via player state and seek-to-end.
    // YouTube marks the player container with specific ad attributes/classes.
    const player = document.querySelector(".html5-video-player.ad-showing, " +
      ".html5-video-player.ad-interrupting");
    if (!player) return;

    const video = player.querySelector("video");
    if (!video || !isFinite(video.duration) || video.duration <= 0) return;
    if (video.currentTime >= video.duration - 0.1) return; // already done

    // Seek to just before the end. Jumping exactly to duration can stall;
    // landing 0.1s before lets the timeupdate fire naturally and YouTube's
    // completion handler picks it up within one frame.
    const targetTime = Math.max(0, video.duration - 0.1);
    try {
      video.currentTime = targetTime;
      send({ type: "safety.event", kind: "yt_ad_skip_seek", platform: "youtube" });
    } catch {
      // Cross-origin video element (unusual but possible in embedded scenarios).
    }
  }

  // Arm the skip loop once per ad detection. We arm on MutationObserver signals
  // that indicate ad state changed, and disarm once the ad class is gone.
  let _adCheckInterval = null;
  function armAdSkip() {
    if (_adSkipArmed || !config.skipAds) return;
    _adSkipArmed = true;
    // Poll lightly (every 300ms) — aggressive polling (< 100ms) is itself a
    // detectable signal. 300ms means a max 0.3s of ad before it's gone.
    _adCheckInterval = setInterval(() => {
      const stillAd = document.querySelector(
        ".html5-video-player.ad-showing, .html5-video-player.ad-interrupting, " +
        ".ytp-skip-ad-button, .ytp-ad-skip-button, .ytp-ad-skip-button-modern"
      );
      if (!stillAd) {
        clearInterval(_adCheckInterval);
        _adSkipArmed = false;
        return;
      }
      trySkipAd();
    }, 300);
  }

  // Detect ad entry via DOM signals — this fires long before the video plays.
  const _adTriggers = [
    ".ytp-ad-player-overlay", ".ad-showing", ".ad-interrupting",
    ".ytp-ad-module", ".video-ads.ytp-ad-module",
  ];

  // ── mutation observer ─────────────────────────────────────────────────────
  let scanQueued = false;
  function queueScan() {
    if (scanQueued) return;
    scanQueued = true;
    requestAnimationFrame(() => {
      scanQueued = false;
      scanVideoCards();
      hideShortsTab();
      scanWatchPage();
      // Ad skip: arm when any ad-indicating DOM element appears.
      // We check here — inside the rAF — so we're never in YouTube's own
      // synchronous event stack when we first touch the video element.
      if (config.skipAds) {
        const hasAd = _adTriggers.some((sel) => document.querySelector(sel));
        if (hasAd) armAdSkip();
      }
    });
  }

  const mo = new MutationObserver(queueScan);
  mo.observe(document.documentElement, { childList: true, subtree: true });
  // YouTube is an SPA — also hook navigation.
  // Clear the autoplay guard flag on each navigation so applyAutoplayGuard
  // can fire again for the new video's player instance.
  // Also disarm any running ad skip loop — the previous ad video element
  // is gone after navigation and the interval would spin uselessly.
  window.addEventListener("yt-navigate-finish", () => {
    delete document.documentElement.dataset.noahYtAutoplay;
    clearInterval(_adCheckInterval);
    _adSkipArmed = false;
    queueScan();
  });
  // initial scan driven by config fetch above (queueScan in .then / .catch)

  document.addEventListener("noah-youtube-config", (e) => {
    try { const next = JSON.parse(e.detail || "{}"); config = { ...config, ...next }; } catch {}
    if (!config.enabled) mo.disconnect();
  });
  document.dispatchEvent(new CustomEvent("noah-youtube-ready"));
})();
