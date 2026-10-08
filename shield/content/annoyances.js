// Annoyances: cookie banners rejected, overlays and newsletter walls
// removed, autoplay stopped, fake countdowns flagged, copying and right-click
// unblocked, and a dark mode for sites that lack one.
(() => {
  "use strict";
  if (window.top !== window && !/consent|cookie|privacy|cmp|sourcepoint|didomi|quantcast|onetrust|trustarc|usercentrics/i.test(location.href)) return;
  const api = globalThis.chrome ?? globalThis.browser;
  const send = (message) => new Promise((resolve) => {
    try { api.runtime.sendMessage(message, (response) => { void api.runtime.lastError; resolve(response || {}); }); } catch { resolve({}); }
  });
  const text = (node) => (node && (node.innerText || node.textContent) || "").replace(/\s+/g, " ").trim();

  function waitForConfig() {
    return new Promise((resolve) => {
      if (window.__noahShieldSiteConfig) return resolve(window.__noahShieldSiteConfig);
      document.addEventListener("noah-shield:site-config", () => resolve(window.__noahShieldSiteConfig), { once: true });
      setTimeout(() => resolve(window.__noahShieldSiteConfig || null), 4000);
    });
  }
  function visible(element) {
    if (!element || !element.isConnected) return false;
    const style = getComputedStyle(element);
    if (style.display === "none" || style.visibility === "hidden" || parseFloat(style.opacity) === 0) return false;
    const box = element.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  }

  // ---- cookie banners: "reject all" first, then the known consent managers ----------
  const REJECT_SELECTORS = [
    "#onetrust-reject-all-handler", ".onetrust-close-btn-handler", "#CybotCookiebotDialogBodyButtonDecline", "#CybotCookiebotDialogBodyLevelButtonLevelOptinDeclineAll",
    "#didomi-notice-disagree-button", ".didomi-continue-without-agreeing", "#truste-consent-required", ".truste-button2", "#uc-btn-deny-banner", "button[data-testid='uc-deny-all-button']",
    ".cmpboxbtnno", "#cmpwelcomebtnno", ".sp_choice_type_13", "button[title='Reject All']", "button[title='Reject all']", ".qc-cmp2-summary-buttons button[mode='secondary']",
    "#cookie-banner .reject", ".cc-btn.cc-deny", ".cc-deny", "button#rejectAll", "button.reject-all", "[data-cookiebanner='reject_button']", "#consent-reject-all", ".js-reject-cookies",
    "button[aria-label='Reject all']", "button[aria-label='Reject All']", "#W0wltc", "button.fc-cta-do-not-consent", ".fc-cta-do-not-consent",
  ];
  const REJECT_TEXT = /^(reject( all)?|decline( all)?|refuse( all)?|only (necessary|essential|required)( cookies)?|necessary (cookies )?only|essential only|continue without (accepting|agreeing)|do not (accept|consent|sell)|no,? thanks|disagree( and close)?|alle ablehnen|ablehnen|nur notwendige|tout refuser|refuser|rechazar( todo)?|rifiuta( tutto)?|weigeren|alles afwijzen|odrzuć( wszystkie)?|отклонить( все)?)$/i;
  const CMP_HINT = /cookie|consent|privacy|gdpr|tracking|personalis|personaliz|datenschutz|zustimm/i;
  let bannerDone = false;
  function rejectBanner() {
    if (bannerDone) return;
    for (const selector of REJECT_SELECTORS) {
      const button = document.querySelector(selector);
      if (button && visible(button)) {
        button.click();
        bannerDone = true;
        send({ type: "safety.event", kind: "banners", amount: 1 });
        return;
      }
    }
    // FIX: check text (cheap string match) before visible() (layout read: getComputedStyle +
    // getBoundingClientRect). Pages with hundreds of buttons only call visible() on those
    // whose label matches the reject pattern — typically 0–3 elements, not hundreds.
    const buttons = Array.from(document.querySelectorAll("button, a[role='button'], input[type='button'], input[type='submit'], [role='button']"))
      .filter((element) => REJECT_TEXT.test(text(element) || element.value || "") && visible(element));
    for (const button of buttons) {
      const container = button.closest("[class*='cookie' i], [id*='cookie' i], [class*='consent' i], [id*='consent' i], [class*='cmp' i], [id*='cmp' i], [class*='gdpr' i], [id*='gdpr' i], [class*='privacy' i], [id*='privacy' i], [aria-label*='cookie' i], [aria-modal='true'], dialog, [role='dialog']");
      if (!container || !CMP_HINT.test(text(container).slice(0, 600))) continue;
      button.click();
      bannerDone = true;
      send({ type: "safety.event", kind: "banners", amount: 1 });
      return;
    }
    // A banner with only "accept" and "settings": open settings, save without consent.
    const settingsButton = Array.from(document.querySelectorAll("button, a")).find((element) => visible(element) && /^(manage|cookie|customi[sz]e|more) (settings|preferences|options|choices)$|^(settings|preferences|customi[sz]e)$/i.test(text(element)) && CMP_HINT.test(text(element.closest("[class*='cookie' i], [id*='cookie' i], [class*='consent' i], [id*='consent' i], dialog, [role='dialog']") || document.body).slice(0, 600)));
    if (settingsButton && !settingsButton.dataset.noahTried) {
      settingsButton.dataset.noahTried = "1";
      settingsButton.click();
      setTimeout(() => {
        const save = Array.from(document.querySelectorAll("button")).find((element) => visible(element) && /^(save( (my )?(settings|preferences|choices))?|confirm( my)? choices|reject all|allow (necessary|selection)|save and (exit|close))$/i.test(text(element)));
        if (save) { save.click(); bannerDone = true; send({ type: "safety.event", kind: "banners", amount: 1 }); }
      }, 900);
    }
  }

  // ---- overlays and walls ---------------------------------------------------------------
  const WALL = /subscribe|newsletter|sign up|signup|create (a |an )?(free )?account|log in to continue|register to continue|continue reading|you've reached|articles? left|support (our|independent) journalism|become a member|unlock (this|unlimited)|premium|paywall|already a subscriber|get unlimited access|enable notifications|turn on notifications|download (the|our) app|open in (the )?app|install (the|our) app/i;
  const KEEP = /cookie|consent|gdpr|privacy|checkout|payment|cart|sign in|log in$|login|password|captcha|verify/i;
  let overlaysRemoved = 0;
  function removeOverlays() {
    // FIX: "body *" visits every element in the DOM, calling getComputedStyle() and
    // getBoundingClientRect() on each — hundreds to thousands of forced layout reflows.
    // Overlays are overwhelmingly: (a) direct children of body, (b) elements whose
    // class or id contains modal/overlay/wall/popup hints, (c) dialog elements.
    // This targeted selector reduces the pool by 95%+ on typical pages.
    const pool = document.querySelectorAll(
      "body > *, " +
      "[class*='overlay' i], [class*='modal' i], [class*='popup' i], [class*='wall' i], " +
      "[class*='paywall' i], [class*='gate' i], [class*='banner' i], [class*='subscribe' i], " +
      "[id*='overlay' i], [id*='modal' i], [id*='popup' i], [id*='wall' i], " +
      "dialog, [role='dialog'], [aria-modal='true']"
    );
    const candidates = Array.from(pool).filter((element) => {
      if (/^(NOAH-SHIELD|NS-[0-9A-F]{8})-(SHOP|BAR|NOTICE|TOAST)$/.test(element.tagName)) return false;
      const style = getComputedStyle(element);
      if (style.position !== "fixed" && style.position !== "sticky" && style.position !== "absolute") return false;
      const zIndex = parseInt(style.zIndex, 10);
      if (!(zIndex >= 100)) return false;
      const box = element.getBoundingClientRect();
      const covers = box.width >= innerWidth * 0.6 && box.height >= innerHeight * 0.5;
      const isBackdrop = covers && element.children.length <= 2 && (style.backgroundColor !== "rgba(0, 0, 0, 0)" || style.backdropFilter !== "none");
      const words = text(element).slice(0, 1500);
      const isWall = covers && WALL.test(words) && !KEEP.test(words) && words.length < 1500;
      return isBackdrop || isWall;
    });
    if (!candidates.length) return;
    for (const element of candidates) {
      element.style.setProperty("display", "none", "important");
      overlaysRemoved++;
    }
    // Sites lock scrolling under the wall; give it back.
    for (const element of [document.documentElement, document.body]) {
      if (!element) continue;
      const style = getComputedStyle(element);
      if (style.overflow === "hidden" || style.overflowY === "hidden") element.style.setProperty("overflow", "auto", "important");
      if (style.position === "fixed") element.style.setProperty("position", "static", "important");
    }
    for (const element of document.querySelectorAll("[style*='blur']")) {
      if (/blur\(/.test(element.style.filter) && element.getBoundingClientRect().height > innerHeight * 0.4) element.style.setProperty("filter", "none", "important");
    }
    send({ type: "safety.event", kind: "overlays", amount: candidates.length });
  }

  // ---- autoplay ---------------------------------------------------------------------------
  function stopAutoplay() {
    for (const media of document.querySelectorAll("video, audio")) {
      if (media.dataset.noahAutoplay) continue;
      media.dataset.noahAutoplay = "1";
      const userStarted = () => { media.dataset.noahAutoplay = "user"; };
      media.addEventListener("click", userStarted, true);
      const parent = media.closest("[class*='player' i], [id*='player' i]") || media.parentElement;
      if (parent) parent.addEventListener("click", userStarted, true);
      if (media.autoplay) { media.autoplay = false; media.removeAttribute("autoplay"); }
      const stop = () => {
        if (media.dataset.noahAutoplay === "user" || navigator.userActivation && navigator.userActivation.isActive) return;
        if (!media.paused) { media.pause(); send({ type: "safety.event", kind: "autoplay", amount: 1 }); }
      };
      media.addEventListener("play", () => setTimeout(stop, 50));
      media.addEventListener("loadeddata", stop);
      stop();
    }
  }

  // ---- fake countdowns and urgency ---------------------------------------------------------
  const URGENCY = /(only|just) \d+ left( in stock)?|\d+ (other )?people (are )?(looking|viewing)|(hurry|ends|expires|offer ends?|sale ends?|deal ends?) (in|soon)|\d{1,2}:\d{2}(:\d{2})? (left|remaining)|limited time|selling fast|almost gone|last chance/i;
  function flagUrgency() {
    let key;
    try { key = "noah-shield-timer:" + location.pathname; } catch { return; }
    // FIX: original called visible() (layout reflow) on EVERY span/div/p/b/strong/em/li/td —
    // potentially thousands of elements. Text check is pure string work; visible() is a
    // forced layout read. Filter by text first, then check visibility only for matches.
    const elements = Array.from(document.querySelectorAll("span, div, p, b, strong, em, li, td"))
      .filter((element) => {
        if (element.children.length > 3) return false;
        const words = text(element);
        return words.length < 120 && URGENCY.test(words) && visible(element);
      })
      .slice(0, 8);
    if (!elements.length) return;
    let previous = null;
    try { previous = JSON.parse(sessionStorage.getItem(key) || "null"); } catch { previous = null; }
    const now = elements.map((element) => text(element));
    const timers = now.map((words) => (words.match(/(\d{1,2}):(\d{2})(?::(\d{2}))?/) || []).slice(1).filter((part) => part !== undefined).map(Number));
    let reset = false;
    if (previous && previous.timers) {
      // A real countdown is lower than it was; a fake one starts over on reload.
      for (let index = 0; index < Math.min(timers.length, previous.timers.length); index++) {
        const before = previous.timers[index];
        const after = timers[index];
        if (before.length && after.length) {
          const seconds = (parts) => parts.reduce((sum, value) => sum * 60 + value, 0);
          if (seconds(after) >= seconds(before) - 1 && Date.now() - previous.at > 2500) reset = true;
        }
      }
    }
    // The first sighting on each page load is what the next load compares against.
    const origin = Math.round(performance.timeOrigin);
    if (!previous || previous.origin !== origin) {
      try { sessionStorage.setItem(key, JSON.stringify({ at: Date.now(), timers, origin })); } catch { /* fine */ }
    }
    elements.forEach((element, index) => {
      if (element.dataset.noahUrgency) return;
      element.dataset.noahUrgency = "1";
      element.style.setProperty("opacity", "0.55", "important");
      element.title = reset && timers[index] && timers[index].length ? "noah shield: this countdown restarted on reload; it is theatre" : "noah shield: urgency wording, take your time";
      if (reset && timers[index] && timers[index].length) element.style.setProperty("text-decoration", "line-through", "important");
    });
    send({ type: "safety.event", kind: "timers", amount: 1 });
  }

  // ---- copy and right-click ------------------------------------------------------------------
  function unblockCopy() {
    const style = document.createElement("style");
    style.textContent = "* { user-select: text !important; -webkit-user-select: text !important; } ::selection { background: rgba(95,138,88,.4) !important; }";
    (document.head || document.documentElement).append(style);
    for (const type of ["contextmenu", "copy", "cut", "paste", "selectstart", "dragstart", "mousedown", "mouseup", "keydown"]) {
      window.addEventListener(type, (event) => {
        if (type === "keydown" && !(event.ctrlKey || event.metaKey)) return;
        if (type === "mousedown" || type === "mouseup") { if (event.button !== 2) return; }
        event.stopImmediatePropagation();
      }, true);
    }
    for (const attribute of ["oncontextmenu", "oncopy", "oncut", "onpaste", "onselectstart", "ondragstart"]) {
      document.querySelectorAll("[" + attribute + "]").forEach((element) => element.removeAttribute(attribute));
      if (document.body) document.body[attribute] = null;
      document[attribute] = null;
    }
  }

  // ---- dark mode for sites without one ---------------------------------------------------------
  function darkMode() {
    const bodyColor = getComputedStyle(document.body || document.documentElement).backgroundColor;
    const match = /rgba?\((\d+), (\d+), (\d+)/.exec(bodyColor);
    const luminance = match ? (0.2126 * match[1] + 0.7152 * match[2] + 0.0722 * match[3]) / 255 : 1;
    const transparent = /rgba\(0, 0, 0, 0\)/.test(bodyColor);
    if (!transparent && luminance < 0.45) return;
    const style = document.createElement("style");
    style.id = "noah-shield-dark";
    style.textContent = "html { filter: invert(0.92) hue-rotate(180deg) !important; background: #0e0f0e !important; } img, video, picture, canvas, iframe, svg image, [style*='background-image'], .noah-keep { filter: invert(1) hue-rotate(180deg) !important; }";
    (document.head || document.documentElement).append(style);
  }

  waitForConfig().then((config) => {
    if (!config || config.trusted) return;
    const annoyances = config.annoyances || {};
    const run = () => {
      if (annoyances.banners !== false) rejectBanner();
      if (annoyances.overlays !== false && window.top === window) removeOverlays();
      if (annoyances.autoplay !== false) stopAutoplay();
      if (annoyances.timers !== false && window.top === window) flagUrgency();
    };
    if (annoyances.copy) unblockCopy();
    if (annoyances.dark) {
      if (document.body) darkMode();
      else document.addEventListener("DOMContentLoaded", darkMode, { once: true });
    }
    const start = () => { run(); setTimeout(run, 1500); setTimeout(run, 4000); };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, { once: true });
    else start();
    let pending = null;
    new MutationObserver(() => {
      if (pending) return;
      pending = setTimeout(() => { pending = null; run(); }, 800);
    }).observe(document.documentElement, { childList: true, subtree: true });
  });
})();
