// Shopping: the same product at other stores, and codes that still work.
// Price lookups are plain fetches of the other stores' public search pages,
// sent without cookies and carrying nothing but the product's name; nothing
// about you goes anywhere. Stores that cannot be read this way still get a
// direct search link, so there is always a way to compare.
"use strict";
(() => {

const Shield = (globalThis.Shield = globalThis.Shield || {});

const LOOKUP_TIMEOUT_MS = 9000;
const STOPWORDS = new Set(["the", "and", "for", "with", "of", "a", "an", "to", "in", "on", "by", "new", "pack", "set", "inch", "inches"]);

function decodeEntities(text) {
  return String(text)
    .replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"')
    .replace(/&#39;|&apos;/g, "'").replace(/&nbsp;/g, " ").replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code)));
}

function stripTags(html) {
  return decodeEntities(String(html).replace(/<[^>]+>/g, " ")).replace(/\s+/g, " ").trim();
}

Shield.parsePrice = function parsePrice(text) {
  const cleaned = String(text).replace(/[^\d.,]/g, "");
  if (!cleaned) return null;
  let normalized = cleaned;
  if (/,\d{2}$/.test(cleaned) && !/\.\d{2}$/.test(cleaned)) normalized = cleaned.replace(/\./g, "").replace(",", ".");
  else normalized = cleaned.replace(/,/g, "");
  const value = parseFloat(normalized);
  return Number.isFinite(value) && value > 0 ? value : null;
};

function tokens(title) {
  return String(title)
    .toLowerCase()
    .replace(/[^a-z0-9.+ ]/g, " ")
    .split(/\s+/)
    .filter((token) => token.length > 1 && !STOPWORDS.has(token));
}

// A result is the same product when it shares most of the words, and every
// word with a digit (model numbers, sizes) that the query had.
Shield.similarity = function similarity(query, candidate) {
  const wanted = tokens(query);
  const have = new Set(tokens(candidate));
  if (!wanted.length) return 0;
  const numeric = wanted.filter((token) => /\d/.test(token));
  if (numeric.some((token) => !have.has(token))) return 0;
  const shared = wanted.filter((token) => have.has(token)).length;
  return shared / wanted.length;
};

const FINDERS = [
  {
    id: "ebay",
    name: "eBay",
    currencies: { USD: "https://www.ebay.com", GBP: "https://www.ebay.co.uk", EUR: "https://www.ebay.de", CAD: "https://www.ebay.ca", AUD: "https://www.ebay.com.au" },
    search(base, query) {
      return `${base}/sch/i.html?_nkw=${encodeURIComponent(query)}&LH_BIN=1&LH_ItemCondition=1000&_sop=15`;
    },
    parse(html) {
      const offers = [];
      const items = html.split(/class="s-item\b/).slice(1, 40);
      for (const item of items) {
        const title = /class="s-item__title[^"]*"[^>]*>(?:<span[^>]*>)?([\s\S]*?)<\/(?:span|div)>/.exec(item);
        const price = /class="s-item__price"[^>]*>([\s\S]*?)<\/span>/.exec(item);
        const link = /href="(https:\/\/www\.ebay\.[a-z.]+\/itm\/[^"?]+)/.exec(item);
        if (!title || !price || !link) continue;
        const text = stripTags(title[1]);
        if (/^shop on ebay$/i.test(text)) continue;
        const shippingText = /class="s-item__(?:shipping|logisticsCost)[^"]*"[^>]*>([\s\S]*?)<\/span>/.exec(item);
        const shippingWords = shippingText ? stripTags(shippingText[1]) : "";
        const shipping = /free/i.test(shippingWords) ? 0 : shippingWords ? Shield.parsePrice(shippingWords) : null;
        offers.push({ title: text, price: Shield.parsePrice(stripTags(price[1])), url: link[1], shipping });
      }
      return offers;
    },
  },
  {
    id: "newegg",
    name: "Newegg",
    currencies: { USD: "https://www.newegg.com", CAD: "https://www.newegg.ca" },
    search(base, query) {
      return `${base}/p/pl?d=${encodeURIComponent(query)}&Order=1`;
    },
    parse(html) {
      const offers = [];
      const items = html.split(/class="item-cell"/).slice(1, 40);
      for (const item of items) {
        const link = /<a[^>]+class="item-title"[^>]+href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/.exec(item);
        const whole = /class="price-current"[^>]*>[\s\S]*?<strong>([\d,]+)<\/strong><sup>([.\d]+)<\/sup>/.exec(item);
        if (!link || !whole) continue;
        offers.push({ title: stripTags(link[2]), price: Shield.parsePrice(whole[1] + whole[2]), url: link[1] });
      }
      return offers;
    },
  },
  {
    id: "walmart",
    name: "Walmart",
    currencies: { USD: "https://www.walmart.com", CAD: "https://www.walmart.ca" },
    search(base, query) {
      return `${base}/search?q=${encodeURIComponent(query)}&sort=price_low`;
    },
    parse(html, base) {
      const data = /<script id="__NEXT_DATA__"[^>]*>([\s\S]*?)<\/script>/.exec(html);
      if (!data) return [];
      const offers = [];
      try {
        const json = JSON.parse(data[1]);
        const stacks = json.props?.pageProps?.initialData?.searchResult?.itemStacks || [];
        for (const stack of stacks) {
          for (const item of stack.items || []) {
            const price = item.priceInfo?.linePrice || item.price;
            if (!item.name || !price || !item.canonicalUrl) continue;
            offers.push({ title: item.name, price: Shield.parsePrice(String(price)), url: base + item.canonicalUrl.split("?")[0] });
          }
        }
      } catch {
        return [];
      }
      return offers;
    },
  },
  {
    id: "bestbuy",
    name: "Best Buy",
    currencies: { USD: "https://www.bestbuy.com", CAD: "https://www.bestbuy.ca" },
    search(base, query) {
      return `${base}/site/searchpage.jsp?st=${encodeURIComponent(query)}&sp=-currentprice%20skuidsaas`;
    },
    parse(html, base) {
      const offers = [];
      const items = html.split(/class="sku-item"/).slice(1, 40);
      for (const item of items) {
        const link = /<h4 class="sku-title">\s*<a[^>]+href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/.exec(item);
        const price = /class="priceView-customer-price"[^>]*>\s*<span[^>]*>([^<]+)</.exec(item);
        if (!link || !price) continue;
        offers.push({ title: stripTags(link[2]), price: Shield.parsePrice(price[1]), url: link[1].startsWith("http") ? link[1] : base + link[1] });
      }
      return offers;
    },
  },
  {
    id: "amazon",
    name: "Amazon",
    currencies: { USD: "https://www.amazon.com", GBP: "https://www.amazon.co.uk", EUR: "https://www.amazon.de", CAD: "https://www.amazon.ca", AUD: "https://www.amazon.com.au", JPY: "https://www.amazon.co.jp" },
    search(base, query) {
      return `${base}/s?k=${encodeURIComponent(query)}&s=price-asc-rank`;
    },
    parse(html, base) {
      const offers = [];
      const items = html.split(/data-component-type="s-search-result"/).slice(1, 40);
      for (const item of items) {
        const asin = /data-asin="([A-Z0-9]{10})"/.exec(item);
        const title = /<h2[^>]*>[\s\S]*?<span[^>]*>([\s\S]*?)<\/span>/.exec(item);
        const whole = /class="a-price-whole">([\d,.]+)</.exec(item);
        const fraction = /class="a-price-fraction">(\d+)</.exec(item);
        if (!asin || !title || !whole) continue;
        offers.push({
          title: stripTags(title[1]),
          price: Shield.parsePrice(whole[1].replace(/[.,]$/, "") + (fraction ? "." + fraction[1] : "")),
          url: `${base}/dp/${asin[1]}`,
        });
      }
      return offers;
    },
  },
];

