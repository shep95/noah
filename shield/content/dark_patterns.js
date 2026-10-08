// Dark pattern detection and neutralization for noah shield.
//
// Pattern (from shepherd.intel): dark patterns are deception mechanisms.
// The mechanism: each one manipulates a specific cognitive vulnerability —
// pre-ticks exploit defaults, fake countdowns exploit urgency, confirm-shaming
// exploits shame, roach motels exploit friction asymmetry, fake anchoring
// exploits price relativity.
//
// We operate at the mechanism level, not the surface: we do not try to
// detect "bad buttons" by color or position. We detect the pattern by its
// behavioral signature and neutralize the specific exploit it uses.
//
// Runs in the ISOLATED world.
(() => {
  "use strict";
  if (window.__noahShieldDarkPatterns) return;
  window.__noahShieldDarkPatterns = true;

  const api = globalThis.chrome ?? globalThis.browser;
  const send = (msg) => {
    try { api.runtime.sendMessage(msg, () => { void api.runtime.lastError; }); }
    catch { }
  };

  let config = { darkPatterns: true };
  try {
    api.runtime.sendMessage({ type: "dark.config" }, (r) => {
      void api.runtime.lastError;
      if (r && r.config) config = { ...config, ...r.config };
    });
  } catch { }

  // ── styles ────────────────────────────────────────────────────────────────
  let _styled = false;
  function injectStyles() {
    if (_styled) return;
    _styled = true;
    const style = document.createElement("style");
    style.id = "noah-dp-styles";
    style.textContent = `
      .noah-dp-warning {
        display: inline-flex !important;
        align-items: center;
        gap: 5px;
        font: 11px/1.4 -apple-system, system-ui, sans-serif !important;
        color: #e8b4a8 !important;
        background: rgba(7,4,4,0.92) !important;
        border: 1px solid rgba(232,180,168,0.28) !important;
        border-radius: 5px !important;
        padding: 3px 8px !important;
        margin: 4px 0 !important;
        pointer-events: none;
        white-space: nowrap;
        z-index: 9999999;
        position: relative;
      }
      .noah-dp-warning::before { content: "◈ "; }
      .noah-dp-countdown-warn {
        color: #dcc896 !important;
        border-color: rgba(220,200,150,0.28) !important;
      }
      .noah-dp-uncheck-notice {
        color: #a9cf9f !important;
        border-color: rgba(169,207,159,0.2) !important;
      }
    `;
    (document.head || document.documentElement).appendChild(style);
  }

  function warn(element, text, cls) {
    if (!element || element.dataset.noahDpWarned) return;
    element.dataset.noahDpWarned = "1";
    injectStyles();
    const badge = document.createElement("span");
    badge.className = `noah-dp-warning${cls ? " " + cls : ""}`;
    badge.textContent = text;
    element.insertAdjacentElement("afterend", badge);
    send({ type: "safety.event", kind: "dark_pattern", pattern: text });
  }

  // ── 1. Pre-ticked opt-in checkboxes ─────────────────────────────────────
  // Mechanism: a checkbox is checked by default, so the user must take
  // affirmative action to opt OUT rather than opt IN. The exploit is the
  // default effect (status quo bias) — most users don't change defaults.
  // Neutralization: uncheck the box and label it so the user knows.
  const OPT_IN_PATTERNS = [
    /newsletter/i, /marketing/i, /promotional/i, /offers/i,
    /updates/i, /partner/i, /third.party/i, /advertising/i,
    /news.?letter/i, /subscribe/i, /notify/i, /keep me/i,
    /opt.?in/i, /agree to receive/i, /send me/i, /contact me/i,
  ];

  function scanPreTicked() {
    if (!config.darkPatterns) return;
    const checkboxes = document.querySelectorAll("input[type='checkbox']:not([data-noah-dp-scanned])");
    for (const cb of checkboxes) {
      cb.dataset.noahDpScanned = "1";
      if (!cb.checked) continue;
      // Find the associated label text
      const labelEl = cb.labels && cb.labels[0]
        ? cb.labels[0]
        : cb.closest("label") || document.querySelector(`label[for="${cb.id}"]`);
      const labelText = labelEl ? labelEl.textContent || "" : cb.getAttribute("aria-label") || "";
      if (!labelText) continue;
      if (OPT_IN_PATTERNS.some((p) => p.test(labelText))) {
        cb.checked = false;
        warn(labelEl || cb, "pre-ticked opt-in unchecked by noah shield", "noah-dp-uncheck-notice");
      }
    }
  }

  // ── 2. Fake countdown timers ──────────────────────────────────────────────
  // Mechanism: a ticking countdown ("offer expires in 09:47") manufactures
  // urgency. Most of these reset when the page is refreshed — revealing they
  // are fake urgency triggers, not real expiry deadlines.
  // Neutralization: detect countdown elements and label them as false urgency.
  // We do NOT remove them — pausing or freezing would break legitimate timers
  // (e.g. booking session timeouts). We label so the user can evaluate.
  const COUNTDOWN_SELECTORS = [
    "[class*='countdown']", "[class*='count-down']", "[class*='timer']",
    "[id*='countdown']", "[id*='count-down']", "[id*='timer']",
    "[data-countdown]", "[data-timer]",
  ];
  // If a countdown container contains multiple ticking digits (hours, mins, secs)
  // it's a sales urgency timer, not a functional one.
  function looksLikeSalesCountdown(el) {
    const text = el.textContent || "";
    // Looks like a countdown if it has 2-digit groups separated by : or letters
    const hasDigits = /\d{1,2}\s*[:\s]\s*\d{1,2}/.test(text);
    if (!hasDigits) return false;
    // Check for urgency language nearby
    const urgencyRoot = el.closest("[class*='banner'], [class*='promo'], [class*='sale'], [class*='offer'], [class*='deal'], header, aside") || el.parentElement;
    const urgencyText = (urgencyRoot && urgencyRoot.textContent) || "";
    return /sale|deal|off|discount|limited|hurry|ends|today|only|left/i.test(urgencyText);
  }

  function scanCountdowns() {
    if (!config.darkPatterns) return;
    for (const sel of COUNTDOWN_SELECTORS) {
      const elements = document.querySelectorAll(`${sel}:not([data-noah-dp-countdown])`);
      for (const el of elements) {
        el.dataset.noahDpCountdown = "1";
        if (looksLikeSalesCountdown(el)) {
          warn(el, "false urgency countdown — likely resets on refresh", "noah-dp-countdown-warn");
        }
      }
    }
  }

  // ── 3. Confirm-shaming language ───────────────────────────────────────────
  // Mechanism: the "decline" option is phrased to make the user feel stupid
  // or bad for declining ("No thanks, I hate saving money"). The exploit is
  // shame and identity threat — it links refusal to a negative self-image.
  // Neutralization: detect and label the shaming text.
  const SHAME_PATTERNS = [
    /no.{0,15}(hate|don.?t|don.?t want|prefer not|i.?d rather)/i,
    /no thanks,.{0,30}(money|savings|deals|free|discount)/i,
    /i (hate|don.?t like|prefer not to) (save|get|have|receive|learn)/i,
    /i.?d rather (pay|miss|not|stay)/i,
    /not interested in (saving|deals|offers|learning|improving)/i,
    /no.{0,8}i.?m (fine|okay|good) (paying|without|spending)/i,
    /keep (paying|spending) (full|more)/i,
  ];

  function scanConfirmShaming() {
    if (!config.darkPatterns) return;
    const candidates = document.querySelectorAll(
      "a:not([data-noah-dp-shame]), button:not([data-noah-dp-shame]), " +
      "label:not([data-noah-dp-shame]), span:not([data-noah-dp-shame]), " +
      "p:not([data-noah-dp-shame])"
    );
    for (const el of candidates) {
      // Only scan elements with limited text (shame text is always short)
      const text = (el.textContent || "").trim();
      if (!text || text.length > 120) continue;
      el.dataset.noahDpShame = "1";
      if (SHAME_PATTERNS.some((p) => p.test(text))) {
        warn(el, "confirm-shaming language detected", "");
      }
    }
  }

  // ── 4. Roach motel unsubscribe patterns ──────────────────────────────────
  // Mechanism: signing up is easy (one click) but cancelling is hidden behind
  // many steps, calls, or obfuscated links. The friction asymmetry exploits
  // loss aversion and effort justification.
  // Neutralization: surface the unsubscribe/cancel link if it's buried.
  const ROACH_MOTEL_SELECTORS = [
    "a[href*='unsubscribe']", "a[href*='cancel']",
    "a[href*='opt-out']", "a[href*='optout']",
    "a[href*='manage-preferences']",
  ];
  function scanRoachMotel() {
    if (!config.darkPatterns) return;
    for (const sel of ROACH_MOTEL_SELECTORS) {
      const links = document.querySelectorAll(`${sel}:not([data-noah-dp-roach])`);
      for (const link of links) {
        link.dataset.noahDpRoach = "1";
        const style = window.getComputedStyle(link);
        // Roach motel sign: unsubscribe link is nearly invisible (very small font,
        // near-background color, or hidden in a footer with opacity < 0.3)
        const fontSize = parseFloat(style.fontSize) || 14;
        const opacity = parseFloat(style.opacity) || 1;
        if (fontSize < 10 || opacity < 0.3) {
          link.style.setProperty("font-size", "12px", "important");
          link.style.setProperty("opacity", "1", "important");
          link.style.setProperty("color", "#a9cf9f", "important");
          link.style.setProperty("text-decoration", "underline", "important");
          warn(link, "buried cancel link surfaced", "noah-dp-uncheck-notice");
        }
      }
    }
  }

  // ── 5. Fake price anchoring strikethroughs ────────────────────────────────
  // Mechanism: a "was $199, now $99!" display uses a fake original price
  // (the item was never sold at $199) to make the current price look like
  // a bargain. The exploit is reference point manipulation.
  // Neutralization: flag struck-through price pairs where the discount
  // seems implausibly large (> 60% off) or where the "original" shows
  // no evidence of being a real historical price.
  function scanFakePricing() {
    if (!config.darkPatterns) return;
    const stricken = document.querySelectorAll(
      "s:not([data-noah-dp-price]), del:not([data-noah-dp-price]), " +
      "[class*='original-price']:not([data-noah-dp-price]), " +
      "[class*='was-price']:not([data-noah-dp-price]), " +
      "[class*='list-price']:not([data-noah-dp-price]), " +
      "[class*='compare-at']:not([data-noah-dp-price])"
    );
    for (const el of stricken) {
      el.dataset.noahDpPrice = "1";
      const originalText = el.textContent || "";
      const originalMatch = originalText.match(/[\d,.]+/);
      if (!originalMatch) continue;
      const originalPrice = parseFloat(originalMatch[0].replace(/,/g, ""));
      if (!originalPrice) continue;
      // Look for a nearby "current" price in the same container
      const container = el.closest("[class*='price'], [class*='pricing'], li, .product, [class*='card']") || el.parentElement;
      if (!container) continue;
      const priceEls = container.querySelectorAll(
        "[class*='sale'], [class*='current'], [class*='now'], [class*='discounted'], " +
        "[class*='our-price'], [class*='offer-price']"
      );
      for (const priceEl of priceEls) {
        const currentText = priceEl.textContent || "";
        const currentMatch = currentText.match(/[\d,.]+/);
        if (!currentMatch) continue;
        const currentPrice = parseFloat(currentMatch[0].replace(/,/g, ""));
        if (!currentPrice || currentPrice >= originalPrice) continue;
        const discount = 1 - (currentPrice / originalPrice);
        if (discount > 0.6) {
          warn(el, `${Math.round(discount * 100)}% off claim — verify original price`, "noah-dp-countdown-warn");
          break;
        }
      }
    }
  }

  // ── 6. Hidden subscription triggers ─────────────────────────────────────
  // Some "free trial" buttons auto-enroll in a paid subscription after the
  // trial. The mechanism: the billing terms are disclosed in 6pt gray text
  // below the button. Detect and surface these.
  function scanHiddenSubscriptions() {
    if (!config.darkPatterns) return;
    const freeTrialButtons = document.querySelectorAll(
      "button:not([data-noah-dp-sub]), a:not([data-noah-dp-sub]), input[type='submit']:not([data-noah-dp-sub])"
    );
    for (const btn of freeTrialButtons) {
      const text = (btn.textContent || btn.value || "").trim();
      if (!/free trial|start free|try free|get free/i.test(text)) continue;
      btn.dataset.noahDpSub = "1";
      // Look for billing disclosure in fine print near the button
      const container = btn.closest("section, .cta, [class*='pricing'], [class*='plan'], form") || btn.parentElement;
      if (!container) continue;
      const finePrint = container.querySelectorAll("p, span, small, .fine-print, [class*='disclaimer'], [class*='terms']");
      for (const fp of finePrint) {
        const fpText = fp.textContent || "";
        if (/(?:after|then|billed|charged|per month|per year|\$\d|credit card)/i.test(fpText) && fpText.length < 300) {
          const fpStyle = window.getComputedStyle(fp);
          const fpSize = parseFloat(fpStyle.fontSize) || 14;
          if (fpSize < 12) {
            fp.style.setProperty("font-size", "12px", "important");
            fp.style.setProperty("opacity", "1", "important");
            warn(btn, "auto-billing after trial — read terms below", "noah-dp-countdown-warn");
            break;
          }
        }
      }
    }
  }

  // ── scan orchestration ────────────────────────────────────────────────────
  let _scanQueued = false;
  function queueScan() {
    if (_scanQueued || !config.darkPatterns) return;
    _scanQueued = true;
    requestAnimationFrame(() => {
      _scanQueued = false;
      scanPreTicked();
      scanCountdowns();
      scanConfirmShaming();
      scanRoachMotel();
      scanFakePricing();
      scanHiddenSubscriptions();
    });
  }

  const mo = new MutationObserver(queueScan);
  mo.observe(document.documentElement, { childList: true, subtree: true });

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", queueScan);
  } else {
    queueScan();
  }

  api.runtime.onMessage.addListener((msg) => {
    if (msg && msg.type === "dark.set" && msg.config) {
      config = { ...config, ...msg.config };
      if (!config.darkPatterns) mo.disconnect();
      else queueScan();
    }
  });
})();
