# seo — noah

`site.usel.json` describes this site for **usel 0.2**, the international SEO
compiler in `usel/` (python 3.11+, standard library only for a JSON spec).
the site already generates its own `robots.txt` and `sitemap.xml`; usel leaves them alone, writes only the IndexNow key file into `public/`, and checks the built site.

## regenerate and check

```bash
cd seo/usel
python -m usel compile ../site.usel.json --output ../../public --reports ../reports
python -m usel check-site ../../public --base-url https://noah.asherin.com
python -m pytest -q        # the compiler's own tests (needs pytest, PyYAML, fastapi, httpx)
```

`seo/reports/` holds the engine-by-engine guide, the 250-market coverage table,
the manifest and the IndexNow plan. they describe the plan, not the site, so
they are kept out of `public/`.

## tell search engines about changes (IndexNow)

Bing, Yandex, Seznam, Naver and the other IndexNow participants accept change
notifications. after a deploy, once `https://noah.asherin.com/<key>.txt` is live:

```bash
cd seo/usel
python -m usel submit-indexnow --sitemap ../../public/sitemap.xml --key <indexnow_key from site.usel.json> --host https://noah.asherin.com           # dry run
python -m usel submit-indexnow --sitemap ../../public/sitemap.xml --key <indexnow_key from site.usel.json> --host https://noah.asherin.com --execute
```

nothing is ever sent without `--execute`. Google does not take IndexNow; it reads
the sitemap named in `robots.txt`.

## what it does not do

it does not guarantee indexing or rankings, translate content, or contact any
search engine unless you run `submit-indexnow --execute`. see `usel/README.md`.