// Stores that cannot be read as plain pages still deserve a link.
const LINK_ONLY = [
  { name: "Google Shopping", currencies: ["USD", "GBP", "EUR", "CAD", "AUD", "JPY"], url: (query) => `https://www.google.com/search?tbm=shop&q=${encodeURIComponent(query)}` },
  { name: "Target", currencies: ["USD"], url: (query) => `https://www.target.com/s?searchTerm=${encodeURIComponent(query)}` },
  { name: "Costco", currencies: ["USD"], url: (query) => `https://www.costco.com/s?keyword=${encodeURIComponent(query)}` },
  { name: "B&H", currencies: ["USD"], url: (query) => `https://www.bhphotovideo.com/c/search?q=${encodeURIComponent(query)}` },
  { name: "idealo", currencies: ["EUR"], url: (query) => `https://www.idealo.de/preisvergleich/MainSearchProductCategory.html?q=${encodeURIComponent(query)}` },
  { name: "Geizhals", currencies: ["EUR"], url: (query) => `https://geizhals.eu/?fs=${encodeURIComponent(query)}` },
  { name: "PriceRunner", currencies: ["GBP"], url: (query) => `https://www.pricerunner.com/search?q=${encodeURIComponent(query)}` },
  { name: "PriceSpy", currencies: ["GBP"], url: (query) => `https://pricespy.co.uk/search?search=${encodeURIComponent(query)}` },
  { name: "AliExpress", currencies: ["USD", "EUR", "GBP"], url: (query) => `https://www.aliexpress.com/w/wholesale-${encodeURIComponent(query).replace(/%20/g, "-")}.html` },
];

