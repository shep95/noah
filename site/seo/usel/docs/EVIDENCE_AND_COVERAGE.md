# usel research evidence & global gap register

snapshot date: 2026-11-14. each engine is evaluated on its own terms. a search engine's use of Bing/Google results is **not** proof that every element of its SEO rules, local UI, rankings, or content eligibility is identical. a search engine can share an index while implementing its own snippets, ranking reranking, verticals, safe-search, or freshness policies.

## source-reviewed differences (not exhaustive ranking disclosures)

| engine | documented observations | source |
|---|---|---|
| Google | localized pages can use hreflang in HTML, HTTP header or sitemap; noindex requires crawling access. | https://developers.google.com/search/docs/specialty/international/localized-versions ; https://developers.google.com/search/docs/crawling-indexing/robots-meta-tag |
| Bing | strongly recommends IndexNow for fresh URLs; other webmaster submissions remain supported. | https://www.bing.com/webmasters/help/?topicid=62f2860b |
| Yandex | internal links and sitemaps help discovery; regionalized sites must be available to bots without dependence on geographic IP. | https://yandex.com/support/webmaster/en/yandex-indexing/site-indexing ; https://yandex.com/support/webmaster/en/site-geography/site-region |
| Naver | Search Advisor documents robots and sitemap; it also supports IndexNow. | https://searchadvisor.naver.com/guide/seo-basic-robots ; https://searchadvisor.naver.com/guide/indexnow-about |
| Seznam | indexnow metadata directly lists Seznam; manual URL add tools exist. | https://www.indexnow.org/searchengines.json ; https://search.seznam.cz/wt/pridej-stranku |
| Baidu | its resource platform documents manual, API and sitemap submissions, with eligibility/quota constraints. | https://ziyuan.baidu.com/college/articleinfo?id=3076 ; https://help.baidu.com/question?class=0&id=3032&prod_id=99 |
| Sogou | XML sitemaps recognized, but webmaster platform says sitemap submissions may be invitation-only. | https://help.sogou.com/sitemap.html ; https://zhanzhang.sogou.com/index.php/help/sitemap |
| 360 Search | official 360 forum instructions point to verified sitemap submissions and URL inclusion requests. older guide; recheck. | https://bbs.360.cn/thread-15979727-1-1.html |
| Brave Search | crawler has independent policies and avoids a distinctive UA; Googlebot crawlability is relevant. | https://search.brave.com/help/brave-search-crawler |
| Mojeek | bot publishes robots policy; a 2022 staff statement says it did not use sitemaps for URL discovery at that point. *stale source; reverify*. | https://www.mojeek.com/bot.html ; https://community.mojeek.com/t/crawling-and-webmaster/215 |
| DuckDuckGo | many traditional search links rely on Bing, while other parts use its crawler and other content sources. | https://safe.duckduckgo.com/duckduckgo-help-pages/results/sources |
| Ecosia | providers can include Bing, Google and EUSP based on market, device, cookies and consent settings; sources can change. | https://support.ecosia.org/article/579-search-results-providers |
| Yep | listed as an IndexNow participant, but broader SEO rules not independently reviewed. | https://www.indexnow.org/searchengines.json |
| Internet Archive crawler | IndexNow participant; do not mistake it for a separate conventional search-results engine. | https://www.indexnow.org/searchengines.json |
| Amazonbot | IndexNow participant crawler; do not mistake it for a general search engine. | https://www.indexnow.org/searchengines.json |

## completeness is an ongoing discovery problem

there is no authoritative, permanent single list of **every** search engine, metasearch front end, language-specific search provider, internal vertical-search platform, portal or directory. regional availability may change with domain transfers, geoblocking or product shutdowns. each country can have zero domestic full-web engines or many specialized search services. country of origin and country of actual users are different relationships. local-language source research is required for each.

- international directory *gaiaseek* reported **814** search/portal/directory entries from **132** countries and territories checked in august 2026. it separately recorded **139 hijacked or gone sites**. it is a secondary discovery lead, not 814 independently documented active full-web search engines: https://gaiaseek.com/en/ and https://gaiaseek.com/atlas/
- international directory Search Engine Colossus separately claims more than 2,100 listings across 198 countries and 60 territories. this count includes search resources beyond independently crawling general search engines and should not be accepted without checking: https://www.searchenginecolossus.com/
- the authoritative live IndexNow participants discovery is https://www.indexnow.org/searchengines.json — its participant list changes over time and may contain crawlers not exposed as public search engines.

## per-engine research acceptance process

1. confirm service identity, original/canonical domain and whether product currently operates in the target jurisdiction. flag shut down, recycled, captive-portal or bot-blocked sites separately.
2. document country of origin, languages and scripts **separately** from verified markets and traffic distribution. no country coverage is inferred merely from hosting location.
3. identify own crawler/index, metasearch inputs, hybrid ranking layers, vertical specialization and submission requirements. keep unknown properties `null`.
4. locate the operator's original local-language webmaster rules: robots behavior, JS rendering, URL discovery, IndexNow, sitemap, structured data, locale targeting, titles, canonical signals, product-specific requirements, content policy and prohibitions.
5. record source URL, language, retrieval date, publication date (if known), source class, exact applicability and any contradictory or obsolete guidance.
6. implement an adapter **only** for confirmed rules and test generated output against that documented contract. never add imaginary one-click indexing endpoints.
7. after publishing real site content, verify crawling/indexing through supported webmaster tools where accessible, then compare clickthrough, signups and retention by market. a valid sitemap does not prove successful indexing.
8. retest when engine policy updates, provider agreements change, a country changes its legal requirements, or the original evidence becomes stale.

## operational restrictions

- SEO is permitted optimization, not a bypass for content laws, access controls or spam restrictions.
- do not generate scraped/private content as public pages, cloak bots with unrelated content, keyword-stuff translations, or claim guaranteed ranking.
- China, Russia, South Korea and other jurisdictions may impose their own operational and platform rules. local legal counsel and country experts must evaluate deployment; this code does not certify legal compliance.
- one universal language can centralize intent and compilation, but cannot erase the need for real local-language content and evidence-based search-engine customization.
