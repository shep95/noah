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
    addPeekButtons();
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
    const query = new URLSearchParams(location.search).get("q") || new URLSearchParams(location.search).get("p") || "";
    if (looksLikeAName(query)) {
      const footprint = document.createElement("a");
      footprint.href = api.runtime.getURL("footprint.html?name=" + encodeURIComponent(query.replace(/^"|"$/g, "")));
      footprint.target = "_blank";
      footprint.rel = "noopener";
      footprint.textContent = "your footprint?";
      footprint.style.cssText = "margin-left:10px;color:inherit;text-decoration:underline;text-underline-offset:3px;";
      note.append(footprint);
    }
    note.classList.add("on");
    clearTimeout(noteTimer);
    noteTimer = setTimeout(() => note.classList.remove("on"), 6000);
  }

  // Two to four capitalised words, or a quoted phrase, with no operators:
  // what someone types when they look a person up.
  function looksLikeAName(query) {
    const trimmed = query.trim();
    if (!trimmed || /[:@/.]/.test(trimmed)) return false;
    if (/^".+"$/.test(trimmed)) return true;
    const words = trimmed.split(/\s+/);
    return words.length >= 2 && words.length <= 4 && words.every((word) => /^\p{Lu}[\p{L}'’-]+$/u.test(word));
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
  // The whole results page takes the picture's palette, the shield's type and
  // its spacing: cards with the same radius and hairline, the same rhythm
  // between them, one accent where a hand goes. Whatever mode the engine was
  // in, the words are the palette's, so they always read.
  const THEME = {
    google: {
      cards: "#rso > div:has(h3), #rso .MjjYud:has(h3) > div:not(:has(.MjjYud)), .related-question-pair, #rhs .kp-blk, #rhs > div:has(h2), #botstuff > div:has(h3)",
      cardsInner: "#rso .MjjYud > div > div.g, #rso .g",
      title: "#rso h3, #rhs h2, #rhs h3, #botstuff h3",
      snippet: "#rso [data-sncf], #rso .VwiC3b, #rso div[style*='line-clamp'], #rso .ITZIwc, #rhs .kno-rdesc span, #rso span.hgKElc",
      url: "#rso cite, #rso .VuuXrf, #rso .byrV5b, #rhs cite",
      searchBox: "#searchform form, #tsf, .RNNXgb, form[role='search'] > div, .SDkEP",
      searchInput: "textarea[name='q'], input[name='q']",
      tabs: "#hdtb a, #top_nav a, .crJ18e a, #hdtb div[role='listitem'] a, .T47uwc a",
      tabSelected: "#hdtb .hdtb-msel, #hdtb a[aria-current], #hdtb [selected] a, .Ww4FFb[aria-selected='true']",
      chips: "#rso a[role='link'].fl, #rso .Xk52Ge a, #rso .zVvuGd a, #rso a.MXl0lf, .k8XOCe",
      quiet: "#rso .g .MUxGbd:not(.VwiC3b), #rso .fG8Fp, #rso .LEwnzc, #rhs .rhstc4, #botstuff td",
    },
    bing: { cards: "#b_results > li.b_algo, #b_results > li.b_ans, #b_context .b_ans", title: "#b_results h2, #b_context h2", snippet: "#b_results .b_caption p, #b_results .b_lineclamp2, #b_results .b_lineclamp3", url: "#b_results cite, #b_results .b_attribution", searchBox: "#sb_form, .b_searchboxForm", searchInput: "#sb_form_q", tabs: "#b_header .b_scopebar a", tabSelected: "#b_header .b_scopebar .b_active a", chips: "#b_results .b_rs a", quiet: "#b_results .b_factrow, #b_results .b_vlist2col" },
    duckduckgo: { cards: "article[data-testid='result'], li[data-layout='organic'] > article, section[data-testid='sidebar'] > div", title: "article h2, article h2 a span", snippet: "article [data-result='snippet'], article div[data-result='snippet'] span", url: "article a[data-testid='result-extras-url-link'], article p[data-testid='result-extras-url']", searchBox: "form[role='search'], #search_form, .search--header, div[data-testid='search-box-container']", searchInput: "#search_form_input, input[name='q']", tabs: "ul[data-testid='zci-tabs'] a, .zcm__link", tabSelected: ".zcm__link.is-active, ul[data-testid='zci-tabs'] a[aria-current]", chips: "article .related-searches a", quiet: "article footer" },
    brave: { cards: "#results > .snippet, #side-right > div", title: ".snippet .title, .snippet-title, #side-right h2", snippet: ".snippet .snippet-description, .snippet-content", url: ".snippet .netloc, .snippet cite, .snippet .site-name", searchBox: "#searchform, .searchbox, form#searchform > div", searchInput: "#searchbox, input[name='q']", tabs: ".tabs a, #tabs a, nav a.tab", tabSelected: ".tabs a.active, nav a.tab[aria-current]", chips: ".related a", quiet: ".snippet .footer" },
    yahoo: { cards: "#web ol > li, #right .dd", title: "#web h3, #right h3", snippet: "#web .compText p, #web .compText", url: "#web .compTitle cite, #web span.fz-ms", searchBox: "#sbq-wrap, .sbq-w, form[role='search']", searchInput: "#yschsp, input[name='p']", tabs: "#horizontal-bar a", tabSelected: "#horizontal-bar .active a", chips: "#web .compDlink a", quiet: "#web .compAttribution" },
    startpage: { cards: ".w-gl__result, .result, [class*='w-gl__result']", title: ".w-gl__result-title, .result-title, h2", snippet: ".w-gl__description, .result-description, p.description", url: ".w-gl__result-url, .result-link, a.link", searchBox: "form.search-form, .search-form", searchInput: "#q, input[name='query']", tabs: ".nav-bar a, .layout-web__header a", tabSelected: ".nav-bar a.active", chips: "", quiet: "" },
    ecosia: { cards: "[data-test-id='mainline-result'], .result, article", title: "[data-test-id='result-title'], .result-title, article h2", snippet: "[data-test-id='result-description'], .result-snippet, article p", url: "[data-test-id='result-url'], .result-url, article cite", searchBox: "form[role='search'], .search-form", searchInput: "input[name='q']", tabs: ".search-nav a, nav a", tabSelected: ".search-nav a[aria-current], nav a.active", chips: "", quiet: "" },
  };

  function themeCss(palette, image) {
    const theme = THEME[engine.id] || THEME.startpage;
    const p = {
      bg: palette.bg || "#070909", raise: palette.raise || "#0a100c", surface: palette.surface || "rgba(8, 11, 9, 0.92)", surface2: palette.surface2 || "rgba(10, 14, 11, 0.9)",
      line: palette.line || "rgba(180, 210, 190, 0.08)", lineMid: palette.lineMid || "rgba(180, 210, 190, 0.14)", text: palette.text || "#d8ddd6", sub: palette.sub || "#b9beb7",
      muted: palette.muted || "#9aa298", faint: palette.faint || "#6f766e", accent: palette.accent || "#5f8a58", accentBright: palette.accentBright || "#72a868", accentSoft: palette.accentSoft || "#a9cf9f", bright: palette.bright || "#f1f4ef",
    };
    const rule = (selector, body) => (selector ? `${selector} { ${body} }\n` : "");
    return `
      :root { --noah-bg: ${p.bg}; --noah-raise: ${p.raise}; --noah-surface: ${p.surface}; --noah-surface2: ${p.surface2}; --noah-line: ${p.line}; --noah-line-mid: ${p.lineMid}; --noah-text: ${p.text}; --noah-sub: ${p.sub}; --noah-muted: ${p.muted}; --noah-faint: ${p.faint}; --noah-accent: ${p.accent}; --noah-accent-bright: ${p.accentBright}; --noah-accent-soft: ${p.accentSoft}; --noah-bright: ${p.bright}; --noah-display: Georgia, "Times New Roman", serif; --noah-body: -apple-system, "Segoe UI", system-ui, sans-serif; --noah-ease: cubic-bezier(0.2, 0.7, 0.2, 1); }
      html, body { background: transparent !important; color: var(--noah-text) !important; font-family: var(--noah-body) !important; color-scheme: dark; }
      ${engine.transparent.join(", ")} { background: transparent !important; background-color: transparent !important; box-shadow: none !important; border-color: var(--noah-line) !important; }
      #noah-shield-look-layer { position: fixed; inset: 0; z-index: -1; pointer-events: none; background-image: linear-gradient(color-mix(in srgb, ${p.bg} 82%, transparent), color-mix(in srgb, ${p.bg} 88%, transparent)), url("${image}"); background-size: cover; background-position: center; background-repeat: no-repeat; }
      ${rule(theme.cards, "background: var(--noah-surface) !important; border: 1px solid var(--noah-line) !important; border-radius: 14px !important; padding: 16px 18px !important; margin: 0 0 12px !important; box-shadow: none !important; transition: border-color .16s var(--noah-ease), transform .26s var(--noah-ease);")}
      ${rule(theme.cards ? theme.cards.split(",").map((part) => part.trim() + ":hover").join(", ") : "", "border-color: var(--noah-line-mid) !important;")}
      ${rule(theme.cardsInner, "background: transparent !important; box-shadow: none !important; padding: 0 !important; margin: 0 !important; border: 0 !important;")}
      ${rule(theme.title, "font-family: var(--noah-display) !important; font-weight: 400 !important; font-size: 21px !important; line-height: 1.25 !important; letter-spacing: .005em; color: var(--noah-bright) !important; margin: 2px 0 6px !important;")}
      ${rule(theme.title ? theme.title.split(",").map((part) => "a:hover " + part.trim().replace(/^#\w+ /, "")).join(", ") : "", "color: var(--noah-accent-soft) !important;")}
      ${rule(theme.snippet, "font-family: var(--noah-body) !important; font-size: 14px !important; line-height: 1.6 !important; color: var(--noah-sub) !important;")}
      ${rule(theme.url, "font-family: var(--noah-body) !important; font-size: 12px !important; color: var(--noah-muted) !important; letter-spacing: .01em;")}
      ${rule(theme.searchBox, "background: var(--noah-raise) !important; border: 1px solid var(--noah-line-mid) !important; border-radius: 999px !important; box-shadow: none !important;")}
      ${rule(theme.searchInput, "color: var(--noah-bright) !important; font-family: var(--noah-body) !important; font-size: 16px !important; background: transparent !important; caret-color: var(--noah-accent-soft);")}
      ${rule(theme.tabs, "color: var(--noah-muted) !important; font-family: var(--noah-body) !important; font-size: 13px !important; letter-spacing: .02em; text-decoration: none !important; border-bottom-color: transparent !important; transition: color .16s var(--noah-ease);")}
      ${rule(theme.tabs ? theme.tabs.split(",").map((part) => part.trim() + ":hover").join(", ") : "", "color: var(--noah-text) !important;")}
      ${rule(theme.tabSelected, "color: var(--noah-bright) !important; border-bottom: 2px solid var(--noah-accent-bright) !important;")}
      ${rule(theme.chips, "background: transparent !important; border: 1px solid var(--noah-line-mid) !important; border-radius: 999px !important; color: var(--noah-sub) !important; font-family: var(--noah-body) !important; font-size: 12px !important;")}
      ${rule(theme.quiet, "color: var(--noah-muted) !important;")}
      #rso, #rhs, #botstuff, #b_results, #b_context, #web, #right, #results, main, article { color: var(--noah-text) !important; }
      #rso span, #rso div, #rso em, #rso b, #rso td, #rhs span, #rhs div, #rhs td, #botstuff span, #botstuff div, #b_results span, #b_results div, #b_results p, #b_context span, #b_context div, #web span, #web div, #right span, #right div, #results span, #results div, article span, article p, article div { color: inherit; }
      #rso a, #rhs a, #botstuff a, #b_results a, #b_context a, #web a, #right a, #results a, article a { color: var(--noah-accent-soft); }
      #rso em, #b_results strong, article b { color: var(--noah-bright) !important; font-style: normal; font-weight: 500; }
      #rso img, #rhs img, #b_results img, article img { border-radius: 10px; }
      #rso svg, #rhs svg, #hdtb svg, #searchform svg { fill: var(--noah-muted); color: var(--noah-muted); }
      ::selection { background: color-mix(in srgb, var(--noah-accent) 45%, transparent); }
      #noah-shield-search-note { font-family: var(--noah-body) !important; }
      /* peek */
      .noah-peek-button { display: inline-flex; align-items: center; gap: 6px; margin: 6px 0 0; padding: 3px 10px; border-radius: 999px; border: 1px solid var(--noah-line-mid); background: transparent; color: var(--noah-muted); font: 12px var(--noah-body); cursor: pointer; transition: color .16s var(--noah-ease), border-color .16s var(--noah-ease); }
      .noah-peek-button:hover { color: var(--noah-bright); border-color: var(--noah-accent); }
      .noah-peek-button.on { color: var(--noah-accent-soft); border-color: var(--noah-accent); }
      .noah-peek { margin: 12px 0 2px; padding: 14px 16px; border-radius: 12px; background: var(--noah-surface2); border: 1px solid var(--noah-line); font: 14px/1.6 var(--noah-body); color: var(--noah-text); animation: noah-arrive .26s var(--noah-ease) both; }
      @keyframes noah-arrive { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
      .noah-peek header { display: flex; gap: 12px; align-items: baseline; margin: 0 0 8px; font: 12px var(--noah-body); color: var(--noah-muted); }
      .noah-peek header b { font: 400 16px var(--noah-display); color: var(--noah-bright); }
      .noah-peek .noah-peek-lead { color: var(--noah-sub); margin: 0 0 10px; }
      .noah-peek .noah-peek-body { display: grid; grid-template-columns: 1fr auto; gap: 16px; }
      .noah-peek .noah-peek-text { max-height: 340px; overflow: auto; padding-right: 8px; scrollbar-gutter: stable; }
      .noah-peek .noah-peek-text p { margin: 0 0 10px; }
      .noah-peek .noah-peek-text h4 { margin: 12px 0 4px; font: 400 15px var(--noah-display); color: var(--noah-bright); }
      .noah-peek img { max-width: 220px; max-height: 160px; object-fit: cover; border-radius: 10px; border: 1px solid var(--noah-line); }
      .noah-peek .noah-peek-foot { margin: 10px 0 0; font-size: 12px; color: var(--noah-faint); }
      .noah-peek .noah-peek-foot a { color: var(--noah-accent-soft); }
      .noah-peek .noah-peek-error { color: var(--noah-muted); }
      @media (prefers-reduced-motion: reduce) { .noah-peek { animation: none; } }
    `;
  }

  function wearLook() {
    if (!state || !state.look || !state.image || !engine.page()) return;
    if (document.getElementById("noah-shield-look")) return;
    const style = document.createElement("style");
    style.id = "noah-shield-look";
    style.textContent = themeCss(state.palette || {}, state.image);
    (document.head || document.documentElement).append(style);
    const layer = document.createElement("div");
    layer.id = "noah-shield-look-layer";
    document.documentElement.append(layer);
  }

  // ---- peek: the page's own words, here, without the visit --------------------------------------
  const peeked = new WeakSet();
  function addPeekButtons() {
    if (!state || !state.peek || !engine.page()) return;
    for (const block of resultBlocks()) {
      if (peeked.has(block) || block.hasAttribute("data-noah-shield") && block.getAttribute("data-noah-shield") !== "dim") continue;
      const anchor = firstLink(block);
      if (!anchor) continue;
      const target = targetHost(anchor);
      if (!target || !/^https?:$/.test(new URL(anchor.href, location.href).protocol)) continue;
      peeked.add(block);
      const button = document.createElement("button");
      button.type = "button";
      button.className = "noah-peek-button";
      button.textContent = "peek";
      button.title = "Read what this page says, here, without opening it";
      const titleNode = block.querySelector("h3, h2, [data-testid='result-title-a']");
      const parent = titleNode ? titleNode.closest("a") || titleNode : anchor;
      parent.parentElement ? parent.parentElement.insertBefore(button, parent.nextSibling) : block.append(button);
      button.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        togglePeek(block, button, realHref(anchor));
      });
    }
  }

  function realHref(anchor) {
    try {
      const url = new URL(anchor.href, location.href);
      for (const key of ["uddg", "url", "u", "q"]) {
        const inner = url.searchParams.get(key);
        if (inner && /^https?:\/\//.test(inner)) return inner;
      }
      return url.href;
    } catch {
      return anchor.href;
    }
  }

  function textOf(html, url) {
    const doc = new DOMParser().parseFromString(html, "text/html");
    for (const junk of doc.querySelectorAll("script, style, noscript, template, svg, iframe, nav, footer, aside, header, form, [role='navigation'], [role='banner'], [role='contentinfo'], [aria-hidden='true'], .cookie, .cookies, .newsletter, .sidebar, .advert, .ad")) junk.remove();
    const meta = (name) => { const node = doc.querySelector(`meta[property='${name}'], meta[name='${name}']`); return node ? node.getAttribute("content") || "" : ""; };
    const title = (meta("og:title") || (doc.querySelector("title") || {}).textContent || "").trim().slice(0, 160);
    const description = (meta("description") || meta("og:description") || "").trim().slice(0, 400);
    let image = meta("og:image") || meta("twitter:image") || "";
    try { if (image) image = new URL(image, url).href; } catch { image = ""; }
    if (!/^https:/.test(image)) image = "";
    const roots = [doc.querySelector("article"), doc.querySelector("main"), doc.querySelector("[role='main']"), doc.body].filter(Boolean);
    let best = roots[0];
    let bestScore = -1;
    for (const root of roots) {
      const score = Array.from(root.querySelectorAll("p")).reduce((sum, paragraph) => sum + (paragraph.textContent.trim().length > 60 ? paragraph.textContent.trim().length : 0), 0);
      if (score > bestScore) { best = root; bestScore = score; }
    }
    const pieces = [];
    let total = 0;
    for (const node of best.querySelectorAll("h1, h2, h3, p, li")) {
      const text = node.textContent.replace(/\s+/g, " ").trim();
      if (!text) continue;
      const heading = /^H[1-3]$/.test(node.tagName);
      // The page's own heading repeats the title line above the text.
      if (heading && title && text.toLowerCase() === title.toLowerCase()) continue;
      if (!heading && text.length < 50) continue;
      if (node.tagName === "LI" && text.length < 80) continue;
      pieces.push({ heading, text: text.slice(0, 700) });
      total += text.length;
      if (total > 6000 || pieces.length > 40) break;
    }
    const words = Math.round(best.textContent.split(/\s+/).filter(Boolean).length);
    return { title, description, image, pieces, words };
  }

  async function togglePeek(block, button, url) {
    const existing = block.querySelector(".noah-peek");
    if (existing) { existing.remove(); button.classList.remove("on"); return; }
    button.classList.add("on");
    const panel = document.createElement("div");
    panel.className = "noah-peek";
    panel.textContent = "reading…";
    const at = button.closest("a") ? button.closest("a").parentElement : button.parentElement;
    (at && at !== block ? at : block).append(panel);
    const answer = await new Promise((resolve) => api.runtime.sendMessage({ type: "search.peek", url }, (response) => { void api.runtime.lastError; resolve(response || { error: "no answer" }); }));
    panel.replaceChildren();
    if (answer.error) {
      const error = document.createElement("div");
      error.className = "noah-peek-error";
      error.textContent = "Could not read it from here: " + answer.error;
      panel.append(error);
      return;
    }
    const page = textOf(answer.html, answer.url || url);
    const head = document.createElement("header");
    const strong = document.createElement("b");
    strong.textContent = page.title || new URL(url).hostname;
    head.append(strong, document.createTextNode(`${new URL(answer.url || url).hostname} · about ${Math.max(1, Math.round(page.words / 230))} min to read`));
    panel.append(head);
    if (page.description) {
      const lead = document.createElement("p");
      lead.className = "noah-peek-lead";
      lead.textContent = page.description;
      panel.append(lead);
    }
    const body = document.createElement("div");
    body.className = "noah-peek-body";
    const text = document.createElement("div");
    text.className = "noah-peek-text";
    for (const piece of page.pieces) {
      const node = document.createElement(piece.heading ? "h4" : "p");
      node.textContent = piece.text;
      text.append(node);
    }
    if (!page.pieces.length) {
      const none = document.createElement("p");
      none.className = "noah-peek-error";
      none.textContent = "This page carries its words in scripts, so there is nothing to read without opening it.";
      text.append(none);
    }
    body.append(text);
    if (page.image) {
      const image = document.createElement("img");
      image.src = page.image;
      image.alt = "";
      image.referrerPolicy = "no-referrer";
      image.loading = "lazy";
      body.append(image);
    }
    panel.append(body);
    const foot = document.createElement("div");
    foot.className = "noah-peek-foot";
    const open = document.createElement("a");
    open.href = url;
    open.textContent = "open the page";
    open.rel = "noopener noreferrer";
    foot.append("Fetched once, without cookies or your address in a referrer. ", open);
    panel.append(foot);
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
    if (!response || response.error || !(response.clean || response.farms || response.look || response.peek)) return;
    state = response;
    if (document.body) start();
    else document.addEventListener("DOMContentLoaded", start, { once: true });
  });
})();