// Amazon titles run long; the first dozen words, minus marketing, find the product.
Shield.searchQuery = function searchQuery(product) {
  let title = String(product.title || "").replace(/\(.*?\)|\[.*?\]/g, " ");
  const cut = title.search(/[,|–—-]\s|\bwith\b|\bfor\b/i);
  if (cut > 20) title = title.slice(0, cut);
  const words = title.split(/\s+/).filter(Boolean).slice(0, 12);
  if (product.brand && !words.join(" ").toLowerCase().includes(String(product.brand).toLowerCase())) words.unshift(product.brand);
  return words.join(" ").trim();
};

async function fetchPage(url) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), LOOKUP_TIMEOUT_MS);
  try {
    const response = await fetch(url, {
      credentials: "omit",
      redirect: "follow",
      signal: controller.signal,
      headers: { accept: "text/html,application/xhtml+xml", "accept-language": "en" },
    });
    if (!response.ok) return null;
    const text = await response.text();
    return text.length > 400 ? text : null;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

Shield.compareProduct = async function compareProduct(product) {
  const currency = String(product.currency || "USD").toUpperCase();
  const query = Shield.searchQuery(product);
  const here = Shield.siteOf(Shield.hostOf(product.url || ""));
  const price = Number(product.price) || 0;
  const lookups = FINDERS.filter((finder) => finder.currencies[currency] && !Shield.siteOf(Shield.hostOf(finder.currencies[currency])).startsWith(here.split(".")[0] + "."))
    .map(async (finder) => {
      const base = finder.currencies[currency];
      const url = finder.search(base, query);
      const html = await fetchPage(url);
      let offers = [];
      if (html) {
        try {
          offers = finder.parse(html, base);
        } catch {
          offers = [];
        }
      }
      const matches = offers
        .filter((offer) => offer.price && offer.title && offer.url)
        .map((offer) => ({ ...offer, store: finder.name, score: Shield.similarity(query, offer.title), total: offer.price + (typeof offer.shipping === "number" ? offer.shipping : 0), shippingKnown: typeof offer.shipping === "number" }))
        .filter((offer) => offer.score >= 0.6 && (!price || offer.price >= price * 0.15))
        .sort((left, right) => left.total - right.total)
        .slice(0, 3);
      return { store: finder.name, searchUrl: url, readable: Boolean(html), offers: matches };
    });
  const results = await Promise.all(lookups);
  const links = LINK_ONLY.filter((store) => store.currencies.includes(currency) && !Shield.hostOf(store.url("x")).includes(here.split(".")[0]))
    .map((store) => ({ store: store.name, searchUrl: store.url(query) }));
  const offers = results.flatMap((result) => result.offers).sort((left, right) => left.total - right.total);
  const median = offers.length ? offers[Math.floor(offers.length / 2)].total : null;
  return {
    query,
    currency,
    price,
    median,
    tooGood: Boolean(price && median && price < median * 0.4),
    offers: offers.slice(0, 6),
    cheaper: offers.filter((offer) => price && offer.total < price),
    searched: results.map(({ store, searchUrl, readable, offers: found }) => ({ store, searchUrl, readable, found: found.length })),
    links,
  };
};

// Codes worth trying at a store, best guesses first: what worked here before,
// what the store itself advertised, what you added, then noah's list.
Shield.codesFor = async function codesFor(site, settings) {
  const [stored, feed] = await Promise.all([Shield.api.storage.local.get(["workedCodes", "seenCodes"]), Shield.feedData("coupons.json")]);
  const now = Date.now();
  const candidates = [];
  const seenSet = new Set();
  const add = (code, source, description = "") => {
    const clean = String(code || "").trim().toUpperCase();
    if (!/^[A-Z0-9][A-Z0-9-]{2,24}$/.test(clean) || seenSet.has(clean)) return;
    seenSet.add(clean);
    candidates.push({ code: clean, source, description });
  };
  for (const code of (stored.workedCodes || {})[site] || []) add(code, "worked here before");
  for (const entry of (stored.seenCodes || {})[site] || []) {
    if (now - entry.at < 14 * 86400000) add(entry.code, "shown by the store");
  }
  for (const entry of settings.shopping.codes) {
    if (!entry.site || entry.site === site) add(entry.code, "yours", entry.note || "");
  }
  if (feed && Array.isArray(feed.coupons)) {
    for (const entry of feed.coupons) {
      if (entry.site !== site) continue;
      if (entry.expires && Date.parse(entry.expires) < now) continue;
      add(entry.code, "noah's list, checked " + String(entry.verified || "").slice(0, 10), entry.description || "");
    }
  }
  return candidates.slice(0, 12);
};

Shield.rememberCode = async function rememberCode(site, code, worked) {
  const stored = await Shield.api.storage.local.get(["workedCodes", "seenCodes"]);
  if (worked) {
    const workedCodes = stored.workedCodes || {};
    const list = workedCodes[site] || [];
    if (!list.includes(code)) list.unshift(code);
    workedCodes[site] = list.slice(0, 10);
    await Shield.api.storage.local.set({ workedCodes });
    await Shield.count("coupons", 1);
  } else {
    const seenCodes = stored.seenCodes || {};
    const list = (seenCodes[site] || []).filter((entry) => entry.code !== code);
    list.unshift({ code, at: Date.now() });
    seenCodes[site] = list.slice(0, 20);
    await Shield.api.storage.local.set({ seenCodes });
  }
};
// ---- price history and the watchlist -------------------------------------------

function productKey(product) {
  const site = Shield.siteOf(Shield.hostOf(product.url || ""));
  if (product.code) return site + "|" + String(product.code);
  try {
    const url = new URL(product.url);
    return site + "|" + url.pathname.slice(0, 120);
  } catch {
    return site + "|" + String(product.title || "").slice(0, 80);
  }
}

// One point per day per product, kept for a year.
Shield.recordPrice = async function recordPrice(product) {
  const key = productKey(product);
  const price = Number(product.price);
  if (!Number.isFinite(price) || price <= 0) return null;
  const stored = await Shield.api.storage.local.get(["priceHistory", "watchlist"]);
  const history = stored.priceHistory || {};
  const points = history[key] || [];
  const today = Shield.todayKey();
  const last = points[points.length - 1];
  if (last && last.day === today) last.price = price;
  else points.push({ day: today, price });
  history[key] = points.slice(-366);
  const keys = Object.keys(history);
  if (keys.length > 400) delete history[keys[0]];
  await Shield.api.storage.local.set({ priceHistory: history });
  return Shield.summarizeHistory(history[key], (stored.watchlist || []).some((entry) => entry.key === key));
};

Shield.summarizeHistory = function summarizeHistory(points, watching) {
  const prices = points.map((point) => point.price).sort((left, right) => left - right);
  const current = points[points.length - 1].price;
  const median = prices[Math.floor(prices.length / 2)];
  return {
    points: points.slice(-60),
    count: points.length,
    since: points[0].day,
    low: prices[0],
    high: prices[prices.length - 1],
    median,
    current,
    watching: Boolean(watching),
    // "Buy later": the price sits above what it usually is.
    waitHint: points.length >= 3 && current > median * 1.05,
  };
};

Shield.priceHistoryFor = async function priceHistoryFor(product) {
  const stored = await Shield.api.storage.local.get(["priceHistory", "watchlist"]);
  const key = productKey(product);
  const points = (stored.priceHistory || {})[key];
  if (!points || !points.length) return null;
  return Shield.summarizeHistory(points, (stored.watchlist || []).some((entry) => entry.key === key));
};

Shield.watchProduct = async function watchProduct(product) {
  const stored = await Shield.api.storage.local.get("watchlist");
  const watchlist = stored.watchlist || [];
  const key = productKey(product);
  if (!watchlist.some((entry) => entry.key === key)) {
    watchlist.push({ key, url: product.url, title: String(product.title || "").slice(0, 120), price: Number(product.price) || null, currency: product.currency || "USD", kind: product.kind || "product", added: new Date().toISOString(), lastChecked: null });
  }
  await Shield.api.storage.local.set({ watchlist: watchlist.slice(0, 100) });
  return { watching: true };
};

Shield.unwatchProduct = async function unwatchProduct(url) {
  const stored = await Shield.api.storage.local.get("watchlist");
  const watchlist = (stored.watchlist || []).filter((entry) => entry.url !== url);
  await Shield.api.storage.local.set({ watchlist });
  return { watching: false };
};

// The price on a fetched page: structured data first, then Amazon's markup,
// then meta tags, then a checkout total.
Shield.priceFromHtml = function priceFromHtml(html, kind) {
  if (kind === "cart") {
    const total = /(?:order |grand |estimated )?total[^<\d$€£¥]{0,80}((?:\$|€|£|¥)\s?[\d.,]+|[\d.,]+\s?(?:€|£|\$))/i.exec(stripTags(html).slice(0, 200000));
    return total ? Shield.parsePrice(total[1]) : null;
  }
  const scripts = html.match(/<script type="application\/ld\+json"[^>]*>([\s\S]*?)<\/script>/gi) || [];
  for (const script of scripts) {
    try {
      const json = JSON.parse(script.replace(/^<script[^>]*>/i, "").replace(/<\/script>$/i, ""));
      const nodes = Array.isArray(json) ? json : json && json["@graph"] ? json["@graph"] : [json];
      for (const node of nodes) {
        if (!node || !/Product/i.test(String(node["@type"]))) continue;
        let offer = node.offers;
        if (Array.isArray(offer)) offer = offer[0];
        const value = offer && (offer.price ?? offer.lowPrice);
        if (value !== undefined) return Shield.parsePrice(String(value));
      }
    } catch {
      continue;
    }
  }
  const amazon = /id="corePrice_feature_div"[\s\S]{0,3000}?class="a-offscreen">([^<]+)</.exec(html) || /class="a-price[^"]*"[^>]*>\s*<span class="a-offscreen">([^<]+)</.exec(html);
  if (amazon) return Shield.parsePrice(amazon[1]);
  const meta = /<meta property="(?:product|og):price:amount" content="([^"]+)"/.exec(html);
  if (meta) return Shield.parsePrice(meta[1]);
  const micro = /itemprop="price"[^>]*content="([^"]+)"/.exec(html);
  return micro ? Shield.parsePrice(micro[1]) : null;
};

