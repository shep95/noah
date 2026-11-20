# compiler architecture

```text
usel YAML or JSON (authored content + locale + markets + engines)
   |
   v
strict parser / data contracts / URL & privacy validation
   |
   +--> engine registry (per-service evidence & capabilities)
   +--> geographic registry (country codes + language candidates)
   |
   v
shared semantic renderer
   |
   +--> crawlable static HTML / canonical links / meta robots
   +--> reciprocal hreflang / WebPage structured data
   +--> bounded XML sitemap files and robots.txt
   +--> offline IndexNow planning manifest
   +--> country-by-country localization gap report
   +--> specific engine rules & research gaps report
   |
   v
static output auditor --> deployment-ready static files *after* passing tests
                              |
                              v
                external site publishing (NOT implemented)
                              |
                              v
                 search engine index visibility testing (NOT implemented)
```

source-of-truth SEO semantics stay independent of vendor adapters. each vendor's documented facts and evidence dates stay in the registry, which is version-controlled and avoids unsafe inheritance. unknown capabilities are kept as `null`. future engine adapters should implement explicit code paths and tests, not silently reuse assumptions.

security: parsers validate HTTPS base origins and path slugs; HTML output is escaped, JSON-LD `<` characters are escaped, and private pages are never written to public output. browser service is local-only. IndexNow execution requires an explicit CLI opt-in, matching URLs and domain, an already-hosted key file and network availability. a production deployment still requires dedicated auth, governance, rate limits, moderation, safe resource isolation, protected output paths, and infrastructure tests.

scaling: the alpha renders static pages from a source spec, with sitemap segmentation by count and conservative XML byte size. connecting a large social platform requires stream/incremental render, a job queue, content-deletion propagation, deployment atomics and real-time indexing-health telemetry. these are not yet implemented.
