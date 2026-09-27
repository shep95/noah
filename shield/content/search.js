// Search results, cleaned: on Google, Bing, DuckDuckGo, Brave, Yahoo,
// Startpage and Ecosia the paid results go, pages from known content farms
// go, and pages that read as written for the search engine rather than for a
// person are dimmed, all with a count you can open. The same pages also wear
// the look the person chose, so a search does not break the picture.
(() => {
  "use strict";
  if (window.top !== window) return;
  const api = globalThis.chrome ?? globalThis.browser;
  const host = location.hostname.toLowerCase();

  const ENGINES = [
    {
      id: "google",
      match: /(^|\.)google\.[a-z.]{2,10}$/,
      page: () => new URLSearchParams(location.search).has("q") && !/^\/(maps|flights|travel|finance|books|scholar|patents|trends)\b/.test(location.pathname),
      ads: ["#tads", "#tadsb", "#bottomads", "#taw [data-text-ad]", "[data-text-ad]", ".commercial-unit-desktop-top", ".commercial-unit-desktop-rhs", ".cu-container", "#rhs .commercial-unit", "div[aria-label='Ads']", "div[aria-label='Sponsored']", ".ads-fr", "[data-pla='1']", "#tvcap"],
      results: ["#rso > div", "#rso .MjjYud", "#rso .g", "#search .g"],
      transparent: ["#main", "#cnt", "#rcnt", "#center_col", ".main", "#searchform", "#appbar", "#hdtb", ".sfbg", "#top_nav", "#before-appbar", "#rhs", "#footcnt", "#botstuff", ".appbar", "#kp-wp-tab-overview"],
    },
    {
      id: "bing",
      match: /(^|\.)bing\.com$/,
      page: () => location.pathname.startsWith("/search"),
      ads: [".b_ad", "#b_pole .b_adTop", ".b_adTop", ".b_adBottom", "li.b_ad", ".b_adLastChild", "#b_results > li.b_ad", ".ad_sc"],
      results: ["#b_results > li"],
      transparent: ["#b_header", "#b_content", "#b_results", ".b_scopebar", "#b_footer", "#b_context", ".b_searchboxForm"],
    },
    {
      id: "duckduckgo",
      match: /(^|\.)duckduckgo\.com$/,
      page: () => new URLSearchParams(location.search).has("q"),
      ads: ["[data-testid='ad']", "li[data-layout='ad']", ".badge--ad", "[data-layout='ad']", ".js-sponsored-results", ".results--ads", ".module--carousel-ads"],
      results: ["article[data-testid='result']", "li[data-layout='organic']", ".result"],
      transparent: ["#react-layout", "main", "header", ".header-wrap", ".results", "#links_wrapper", ".serp__results", "section"],
    },
    {
      id: "brave",
      match: /(^|\.)search\.brave\.com$/,
      page: () => location.pathname.startsWith("/search"),
      ads: [".ad", "[data-type='ad']", ".ad-result", ".ads", "#ads"],
      results: ["#results > .snippet", ".snippet"],
      transparent: ["#main", "main", "header", ".main-column", "#results", ".sidebar", "#side-right", ".header"],
    },
    {
      id: "yahoo",
      match: /(^|\.)search\.yahoo\.com$/,
      page: () => location.pathname.startsWith("/search"),
      ads: [".searchCenterTopAds", ".searchCenterBottomAds", ".searchRightTopAds", ".searchRightMiddleAds", "li.ad", ".AdTop", ".AdBttm", "#right .ads"],
      results: ["#web ol > li", "#web li"],
      transparent: ["#bd", "#main", "#web", "#right", "#header", ".sbq-w", "#ft"],
    },
    {
      id: "startpage",
      match: /(^|\.)startpage\.com$/,
      page: () => /\/(do|sp)\/search/.test(location.pathname),
      ads: [".ad", ".mainline-ad", "[class*='ad-']", ".css-ads"],
      results: [".w-gl__result", ".result", "[class*='w-gl__result']"],
      transparent: [".layout-web", ".w-gl", "header", "main", ".mainline", ".sidebar"],
    },
    {
      id: "ecosia",
      match: /(^|\.)ecosia\.org$/,
      page: () => location.pathname.startsWith("/search"),
      ads: ["[data-test-id='mainline-ad']", "[data-test-id='sidebar-ad']", ".card-ad", ".ad", ".ads"],
      results: ["[data-test-id='mainline-result']", ".result", ".mainline__result-wrapper article"],
      transparent: [".layout", ".layout__content", "main", "header", ".mainline", ".sidebar", ".results"],
    },
  ];
  const engine = ENGINES.find((entry) => entry.match.test(host));
  if (!engine) return;

  // Words a paid result is labelled with, in the languages the engines use most.
  const PAID = /^(sponsored|ad|ads|anzeige|anzeigen|annonce|annonces|publicidad|anuncio|anuncios|annuncio|advertentie|reklam|reklama|广告|廣告|広告|광고|реклама|patrocinado|gesponsert|sponsorisé|sponsorizzato|gesponsord)$/i;

  // Content farms and scraper sites: pages made to rank, not to be read.
  // Copies of Stack Overflow and GitHub, article mills, coupon clones, and
  // "answer" sites that answer nothing.
  const FARMS = [
    "stackoom.com", "codegrepper.com", "programmersought.com", "copyprogramming.com", "iditect.com", "itecnote.com", "githubmemory.com", "gitmemory.com", "gitanswer.com", "giters.com", "githubhelp.com", "coder.social", "issuehunt.io", "lightrun.com", "bleepcoder.com", "fatalerrors.org", "titanwolf.org", "debugah.com", "programmerall.com", "codeleading.com", "cxymm.net", "cxyzjd.com", "codenong.com", "javaer101.com", "ostack.cn", "qastack.cn", "qa-stack.pl", "stackovergo.com", "stackoverrun.com", "coderoad.ru", "question-it.com", "jike.in", "wsxdn.com", "codetd.com", "cdmana.com", "pythonfixing.com", "pythonrepo.com", "askpython.com", "pythonpool.com",
    "ezinearticles.com", "hubpages.com", "buzzle.com", "brighthub.com", "infobarrel.com", "streetarticles.com", "articlecity.com", "selfgrowth.com", "sooperarticles.com", "articlesfactory.com", "amazines.com", "isnare.com", "goarticles.com", "articlebiz.com", "articlesbase.com", "articlealley.com", "articledashboard.com", "helium.com", "examiner.com", "associatedcontent.com", "squidoo.com", "wisegeek.com", "wise-geek.com", "answers.com", "ask.com", "reference.com", "chacha.com", "blurtit.com", "fixya.com", "justanswer.com", "answerbag.com", "askmefast.com", "allexperts.com", "askmehelpdesk.com", "wikianswers.com", "soquestions.com", "askinglot.com", "findanyanswer.com", "treehozz.com", "greedhead.net", "handlebar-online.com", "restaurantnorman.com", "blfilm.com", "sage-answer.com", "sage-advices.com", "sage-tips.com", "short-facts.com", "mysqlpreacher.com", "thehealthyjournal.com", "wisdom-advices.com", "profound-answers.com", "profound-information.com", "profound-tips.com", "easierwithpractice.com", "yourquickadvice.com", "yourquickinfo.com", "great-american-adventures.com", "moviecultists.com", "bookvea.com", "thelastdialogue.org", "erasingdavid.com", "boardgamestips.com", "faq-blog.com", "faq-ans.com", "faq-all.com", "quick-advice.com", "quick-advices.com", "quick-adviser.com", "knowledgeburrow.com", "heimduo.org", "theknowledgeburrow.com", "atheistsforhumanrights.org", "studybuff.com", "howdoiplaythis.com", "ifsql.com", "lastfiascorun.com", "raiseupwa.com", "any-answers.com", "estebantorreshighschool.com", "philosophy-question.com", "answers-to-all.com", "answerstoall.com", "answer-all.com", "all-famous-faqs.com", "famuse.co", "coalitionbrewing.com", "ourgoodbrands.com", "authorscast.com", "juliebutlercreations.com", "thetalentedworld.net", "morethingsjapanese.com", "thecrucibleonscreen.com", "mccnsulting.web.fc2.com",
    "couponxoo.com", "couponupto.com", "couponsdoom.com", "promocodeslab.com", "coupert.com", "dealscove.com", "hotdeals.com", "couponbirds.com", "couponfollow.com", "dealspotr.com", "knoji.com", "tenereteam.com", "couponannie.com", "couponcabin.com", "wethrift.com", "dontpayfull.com", "getcouponsworld.com", "couponsplusdeals.com", "discountscat.com", "promosearcher.com", "shopper.com", "savemoneyfacts.com", "mycoupons.com", "couponchief.com", "couponsandcodes.com", "ultimatecoupons.com",
    "gearrice.com", "gamingdeputy.com", "bollyinside.com", "techviral.net", "techgenyz.com", "technewstube.com", "wsxdn.com", "solveforum.com", "errorsfixing.com", "exceptionshub.com", "fix.code-error.com", "code-examples.net", "newbedev.com", "stackoverflow.fogbugz.com", "tutorialspoint.dev", "w3cschool.cn", "javatpoint.com", "geeksforgeeks.org", "programiz.com", "delftstack.com", "linuxhint.com", "tecadmin.net", "itsfoss.com", "makeuseof.com", "howtogeek.com",
  ];
  // The last line above is the software-tutorial mills; people disagree about
  // them, so they are dimmed as "written for the engine", never hidden.
  const DIMMED_ONLY = new Set(["javatpoint.com", "geeksforgeeks.org", "programiz.com", "delftstack.com", "linuxhint.com", "tecadmin.net", "itsfoss.com", "makeuseof.com", "howtogeek.com", "tutorialspoint.dev", "w3cschool.cn", "answers.com", "ask.com", "reference.com", "justanswer.com", "hubpages.com", "quora.com"]);
  const FARM_SET = new Set(FARMS);
  // A title that reads as a template: "Best 15 X in 2026", "X: Everything You
  // Need to Know", "Top 10 X (Updated)", "X vs Y: Which Is Better?" and so on.
  const TEMPLATE = /(\b(top|best)\s+\d+\b.*\b20\d\d\b)|(\beverything you need to know\b)|(\bultimate guide\b)|(\bupdated\s*(for)?\s*20\d\d\b)|(\b\d+\s+(best|top|amazing|incredible|surprising|mind-blowing|proven)\b)|(\bwhich is better\b)|(\byou won'?t believe\b)|(\bwhat you need to know\b)|(\bthe complete guide\b)|(\ba complete guide\b)|(\bin\s+20\d\d\s*[:(\[-]\s*(complete|full|updated|expert|honest|definitive))/i;

  const STYLE_ID = "noah-shield-search";
  const hidden = new WeakSet();
  const dimmed = new WeakSet();
  let counts = { paid: 0, farms: 0, dimmed: 0 };
  let reported = { paid: 0, farms: 0, dimmed: 0 };
  let state = null;

  function siteOf(hostname) {
    const parts = hostname.toLowerCase().replace(/^www\./, "").split(".");
    if (parts.length <= 2) return parts.join(".");
    const twoPart = /^(co|com|org|net|ac|gov|edu)\.[a-z]{2}$/.test(parts.slice(-2).join("."));
    return parts.slice(twoPart ? -3 : -2).join(".");
  }

  function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = `
      [data-noah-shield="paid"], [data-noah-shield="farm"] { display: none !important; }
      [data-noah-shield="dim"] { opacity: .42 !important; filter: saturate(.6); transition: opacity .2s; }
      [data-noah-shield="dim"]:hover { opacity: 1 !important; filter: none; }
      [data-noah-shield="dim"]::before { content: "written for the search engine · shown faded"; display: block; font: 11px/1.4 system-ui, sans-serif; color: #9aa298; margin: 0 0 2px; }
      #noah-shield-search-note { position: fixed; right: 14px; bottom: 14px; z-index: 2147483000; font: 12px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif; color: var(--noah-text, #d8ddd6); background: var(--noah-raise, rgba(10, 16, 12, .94)); border: 1px solid var(--noah-line, rgba(180, 210, 190, .16)); border-radius: 10px; padding: 7px 11px; box-shadow: 0 6px 24px rgba(0,0,0,.35); cursor: pointer; opacity: 0; transition: opacity .3s ease; }
      #noah-shield-search-note.on { opacity: 1; }
      #noah-shield-search-note b { font-weight: 500; color: var(--noah-accent, #a9cf9f); }
    `;
    (document.head || document.documentElement).append(style);
  }

  function mark(element, kind) {
    if (!element || hidden.has(element) || element.closest("[data-noah-shield]")) return false;
    element.setAttribute("data-noah-shield", kind);
    if (kind === "dim") dimmed.add(element); else hidden.add(element);
    return true;
  }

  function resultBlocks() {
    for (const selector of engine.results) {
      const found = document.querySelectorAll(selector);
      if (found.length) return Array.from(found);
    }
    return [];
  }

  function blockOf(element) {
    for (const selector of engine.results) {
      const block = element.closest(selector);
      if (block) return block;
    }
    return element.closest("li, article, div[data-hveid]");
  }

  function firstLink(block) {
    const anchor = block.querySelector("a[href^='http'] h3, h2 a[href^='http'], h3 a[href^='http'], a[href^='http'][data-testid='result-title-a'], a.result__a, a[href^='http']");
    if (!anchor) return null;
    return anchor.tagName === "A" ? anchor : anchor.closest("a");
  }

  function targetHost(anchor) {
    try {
      const url = new URL(anchor.href, location.href);
      // Some engines wrap the target: the real address rides in a parameter.
      for (const key of ["uddg", "url", "u", "q"]) {
        const inner = url.searchParams.get(key);
        if (inner && /^https?:\/\//.test(inner)) return new URL(inner).hostname;
      }
      if (url.hostname === location.hostname) return "";
      return url.hostname;
    } catch {
      return "";
    }
  }

  function sweep() {
    if (!state || !engine.page()) return;
    const before = { ...counts };
    if (state.clean) {
      for (const selector of engine.ads) {
        for (const element of document.querySelectorAll(selector)) {
          if (mark(element, "paid")) counts.paid++;
        }
      }
      // A "Sponsored" label with nothing else in it marks a paid block.
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT, {
        acceptNode: (node) => (node.childElementCount === 0 && node.textContent.trim().length <= 14 && PAID.test(node.textContent.trim()) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP),
      });
      let label;
      const labels = [];
      while ((label = walker.nextNode())) labels.push(label);
      for (const node of labels) {
        const block = blockOf(node);
        if (block && !block.contains(document.querySelector("form[role='search'], input[name='q']")) && mark(block, "paid")) counts.paid++;
      }
    }
    if (state.farms) {
      for (const block of resultBlocks()) {
        if (block.hasAttribute("data-noah-shield")) continue;
        const anchor = firstLink(block);
        if (!anchor) continue;
        const target = targetHost(anchor);
        if (!target) continue;
        const site = siteOf(target);
        const title = (block.querySelector("h3, h2, [data-testid='result-title-a']") || anchor).textContent.trim();
        if (FARM_SET.has(site) && !DIMMED_ONLY.has(site)) {
          if (mark(block, "farm")) counts.farms++;
        } else if (DIMMED_ONLY.has(site) || TEMPLATE.test(title)) {
          if (mark(block, "dim")) counts.dimmed++;
        }
      }
    }
    if (counts.paid !== before.paid || counts.farms !== before.farms || counts.dimmed !== before.dimmed) {
      showNote();
      report();
    }
  }

  let noteTimer = null;
  function showNote() {
    ensureStyle();
    let note = document.getElementById("noah-shield-search-note");
    if (!note) {
      note = document.createElement("div");
      note.id = "noah-shield-search-note";
      note.title = "noah shield cleaned these results; press to see the log";
      note.addEventListener("click", () => api.runtime.sendMessage({ type: "log.open" }, () => void api.runtime.lastError));
      document.documentElement.append(note);
    }
    const parts = [];
    if (counts.paid) parts.push(`<b>${counts.paid}</b> paid`);
    if (counts.farms) parts.push(`<b>${counts.farms}</b> content-farm`);
    if (counts.dimmed) parts.push(`<b>${counts.dimmed}</b> faded`);
    note.replaceChildren();
    const text = document.createElement("span");
    text.append("noah shield: ");
    parts.forEach((part, index) => {
      if (index) text.append(", ");
      const [count, word] = part.replace(/<\/?b>/g, "|").split("|").filter(Boolean);
      const strong = document.createElement("b");
      strong.textContent = count.trim();
      text.append(strong, " " + word.trim());
    });
    text.append(counts.paid + counts.farms === 1 && !counts.dimmed ? " result removed" : " results");
    note.append(text);
    note.classList.add("on");
    clearTimeout(noteTimer);
    noteTimer = setTimeout(() => note.classList.remove("on"), 6000);
  }

  let reportTimer = null;
  function report() {
    clearTimeout(reportTimer);
    reportTimer = setTimeout(() => {
      const delta = { paid: counts.paid - reported.paid, farms: counts.farms - reported.farms, dimmed: counts.dimmed - reported.dimmed };
      reported = { ...counts };
      if (delta.paid + delta.farms + delta.dimmed <= 0) return;
      api.runtime.sendMessage({ type: "search.hidden", engine: engine.id, query: new URLSearchParams(location.search).get("q") || new URLSearchParams(location.search).get("p") || "", ...delta }, () => void api.runtime.lastError);
    }, 800);
  }

  // ---- the look -----------------------------------------------------------------------------
  function luminanceOf(color) {
    const match = /rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/.exec(color || "");
    if (!match) return null;
    if (match[4] !== undefined && Number(match[4]) === 0) return null;
    const [r, g, b] = [match[1], match[2], match[3]].map(Number);
    return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  }

  function wearLook() {
    if (!state || !state.look || !state.image || !engine.page()) return;
    if (document.getElementById("noah-shield-look")) return;
    const bodyLuminance = luminanceOf(getComputedStyle(document.body).backgroundColor) ?? luminanceOf(getComputedStyle(document.documentElement).backgroundColor) ?? 1;
    const dark = bodyLuminance < 0.45;
    const palette = state.palette || {};
    const veil = dark ? `color-mix(in srgb, ${palette.bg || "#070909"} 84%, transparent)` : "rgba(255, 255, 255, 0.88)";
    const style = document.createElement("style");
    style.id = "noah-shield-look";
    style.textContent = `
      html, body { background: transparent !important; }
      ${engine.transparent.join(", ")} { background: transparent !important; background-color: transparent !important; box-shadow: none !important; }
      #noah-shield-look-layer { position: fixed; inset: 0; z-index: -1; pointer-events: none; background-image: linear-gradient(${veil}, ${veil}), url("${state.image}"); background-size: cover; background-position: center; background-repeat: no-repeat; }
      :root { --noah-text: ${palette.text || "#d8ddd6"}; --noah-raise: ${palette.raise || "#0a100c"}; --noah-line: ${palette.lineMid || "rgba(180,210,190,.16)"}; --noah-accent: ${palette.accentSoft || "#a9cf9f"}; }
    `;
    (document.head || document.documentElement).append(style);
    const layer = document.createElement("div");
    layer.id = "noah-shield-look-layer";
    document.documentElement.append(layer);
  }

  function start() {
    ensureStyle();
    sweep();
    wearLook();
    const observer = new MutationObserver(() => {
      clearTimeout(start.timer);
      start.timer = setTimeout(sweep, 120);
    });
    observer.observe(document.documentElement, { childList: true, subtree: true });
    // Engines that stay on one page while the query changes.
    let lastUrl = location.href;
    setInterval(() => {
      if (location.href === lastUrl) return;
      lastUrl = location.href;
      counts = { paid: 0, farms: 0, dimmed: 0 };
      reported = { paid: 0, farms: 0, dimmed: 0 };
      const old = document.getElementById("noah-shield-look");
      if (old) { old.remove(); const layer = document.getElementById("noah-shield-look-layer"); if (layer) layer.remove(); }
      sweep();
      wearLook();
    }, 700);
  }

  api.runtime.sendMessage({ type: "search.state" }, (response) => {
    void api.runtime.lastError;
    if (!response || response.error || !(response.clean || response.farms || response.look)) return;
    state = response;
    if (document.body) start();
    else document.addEventListener("DOMContentLoaded", start, { once: true });
  });
})();