// Every six hours: each watched page is fetched and its price recorded; a
// drop of three percent or more since the last look is announced.
Shield.checkWatchlist = async function checkWatchlist(notify) {
  const stored = await Shield.api.storage.local.get("watchlist");
  const watchlist = stored.watchlist || [];
  for (const entry of watchlist) {
    const html = await fetchPage(entry.url);
    if (!html) continue;
    const price = Shield.priceFromHtml(html, entry.kind);
    if (!price) continue;
    const previous = entry.price;
    entry.price = price;
    entry.lastChecked = new Date().toISOString();
    await Shield.recordPrice({ url: entry.url, price, title: entry.title });
    if (previous && price <= previous * 0.97) {
      entry.dropped = { from: previous, to: price, at: entry.lastChecked };
      await notify(entry, previous, price);
      await Shield.count("drops", 1);
    }
  }
  await Shield.api.storage.local.set({ watchlist });
};

// ---- reviews on reddit ----------------------------------------------------------------

Shield.redditThreads = async function redditThreads(query) {
  const searchUrl = "https://www.reddit.com/search/?q=" + encodeURIComponent(query + " review");
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 8000);
  try {
    const response = await fetch("https://www.reddit.com/search.json?q=" + encodeURIComponent(query + " review") + "&limit=6&sort=relevance", { credentials: "omit", signal: controller.signal, headers: { accept: "application/json" } });
    if (!response.ok) return { threads: [], searchUrl };
    const data = await response.json();
    const threads = ((data.data && data.data.children) || []).map((child) => child.data).filter((post) => post && post.title).map((post) => ({ title: post.title, url: "https://www.reddit.com" + post.permalink, ups: post.ups || 0, subreddit: post.subreddit, comments: post.num_comments || 0 }));
    return { threads, searchUrl };
  } catch {
    return { threads: [], searchUrl };
  } finally {
    clearTimeout(timer);
  }
};

