// Shopping, in the page: notices a product and asks the background for the
// same thing elsewhere; notices a checkout and tries the codes that might
// still work, one after another, without ever pressing the order button.
(() => {
  "use strict";

  // The shield's own elements carry the session's tag; in stealth mode it is a
  // name no page can look for.
  function tagName(kind) {
    const tag = (window.__noahShieldSiteConfig && window.__noahShieldSiteConfig.tag) || "noah-shield";
    return tag + "-" + kind;
  }
  if (window.top !== window) return;
  const api = globalThis.chrome ?? globalThis.browser;
  const send = (message) => new Promise((resolve) => {
    try {
      api.runtime.sendMessage(message, (response) => {
        void api.runtime.lastError;
        resolve(response || {});
      });
    } catch {
      resolve({});
    }
  });

  const ORDER_BUTTON = /place (your )?order|pay now|buy now|complete (purchase|order)|submit order|confirm (order|purchase|and pay)|checkout now|proceed to (payment|checkout)|continue to payment/i;
  const FIELD_HINT = /promo|coupon|discount|voucher|offer code|redeem|rabatt|gutschein|código|codice|réduction/i;
  const FIELD_AVOID = /zip|postal|cvv|cvc|security|phone|gift card number|card number|password|email|search/i;
  const REVEAL_HINT = /(have|got|enter|add|apply)\b.*\b(promo|coupon|discount|voucher)|\b(promo|coupon|discount|voucher)\s*code/i;
  const FAILURE_HINT = /invalid|expired|not valid|isn't valid|is not valid|doesn't exist|does not exist|cannot be applied|can't be applied|not applicable|unrecognized|not recognised|not recognized|doesn't apply|no longer|limit reached|not eligible/i;

  function waitForConfig() {
    return new Promise((resolve) => {
      if (window.__noahShieldSiteConfig) return resolve(window.__noahShieldSiteConfig);
      document.addEventListener("noah-shield:site-config", () => resolve(window.__noahShieldSiteConfig), { once: true });
      setTimeout(() => resolve(window.__noahShieldSiteConfig || null), 4000);
    });
  }

  function text(node) {
    return (node && (node.innerText || node.textContent) || "").replace(/\s+/g, " ").trim();
  }

  function parsePrice(value) {
    const cleaned = String(value).replace(/[^\d.,]/g, "");
    if (!cleaned) return null;
    const normalized = /,\d{2}$/.test(cleaned) && !/\.\d{2}$/.test(cleaned) ? cleaned.replace(/\./g, "").replace(",", ".") : cleaned.replace(/,/g, "");
    const number = parseFloat(normalized);
    return Number.isFinite(number) && number > 0 ? number : null;
  }

  function currencyOf(sample) {
    const host = location.hostname;
    if (/£/.test(sample)) return "GBP";
    if (/€/.test(sample)) return "EUR";
    if (/¥|JPY/.test(sample)) return "JPY";
    if (/C\$|CAD/.test(sample) || host.endsWith(".ca")) return "CAD";
    if (/A\$|AUD/.test(sample) || host.endsWith(".com.au")) return "AUD";
    if (/\$|USD/.test(sample)) return "USD";
    return null;
  }

  function fromJsonLd() {
    for (const script of document.querySelectorAll('script[type="application/ld+json"]')) {
      let data;
      try {
        data = JSON.parse(script.textContent);
      } catch {
        continue;
      }
      const nodes = Array.isArray(data) ? data : data && data["@graph"] ? data["@graph"] : [data];
      for (const node of nodes) {
        if (!node || !/Product/i.test(String(node["@type"]))) continue;
        let offer = node.offers;
        if (Array.isArray(offer)) offer = offer[0];
        if (offer && Array.isArray(offer.offers)) offer = offer.offers[0];
        const priceText = offer && (offer.price ?? offer.lowPrice ?? (offer.priceSpecification && offer.priceSpecification.price));
        const price = priceText !== undefined ? parsePrice(String(priceText)) : null;
        if (!node.name || !price) continue;
        return {
          title: String(node.name),
          brand: node.brand && (node.brand.name || node.brand) ? String(node.brand.name || node.brand) : "",
          price,
          currency: (offer && offer.priceCurrency) || currencyOf(String(priceText)) || "USD",
          code: node.gtin13 || node.gtin || node.gtin12 || node.mpn || "",
        };
      }
    }
    return null;
  }

  function fromMeta() {
    const amount = document.querySelector('meta[property="product:price:amount"], meta[property="og:price:amount"]');
    const type = document.querySelector('meta[property="og:type"]');
    if (!amount || !(type && /product/i.test(type.content))) return null;
    const title = document.querySelector('meta[property="og:title"]');
    const currency = document.querySelector('meta[property="product:price:currency"], meta[property="og:price:currency"]');
    const price = parsePrice(amount.content);
    if (!title || !price) return null;
    return { title: title.content, brand: "", price, currency: currency ? currency.content : currencyOf(amount.content) || "USD", code: "" };
  }

  function fromAmazon() {
    if (!/amazon\./.test(location.hostname)) return null;
    const title = document.querySelector("#productTitle");
    const priceNode = document.querySelector("#corePrice_feature_div .a-offscreen, #corePriceDisplay_desktop_feature_div .a-offscreen, #apex_desktop .a-offscreen, #price_inside_buybox, #priceblock_ourprice, .a-price .a-offscreen");
    if (!title || !priceNode) return null;
    const price = parsePrice(text(priceNode) || priceNode.textContent);
    if (!price) return null;
    const byline = text(document.querySelector("#bylineInfo")).replace(/^(visit the |brand: )/i, "").replace(/ store$/i, "");
    return { title: text(title), brand: byline.length < 40 ? byline : "", price, currency: currencyOf(priceNode.textContent) || "USD", code: (location.pathname.match(/\/dp\/([A-Z0-9]{10})/) || [])[1] || "" };
  }

  function fromMicrodata() {
    const price = document.querySelector('[itemprop="price"]');
    const name = document.querySelector('[itemprop="name"]');
    if (!price || !name) return null;
    const value = parsePrice(price.getAttribute("content") || text(price));
    if (!value) return null;
    const currency = document.querySelector('[itemprop="priceCurrency"]');
    return { title: text(name), brand: "", price: value, currency: (currency && (currency.getAttribute("content") || text(currency))) || currencyOf(text(price)) || "USD", code: "" };
  }

  function findProduct() {
    const product = fromJsonLd() || fromAmazon() || fromMeta() || fromMicrodata();
    if (!product || product.title.length < 4) return null;
    return { ...product, url: location.href };
  }

  // ---- the card ---------------------------------------------------------

  let host = null;
  let cardBody = null;
  function card() {
    if (host && host.isConnected) return cardBody;
    host = document.createElement(tagName("shop"));
    const shadow = host.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = `
      :host { all: initial; position: fixed; left: 18px; bottom: 18px; z-index: 2147483646; font: 13px/1.45 -apple-system, "Segoe UI", system-ui, sans-serif; }
      .card { width: 340px; max-width: calc(100vw - 36px); background: #0f0f0f; color: #e6e6e6; border: 1px solid #262626; border-radius: 12px; box-shadow: 0 12px 40px rgba(0,0,0,.45); overflow: hidden; }
      .head { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid #1f1f1f; color: #9a9a9a; font-size: 12px; }
      .head b { color: #fff; font-weight: 600; }
      .head .grow { flex: 1; }
      .head button { all: unset; cursor: pointer; color: #8a8a8a; padding: 2px 6px; border-radius: 6px; }
      .head button:hover { color: #fff; background: #1d1d1d; }
      .body { padding: 10px 12px; display: grid; gap: 8px; }
      .row { display: flex; gap: 10px; align-items: baseline; }
      .row .grow { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .price { font-variant-numeric: tabular-nums; color: #fff; }
      .save { color: #8fd18f; font-size: 12px; }
      a { color: #dcdcdc; text-decoration: none; }
      a:hover { text-decoration: underline; }
      .muted { color: #8a8a8a; font-size: 12px; }
      .links { display: flex; flex-wrap: wrap; gap: 6px 10px; }
      .links a { font-size: 12px; color: #a8a8a8; }
      .code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; background: #1a1a1a; padding: 1px 6px; border-radius: 5px; color: #fff; }
      .actions { display: flex; gap: 8px; }
      button.act { font: inherit; padding: 6px 10px; border-radius: 8px; border: 1px solid #333; background: #181818; color: #eee; cursor: pointer; }
      button.act:hover { background: #222; }
      .bar { height: 3px; background: #1c1c1c; border-radius: 2px; overflow: hidden; }
      .bar i { display: block; height: 100%; background: #cfcfcf; width: 0; transition: width .3s; }

      @keyframes ns-arrive { from { opacity: 0; } to { opacity: 1; } }
      :host { animation: ns-arrive .22s ease-out; }
      @media (prefers-reduced-motion: reduce) { :host, * { animation: none !important; transition: none !important; } }
    `;
    const box = document.createElement("div");
    box.className = "card";
    const head = document.createElement("div");
    head.className = "head";
    const name = document.createElement("b");
    name.textContent = "noah shield";
    const grow = document.createElement("span");
    grow.className = "grow";
    const quiet = document.createElement("button");
    quiet.textContent = "quiet here";
    quiet.title = "Never show this on " + location.hostname;
    quiet.addEventListener("click", async () => {
      await send({ type: "shop.quiet" });
      host.remove();
    });
    const close = document.createElement("button");
    close.textContent = "×";
    close.title = "Close";
    close.addEventListener("click", () => host.remove());
    head.append(name, grow, quiet, close);
    cardBody = document.createElement("div");
    cardBody.className = "body";
    box.append(head, cardBody);
    shadow.append(style, box);
    document.documentElement.append(host);
    return cardBody;
  }

  // Lines other scripts (content/checkout.js) want on the card.
  document.addEventListener("noah-shield:checkout-note", (event) => {
    let detail;
    try { detail = JSON.parse(String(event.detail || "{}")); } catch { return; }
    const body = card();
    for (const entry of detail.lines || []) body.append(line("muted", span("", String(entry))));
    if (detail.actions && detail.actions.length) {
      const row = line("actions");
      for (const action of detail.actions) {
        const button = document.createElement("button");
        button.className = "act";
        button.textContent = String(action.label);
        button.addEventListener("click", () => document.dispatchEvent(new CustomEvent("noah-shield:checkout-action", { detail: String(action.action) })));
        row.append(button);
      }
      body.append(row);
    }
  });

  function line(className, ...children) {
    const element = document.createElement("div");
    element.className = className;
    element.append(...children);
    return element;
  }
  function span(className, content) {
    const element = document.createElement("span");
    element.className = className;
    element.textContent = content;
    return element;
  }
  function link(url, label, className = "") {
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.target = "_blank";
    anchor.rel = "noopener noreferrer";
    anchor.textContent = label;
    if (className) anchor.className = className;
    return anchor;
  }
  function money(amount, currency) {
    try {
      return new Intl.NumberFormat(undefined, { style: "currency", currency }).format(amount);
    } catch {
      return amount.toFixed(2) + " " + currency;
    }
  }

  // ---- comparing --------------------------------------------------------

  async function compare(product, config) {
    const body = card();
    body.replaceChildren(line("muted", span("", "Looking for " + product.title.slice(0, 60) + (product.title.length > 60 ? "…" : "") + " at other stores…")));
    const result = await send({ type: "shop.compare", product });
    if (result.error) {
      body.replaceChildren(line("muted", span("", "Could not compare: " + result.error)));
      return;
    }
    send({ type: "shop.compared" });
    body.replaceChildren();
    if (result.cheaper.length) {
      const best = result.cheaper[0];
      body.append(line("row", span("grow", "Cheaper: " + best.store + (best.shippingKnown ? " (with shipping)" : "")), span("price", money(best.total, result.currency)), span("save", "save " + money(product.price - best.total, result.currency))));
      for (const offer of result.cheaper.slice(0, 4)) {
        body.append(line("row", link(offer.url, offer.title.slice(0, 70), "grow"), span("price", money(offer.total, result.currency) + (offer.shippingKnown && offer.shipping ? " incl. " + money(offer.shipping, result.currency) + " ship" : ""))));
      }
    } else if (result.offers.length) {
      body.append(line("muted", span("", "Nothing cheaper found; closest matches:")));
      for (const offer of result.offers.slice(0, 3)) {
        body.append(line("row", link(offer.url, offer.store + " · " + offer.title.slice(0, 60), "grow"), span("price", money(offer.price, result.currency))));
      }
    } else {
      body.append(line("muted", span("", "No readable match at the stores the shield can read. Check the rest by hand:")));
    }
    const links = line("links");
    for (const entry of result.searched) links.append(link(entry.searchUrl, entry.store + (entry.readable ? "" : " (could not read)")));
    for (const entry of result.links) links.append(link(entry.searchUrl, entry.store));
    body.append(links);
    if (result.offers.some((offer) => offer.shippingKnown)) body.append(line("muted", span("", "Prices with shipping where the store shows it (eBay); others exclude it.")));
    if (config.clipCoupons) clipAmazonCoupon();
    extras(product, result, body);
  }

  // ---- history, watching, the store, the reviews, the proof ----------------------------

  async function extras(product, result, body) {
    const history = await send({ type: "shop.price", product });
    if (history && history.count) {
      const summary = history.count === 1
        ? "First time the shield sees this product; it will remember the price."
        : `Seen ${history.count} days since ${history.since}: low ${money(history.low, result.currency)}, high ${money(history.high, result.currency)}, usually ${money(history.median, result.currency)}.`;
      body.append(line("muted", span("", summary)));
      if (history.waitHint) body.append(line("save", span("", "Above its usual price: waiting has paid off before.")));
      const was = wasPrice();
      if (was && history.count >= 3 && was > history.high * 1.02) body.append(line("muted", span("", `The "was" price ${money(was, result.currency)} never appeared in the ${history.count} days the shield watched. The sale is against a made-up number.`)));
    }
    const actions = line("actions");
    const watch = document.createElement("button");
    watch.className = "act";
    watch.textContent = history && history.watching ? "Watching (stop)" : "Watch for a drop";
    watch.addEventListener("click", async () => {
      const answer = await send({ type: history && history.watching ? "shop.unwatch" : "shop.watch", product, url: product.url });
      if (history) history.watching = Boolean(answer.watching);
      watch.textContent = answer.watching ? "Watching (stop)" : "Watch for a drop";
    });
    const proof = document.createElement("button");
    proof.className = "act";
    proof.textContent = "Price-match proof";
    proof.addEventListener("click", () => proofCard(product, result));
    actions.append(watch, proof);
    body.append(actions);

    // What the page itself says comes first; the network answers follow.
    const reviews = reviewHealth();
    if (reviews) body.append(line("muted", span("", reviews)));
    const seller = sellerNote();
    if (seller) body.append(line("muted", span("", seller)));
    const store = await send({ type: "shop.storeCheck", tooGood: result.tooGood, http: location.protocol === "http:" });
    if (store && store.verdict) {
      const age = store.days !== null ? `domain registered ${store.days} days ago${store.registrar ? " via " + store.registrar : ""}` : "domain age unknown";
      const tone = store.verdict === "avoid" ? "save" : "muted";
      body.append(line(tone, span("", `Store check: ${store.verdict} (${age}${store.warnings.length ? "; " + store.warnings.join("; ") : ""}).`)));
    }
    const reddit = await send({ type: "shop.reddit", query: result.query });
    if (reddit && reddit.searchUrl) {
      const row = line("links");
      for (const thread of (reddit.threads || []).slice(0, 3)) row.append(link(thread.url, `r/${thread.subreddit}: ${thread.title.slice(0, 60)} (${thread.ups}↑)`));
      row.append(link(reddit.searchUrl, "more on reddit"));
      body.append(line("muted", span("", "What people say, away from the store's own reviews:")), row);
    }
    const store2 = location.hostname.replace(/^www\./, "").split(".")[0];
    const cards = line("links");
    cards.append(link(`https://www.raise.com/search?query=${encodeURIComponent(store2)}`, "discounted gift cards: Raise"), link(`https://www.cardcash.com/buy-gift-cards/?search=${encodeURIComponent(store2)}`, "CardCash"));
    body.append(line("muted", span("", "A discounted gift card is another few percent off:")), cards);
    const returns = await returnPolicy();
    if (returns) body.append(line("muted", span("", returns)));
  }

  // The struck-through "list price" beside the offer, if any.
  function wasPrice() {
    const candidates = document.querySelectorAll("s, del, strike, [class*='strike' i], [class*='was' i], [class*='list-price' i], [class*='basisPrice' i], .a-text-price .a-offscreen");
    for (const element of candidates) {
      const value = parsePrice(text(element));
      if (value) return value;
    }
    return null;
  }

  // Amazon's own review data, read from the page: how many, how they cluster.
  function reviewHealth() {
    if (!/amazon\./.test(location.hostname)) return null;
    const rating = parseFloat((text(document.querySelector("#acrPopover")) .match(/[\d.]+/) || [])[0]);
    const count = parseInt((text(document.querySelector("#acrCustomerReviewText")).replace(/[^\d]/g, "")) || "0", 10);
    const reviews = Array.from(document.querySelectorAll("[data-hook='review']"));
    if (!reviews.length && !count) return null;
    const notes = [];
    let score = 100;
    const five = document.querySelector("#histogramTable tr:first-child .a-text-right, #histogramTable li:first-child .a-text-right, [data-hook='histogram-row'] :nth-child(3)");
    const fiveShare = five ? parseInt(text(five).replace(/[^\d]/g, ""), 10) : null;
    if (fiveShare !== null && fiveShare >= 90 && count > 50) { score -= 25; notes.push(`${fiveShare}% five-star, which real products rarely reach`); }
    if (reviews.length) {
      const verified = reviews.filter((review) => /verified purchase/i.test(text(review))).length;
      if (verified / reviews.length < 0.5) { score -= 20; notes.push(`only ${verified} of ${reviews.length} shown reviews are verified purchases`); }
      const dates = reviews.map((review) => Date.parse((text(review.querySelector("[data-hook='review-date']")).match(/on (.+)$/) || [])[1] || "")).filter(Boolean).sort();
      if (dates.length >= 5 && dates[dates.length - 1] - dates[0] < 10 * 86400000) { score -= 25; notes.push("the shown reviews all landed within ten days"); }
      const bodies = reviews.map((review) => text(review.querySelector("[data-hook='review-body']")).toLowerCase().slice(0, 80));
      if (new Set(bodies).size < bodies.length) { score -= 20; notes.push("some reviews repeat each other word for word"); }
      const short = reviews.filter((review) => text(review.querySelector("[data-hook='review-body']")).length < 40).length;
      if (short / reviews.length > 0.6) { score -= 10; notes.push("most reviews are a few words"); }
    }
    const verdict = score >= 80 ? "look organic" : score >= 55 ? "mixed" : "look manufactured";
    send({ type: "safety.event", kind: "reviewsChecked", amount: 1 });
    return `Reviews ${verdict} (${rating || "?"}★ from ${count || reviews.length}${notes.length ? ": " + notes.join("; ") : ""}).`;
  }

  // Who actually sells it, on Amazon and eBay.
  function sellerNote() {
    if (/amazon\./.test(location.hostname)) {
      const soldBy = text(document.querySelector("#sellerProfileTriggerId, #merchant-info, [offer-display-feature-name='desktop-merchant-info']"));
      if (!soldBy) return null;
      if (/amazon\.(com|co\.uk|de|ca|fr|it|es)|amazon\b/i.test(soldBy) && !/sold by (?!amazon)/i.test(soldBy)) return "Sold by Amazon itself.";
      return `Third-party seller: ${soldBy.slice(0, 80)}. Open the seller's page and check the feedback percentage and how long they have sold; a new seller with few ratings and a very low price is the counterfeit pattern.`;
    }
    if (/ebay\./.test(location.hostname)) {
      const feedback = text(document.querySelector(".ux-seller-section__item--seller, .x-sellercard-atf__info, #RightSummaryPanel .ux-seller-section"));
      const percent = (feedback.match(/([\d.]+)% positive/i) || [])[1];
      const count = (feedback.match(/\(([\d,]+)\)/) || [])[1];
      if (!percent) return null;
      const number = parseInt((count || "0").replace(/,/g, ""), 10);
      const trust = parseFloat(percent) >= 99 && number >= 100 ? "solid" : parseFloat(percent) >= 97 && number >= 20 ? "acceptable" : "thin";
      return `Seller trust ${trust}: ${percent}% positive from ${count || "?"} ratings.`;
    }
    return null;
  }

  // The site's own return policy page, and the number of days in it.
  async function returnPolicy() {
    const anchor = Array.from(document.querySelectorAll("a[href]")).find((a) => /return/i.test(text(a)) && /return|refund/i.test(a.href) && a.hostname === location.hostname);
    if (!anchor) return null;
    try {
      const controller = new AbortController();
      setTimeout(() => controller.abort(), 6000);
      const response = await fetch(anchor.href, { credentials: "omit", signal: controller.signal });
      const html = await response.text();
      const plain = html.replace(/<script[\s\S]*?<\/script>|<style[\s\S]*?<\/style>|<[^>]+>/g, " ").replace(/\s+/g, " ");
      const days = /(\d{1,3})[- ]day/i.exec(plain);
      const restock = /restocking fee/i.test(plain);
      return `Returns: ${days ? days[1] + " days" : "see policy"}${restock ? ", a restocking fee is mentioned" : ""} (${anchor.href}).`;
    } catch {
      return `Return policy: ${anchor.href}`;
    }
  }

  // A picture to show the other store: what you found, where, when.
  function proofCard(product, result) {
    const canvas = document.createElement("canvas");
    canvas.width = 900;
    canvas.height = 420;
    const context = canvas.getContext("2d");
    context.fillStyle = "#0b0e0c";
    context.fillRect(0, 0, canvas.width, canvas.height);
    context.fillStyle = "#d8ddd6";
    context.font = "22px Georgia, serif";
    context.fillText("Price match request", 30, 50);
    context.font = "15px system-ui, sans-serif";
    const lines = [
      `Product: ${product.title.slice(0, 90)}`,
      `Here: ${location.hostname} at ${money(product.price, result.currency)}`,
      ...result.cheaper.slice(0, 4).map((offer) => `${offer.store}: ${money(offer.total, result.currency)}${offer.shippingKnown ? " incl. shipping" : ""}  ${offer.url.slice(0, 80)}`),
      `Checked ${new Date().toLocaleString()} by noah shield`,
    ];
    lines.forEach((entry, index) => context.fillText(entry, 30, 95 + index * 30));
    canvas.toBlob((blob) => {
      if (!blob) return;
      const anchor = document.createElement("a");
      anchor.href = URL.createObjectURL(blob);
      anchor.download = "price-match-" + Date.now() + ".png";
      anchor.click();
      setTimeout(() => URL.revokeObjectURL(anchor.href), 5000);
    });
  }

  function clipAmazonCoupon() {
    if (!/amazon\./.test(location.hostname)) return;
    const box = document.querySelector('#promoPriceBlockMessage_feature_div input[type="checkbox"]:not(:checked), [id^="couponText"] input[type="checkbox"]:not(:checked), input[type="checkbox"][name*="coupon" i]:not(:checked)');
    if (!box) return;
    box.click();
    card().append(line("muted", span("", "Clipped the coupon Amazon shows on this page.")));
    send({ type: "shop.saved", amount: 0 });
  }

  // ---- codes the store shows on its own pages ----------------------------

  let _couponNonce;
  try {
    _couponNonce = sessionStorage.getItem("_ns_nonce");
    if (!_couponNonce) {
      _couponNonce = Math.random().toString(36).slice(2);
      sessionStorage.setItem("_ns_nonce", _couponNonce);
    }
  } catch { _couponNonce = "x"; }

  function harvestCodes() {
    const seen = new Set();
    const bodyText = (document.body && document.body.innerText || "").slice(0, 200000);
    const pattern = /\b(?:code|coupon|promo)\s*[:\-]?\s*[""']?([A-Z][A-Z0-9]{3,19})\b/g;
    let match;
    while ((match = pattern.exec(bodyText)) && seen.size < 10) {
      const code = match[1];
      if (/^(CODE|PROMO|COUPON|HERE|NOW|SAVE|FREE|SHOP|ONLY|WITH|YOUR|THIS|THAT|FROM|SALE)$/.test(code)) continue;
      seen.add(code);
    }
    const key = "_ns_c_" + _couponNonce + ":" + location.hostname;
    let remembered = [];
    try {
      remembered = JSON.parse(sessionStorage.getItem(key) || "[]");
    } catch {
      remembered = [];
    }
    const fresh = Array.from(seen).filter((code) => !remembered.includes(code));
    if (!fresh.length) return;
    try {
      sessionStorage.setItem(key, JSON.stringify(remembered.concat(fresh)));
    } catch {
      // Session storage may be full or forbidden; the codes still go to the background.
    }
    send({ type: "shop.seen", codes: fresh });
  }

  // ---- checkout ----------------------------------------------------------

  function visible(element) {
    if (!element || !element.isConnected) return false;
    const rectangle = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return rectangle.width > 20 && rectangle.height > 10 && style.visibility !== "hidden" && style.display !== "none";
  }

  function describe(input) {
    const label = input.labels && input.labels[0] ? text(input.labels[0]) : "";
    const byFor = input.id ? text(document.querySelector(`label[for="${CSS.escape(input.id)}"]`)) : "";
    return [input.id, input.name, input.placeholder, input.getAttribute("aria-label"), input.getAttribute("data-testid"), label, byFor].filter(Boolean).join(" ");
  }

  function couponField() {
    const inputs = Array.from(document.querySelectorAll('input[type="text"], input:not([type]), input[type="search"]'));
    return inputs.find((input) => {
      const words = describe(input);
      return FIELD_HINT.test(words) && !FIELD_AVOID.test(words) && visible(input) && !input.disabled && !input.readOnly;
    }) || null;
  }

  function revealCouponField() {
    const candidates = Array.from(document.querySelectorAll("a, button, summary, span, div, label"))
      .filter((element) => element.children.length < 3 && visible(element))
      .filter((element) => {
        const words = text(element);
        return words.length < 60 && REVEAL_HINT.test(words) && !ORDER_BUTTON.test(words);
      });
    if (!candidates.length) return false;
    candidates[0].click();
    return true;
  }

  function applyButton(input) {
    const scope = input.closest("form, fieldset, section, div[class*='promo' i], div[class*='coupon' i], div[class*='discount' i]") || input.parentElement;
    const buttons = Array.from((scope || document).querySelectorAll("button, input[type='submit'], input[type='button'], a[role='button']"));
    return buttons.find((button) => {
      const words = text(button) || button.value || button.getAttribute("aria-label") || "";
      return visible(button) && !ORDER_BUTTON.test(words) && /apply|redeem|add|submit|use|ok|go|→|>/i.test(words || "apply");
    }) || null;
  }

  function readTotal() {
    const rows = Array.from(document.querySelectorAll("tr, li, div, p, dt, dd, span, td, th"))
      .filter((element) => element.children.length <= 6)
      .filter((element) => {
        const words = text(element);
        return words.length < 80 && /^(order |grand |estimated |cart |bag |basket )?total\b|^total (due|price|amount|cost)|amount due/i.test(words);
      });
    for (const row of rows.reverse()) {
      const scope = row.closest("tr, li, dl, div") || row;
      const value = parsePrice((text(scope).match(/[\d.,]+\s*(?:€|£|\$|¥)?$|(?:€|£|\$|¥|C\$|A\$)\s*[\d.,]+/g) || []).pop() || "");
      if (value) return value;
    }
    return null;
  }

  function setValue(input, value) {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    input.focus();
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  // The store's complaint about a code, so a stale one from the previous try
  // is not mistaken for an answer to this one.
  function failureNear(input) {
    const scope = input.closest("form, section, div[class*='promo' i], div[class*='coupon' i], div[class*='discount' i]") || document.body;
    const words = text(scope).slice(0, 4000);
    const match = FAILURE_HINT.exec(words);
    return match ? words.slice(Math.max(0, match.index - 40), match.index + 60) : "";
  }

  async function tryCodes(config, manual) {
    let input = couponField();
    if (!input && revealCouponField()) {
      await sleep(1200);
      input = couponField();
    }
    if (!input) {
      if (manual) card().append(line("muted", span("", "No coupon field on this page yet. Open the promo code box and try again.")));
      return;
    }
    const once = "noah-shield-tried:" + location.pathname;
    if (!manual && sessionStorage.getItem(once)) return;
    const { codes, site } = await send({ type: "shop.codes" });
    if (!codes || !codes.length) {
      const body = card();
      body.replaceChildren(line("muted", span("", "No codes known for " + (site || location.hostname) + " yet. Codes the store shows on its pages are picked up as you browse; add your own in the shield's settings.")));
      return;
    }
    try {
      sessionStorage.setItem(once, "1");
    } catch {
      // Without session storage the run may repeat after a reload; that is harmless.
    }
    const body = card();
    const status = line("muted", span("", "Trying " + codes.length + " codes…"));
    const bar = line("bar", document.createElement("i"));
    body.replaceChildren(status, bar);
    const before = readTotal();
    let best = null;
    let index = 0;
    for (const candidate of codes) {
      index++;
      status.firstChild.textContent = `Trying ${candidate.code} (${index} of ${codes.length}, ${candidate.source})…`;
      bar.firstChild.style.width = Math.round((index / codes.length) * 100) + "%";
      const field = couponField() || input;
      const previous = readTotal();
      const complaintBefore = failureNear(field);
      setValue(field, candidate.code);
      const button = applyButton(field);
      if (button) button.click();
      else field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", keyCode: 13, bubbles: true }));
      let outcome = "unknown";
      for (let waited = 0; waited < 8000; waited += 400) {
        await sleep(400);
        const now = readTotal();
        if (previous && now && now < previous - 0.009) {
          outcome = "worked";
          break;
        }
        const complaint = failureNear(field);
        if (complaint && complaint !== complaintBefore) {
          // Stores answer within a moment of complaining; a lowered total
          // right after still counts.
          await sleep(800);
          const later = readTotal();
          outcome = previous && later && later < previous - 0.009 ? "worked" : "failed";
          break;
        }
      }
      if (outcome === "worked") {
        const now = readTotal();
        const saved = previous - now;
        if (!best || saved > best.saved) best = { code: candidate.code, saved, total: now };
        send({ type: "shop.worked", code: candidate.code });
        send({ type: "shop.saved", amount: saved });
      }
      if (!document.body.contains(field) && !couponField()) break;
    }
    body.replaceChildren();
    if (best) {
      body.append(line("row", span("grow", "Applied"), span("code", best.code), span("save", "saved " + money(best.saved, currencyOf(document.body.innerText.slice(0, 5000)) || "USD"))));
      if (before && best.total) body.append(line("muted", span("", "Total went from " + money(before, currencyOf(document.body.innerText.slice(0, 5000)) || "USD") + " to " + money(best.total, currencyOf(document.body.innerText.slice(0, 5000)) || "USD") + ". The order is still yours to place.")));
    } else {
      body.append(line("muted", span("", "None of the " + codes.length + " codes lowered the total. Nothing was ordered.")));
    }
    const again = document.createElement("button");
    again.className = "act";
    again.textContent = "Try again";
    again.addEventListener("click", () => tryCodes(config, true));
    body.append(line("actions", again));
  }

  function looksLikeCheckout() {
    return /\/(checkout|cart|basket|bag|payment|order|purchase)\b/i.test(location.pathname) || Boolean(couponField());
  }

  // ---- start -------------------------------------------------------------

  waitForConfig().then(async (config) => {
    if (!config || !config.shopping || config.shoppingQuiet) return;
    harvestCodes();
    setTimeout(harvestCodes, 5000);
    if (looksLikeCheckout()) {
      if (config.autoApply) {
        await sleep(1500);
        tryCodes(config, false);
      } else {
        const body = card();
        const button = document.createElement("button");
        button.className = "act";
        button.textContent = "Try coupon codes";
        button.addEventListener("click", () => tryCodes(config, true));
        body.replaceChildren(line("muted", span("", "This looks like a checkout.")), line("actions", button));
      }
      return;
    }
    const product = findProduct();
    if (product) compare(product, config);
  });
})();
