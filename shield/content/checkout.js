// Checkout and confirmation pages: hidden fees added up, pre-ticked add-ons
// pointed out, subscription traps named, a 24-hour pause offered, and the
// receipt saved once the order is placed.
(() => {
  "use strict";
  if (window.top !== window) return;
  const api = globalThis.chrome ?? globalThis.browser;
  const send = (message) => new Promise((resolve) => {
    try {
      api.runtime.sendMessage(message, (response) => { void api.runtime.lastError; resolve(response || {}); });
    } catch {
      resolve({});
    }
  });

  function waitForConfig() {
    return new Promise((resolve) => {
      if (window.__noahShieldSiteConfig) return resolve(window.__noahShieldSiteConfig);
      document.addEventListener("noah-shield:site-config", () => resolve(window.__noahShieldSiteConfig), { once: true });
      setTimeout(() => resolve(window.__noahShieldSiteConfig || null), 4000);
    });
  }
  const text = (node) => (node && (node.innerText || node.textContent) || "").replace(/\s+/g, " ").trim();
  function parsePrice(value) {
    const cleaned = String(value).replace(/[^\d.,]/g, "");
    if (!cleaned) return null;
    const normalized = /,\d{2}$/.test(cleaned) && !/\.\d{2}$/.test(cleaned) ? cleaned.replace(/\./g, "").replace(",", ".") : cleaned.replace(/,/g, "");
    const number = parseFloat(normalized);
    return Number.isFinite(number) && number > 0 ? number : null;
  }
  const PRICE = /(?:\$|€|£|¥|C\$|A\$)\s?[\d.,]+|[\d.,]+\s?(?:€|£|\$)/;
  const money = (amount) => {
    try { return new Intl.NumberFormat(undefined, { style: "currency", currency: "USD" }).format(amount).replace(/^\$/, "$"); } catch { return amount.toFixed(2); }
  };

  // The shopping card is shared with content/shop.js through an event: this
  // script only asks for lines to be shown.
  function show(kind, lines, actions = []) {
    document.dispatchEvent(new CustomEvent("noah-shield:checkout-note", { detail: JSON.stringify({ kind, lines, actions }) }));
  }

  // ---- hidden fees ----------------------------------------------------------------
  const FEE = /\b(service|resort|convenience|processing|handling|booking|facility|cleaning|platform|delivery|fuel|regulatory|environmental|tip|gratuity|surcharge|fee|fees)\b/i;
  const NOT_FEE = /subtotal|^total|tax|shipping|discount|coupon|promo|item|qty|quantity/i;
  function hiddenFees() {
    const rows = Array.from(document.querySelectorAll("tr, li, div, dt, dd, p, span"))
      .filter((element) => element.children.length <= 6)
      .map((element) => ({ element, words: text(element) }))
      .filter(({ words }) => words.length < 90 && FEE.test(words) && !NOT_FEE.test(words) && PRICE.test(words));
    const seen = new Map();
    for (const { element, words } of rows) {
      if (Array.from(seen.keys()).some((other) => other.contains(element) || element.contains(other))) continue;
      const amount = parsePrice((words.match(PRICE) || [""])[0]);
      if (amount) seen.set(element, { label: words.replace(PRICE, "").trim().slice(0, 50), amount });
    }
    const fees = Array.from(seen.values());
    if (!fees.length) return;
    const total = fees.reduce((sum, fee) => sum + fee.amount, 0);
    show("fees", [`Fees on top: ${money(total)} (${fees.map((fee) => `${fee.label} ${money(fee.amount)}`).join(", ")})`]);
    send({ type: "safety.event", kind: "fees", amount: 1 });
  }

  // ---- pre-ticked add-ons -----------------------------------------------------------
  const ADDON = /newsletter|offers|marketing|insurance|protection|warranty|coverage|donat|round ?up|premium|priority|express|subscribe|membership|trial|extended|add\b|upgrade|tip/i;
  function preTicked() {
    const boxes = Array.from(document.querySelectorAll('input[type="checkbox"]:checked'))
      .filter((box) => !box.disabled)
      .map((box) => {
        const label = box.labels && box.labels[0] ? text(box.labels[0]) : text(box.closest("label, li, div, tr"));
        return { box, label: label.slice(0, 90) };
      })
      .filter(({ label }) => ADDON.test(label) && !/terms|privacy|agree|accept|remember me|save (my|this) (card|address)|same as|billing address/i.test(label));
    if (!boxes.length) return;
    show("addons", [`Pre-ticked for you: ${boxes.map((entry) => entry.label).join(" · ")}`], [{ label: "Untick them", action: "untick" }]);
    document.addEventListener("noah-shield:checkout-action", (event) => {
      if (event.detail !== "untick") return;
      for (const { box } of boxes) {
        if (box.checked) box.click();
      }
    }, { once: true });
    send({ type: "safety.event", kind: "darkPatterns", amount: 1 });
  }

  // ---- subscription traps -----------------------------------------------------------
  const TRAP = /(free trial|trial (period|ends)|after (your|the) (free )?trial|will (then )?be (automatically )?(charged|billed)|auto-?renew|automatically renew|renews (automatically|every)|recurring (charge|payment|billing)|billed (monthly|annually|yearly|every))/i;
  function subscriptionTrap() {
    const body = (document.body && document.body.innerText || "").slice(0, 120000);
    const match = TRAP.exec(body);
    if (!match) return;
    const around = body.slice(Math.max(0, match.index - 120), match.index + 160).replace(/\s+/g, " ").trim();
    const daysMatch = /(\d{1,3})[- ]day/i.exec(around) || /(\d{1,3})[- ]day/i.exec(body);
    const days = daysMatch ? parseInt(daysMatch[1], 10) : null;
    const lines = [`Subscription language on this page: “…${around}…”`];
    const actions = [];
    if (days) {
      lines.push(`It mentions ${days} days. A reminder two days before would fall on ${new Date(Date.now() + Math.max(1, days - 2) * 86400000).toLocaleDateString()}.`);
      actions.push({ label: `Remind me in ${Math.max(1, days - 2)} days`, action: "remind:" + Math.max(1, days - 2) });
    } else {
      actions.push({ label: "Remind me in 25 days", action: "remind:25" });
    }
    show("trap", lines, actions);
    document.addEventListener("noah-shield:checkout-action", (event) => {
      const parts = String(event.detail).split(":");
      if (parts[0] !== "remind") return;
      send({ type: "shop.remind", days: Number(parts[1]), label: "Cancel before it charges: " + location.hostname, url: location.href, kind: "trial" });
    });
    send({ type: "safety.event", kind: "traps", amount: 1 });
  }

  // ---- wait 24 hours ------------------------------------------------------------------
  function waitOffer() {
    show("wait", ["Not sure? Sleep on it. The shield can bring you back tomorrow."], [{ label: "Wait 24 hours", action: "wait" }]);
    document.addEventListener("noah-shield:checkout-action", (event) => {
      if (event.detail !== "wait") return;
      send({ type: "shop.remind", days: 1, label: "Still want it? " + document.title.slice(0, 80), url: location.href, kind: "wait" });
    });
  }

  // ---- receipts ---------------------------------------------------------------------------
  const CONFIRMATION_URL = /thank|confirm|order-?(placed|complete|received|success|summary)|receipt|purchase-?(complete|success)/i;
  const CONFIRMATION_TEXT = /thank you for (your )?(order|purchase)|order (has been )?(placed|confirmed|received)|your order (number|#|id)|order confirmation/i;
  function receipt() {
    const body = (document.body && document.body.innerText || "").slice(0, 80000);
    if (!(CONFIRMATION_URL.test(location.pathname + location.search) || CONFIRMATION_TEXT.test(document.title)) || !CONFIRMATION_TEXT.test(body)) return;
    const order = /order\s*(?:number|#|no\.?|id|confirmation)\s*(?:is|:|#)?\s*([A-Z0-9][A-Z0-9-]{4,30})/i.exec(body);
    const total = /(?:order |grand |amount )?total[^\d$€£¥]{0,40}((?:\$|€|£|¥)\s?[\d.,]+|[\d.,]+\s?(?:€|£|\$))/i.exec(body);
    send({
      type: "shop.receipt",
      receipt: {
        site: location.hostname.replace(/^www\./, ""),
        url: location.href.split("#")[0],
        title: document.title,
        orderNumber: order ? order[1] : "",
        total: total ? parsePrice(total[1]) : null,
        currency: /€/.test(body) ? "EUR" : /£/.test(body) ? "GBP" : "USD",
        items: body.slice(0, 400),
      },
    }).then((result) => {
      if (result && result.saved) show("receipt", ["Receipt saved in the shield (settings → purchases). Add a warranty length there if the item has one."]);
    });
  }

  function looksLikeCheckout() {
    return /\/(checkout|cart|basket|bag|payment|order|purchase|review)\b/i.test(location.pathname) || Boolean(document.querySelector('input[name*="promo" i], input[id*="promo" i], input[name*="coupon" i], input[id*="coupon" i]'));
  }

  waitForConfig().then((config) => {
    if (!config || !config.shopping || config.shoppingQuiet || config.trusted) return;
    const run = () => {
      receipt();
      if (!looksLikeCheckout()) return;
      hiddenFees();
      preTicked();
      subscriptionTrap();
      waitOffer();
    };
    if (document.readyState === "complete") setTimeout(run, 1200);
    else window.addEventListener("load", () => setTimeout(run, 1200), { once: true });
  });
})();