// ---- the store itself -------------------------------------------------------------------

// RDAP is the registries' own public record: when the domain was created.
Shield.domainAge = async function domainAge(site) {
  const stored = await Shield.api.storage.local.get("domainAges");
  const ages = stored.domainAges || {};
  const cached = ages[site];
  if (cached && Date.now() - cached.at < 7 * 86400000) return cached;
  let result = { at: Date.now(), registered: null, registrar: null, error: null };
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 9000);
  try {
    const response = await fetch("https://rdap.org/domain/" + encodeURIComponent(site), { credentials: "omit", signal: controller.signal, headers: { accept: "application/rdap+json, application/json" } });
    if (response.ok) {
      const data = await response.json();
      const registration = (data.events || []).find((event) => event.eventAction === "registration");
      result.registered = registration ? registration.eventDate : null;
      const registrar = (data.entities || []).find((entity) => (entity.roles || []).includes("registrar"));
      const card = registrar && registrar.vcardArray && registrar.vcardArray[1];
      const nameField = Array.isArray(card) ? card.find((field) => field[0] === "fn") : null;
      result.registrar = nameField ? nameField[3] : null;
    } else {
      result.error = "rdap answered " + response.status;
    }
  } catch (error) {
    result.error = String(error.message || error);
  } finally {
    clearTimeout(timer);
  }
  ages[site] = result;
  const keys = Object.keys(ages);
  if (keys.length > 500) delete ages[keys[0]];
  await Shield.api.storage.local.set({ domainAges: ages });
  return result;
};

Shield.storeCheck = async function storeCheck(host, hints = {}) {
  const site = Shield.siteOf(host);
  const age = await Shield.domainAge(site);
  const lookalike = Shield.lookalike(host);
  const days = age.registered ? Math.floor((Date.now() - Date.parse(age.registered)) / 86400000) : null;
  const warnings = [];
  if (lookalike) warnings.push(`the name imitates ${lookalike.brand} (${lookalike.reason})`);
  if (days !== null && days < 180) warnings.push(`the domain is ${days} days old`);
  if (hints.tooGood) warnings.push("the price is far below what other stores ask");
  if (hints.http) warnings.push("the checkout is not on https");
  return { site, days, registered: age.registered, registrar: age.registrar, lookalike, warnings, verdict: warnings.length >= 2 ? "avoid" : warnings.length === 1 ? "careful" : days === null ? "unknown" : "established" };
};

// ---- reminders, receipts, warranties ---------------------------------------------------------

Shield.addReminder = async function addReminder(reminder) {
  const stored = await Shield.api.storage.local.get("reminders");
  const reminders = stored.reminders || [];
  const entry = { id: "reminder-" + crypto.randomUUID(), at: reminder.at, label: String(reminder.label || "").slice(0, 160), url: String(reminder.url || "").slice(0, 500), kind: reminder.kind || "reminder" };
  reminders.push(entry);
  await Shield.api.storage.local.set({ reminders });
  await Shield.api.alarms.create(entry.id, { when: entry.at });
  return entry;
};

Shield.dueReminder = async function dueReminder(id) {
  const stored = await Shield.api.storage.local.get("reminders");
  const reminders = stored.reminders || [];
  const entry = reminders.find((reminder) => reminder.id === id);
  await Shield.api.storage.local.set({ reminders: reminders.filter((reminder) => reminder.id !== id) });
  return entry || null;
};

Shield.saveReceipt = async function saveReceipt(receipt) {
  const stored = await Shield.api.storage.local.get("receipts");
  const receipts = stored.receipts || [];
  if (receipts.some((entry) => entry.url === receipt.url)) return { saved: false };
  receipts.unshift({ at: new Date().toISOString(), site: String(receipt.site || "").slice(0, 100), total: Number(receipt.total) || null, currency: String(receipt.currency || "USD").slice(0, 3), orderNumber: String(receipt.orderNumber || "").slice(0, 60), title: String(receipt.title || "").slice(0, 160), url: String(receipt.url || "").slice(0, 500), items: String(receipt.items || "").slice(0, 400) });
  await Shield.api.storage.local.set({ receipts: receipts.slice(0, 500) });
  return { saved: true };
};

Shield.spendingByStore = function spendingByStore(receipts) {
  const months = {};
  for (const receipt of receipts) {
    if (!receipt.total) continue;
    const month = receipt.at.slice(0, 7);
    months[month] = months[month] || {};
    months[month][receipt.site] = (months[month][receipt.site] || 0) + receipt.total;
  }
  return months;
};

})();
