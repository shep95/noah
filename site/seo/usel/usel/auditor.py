"""Static output audits. They cannot certify search-engine indexing or ranking.

``audit_site`` checks what ``usel compile`` wrote. ``check_site`` checks any
built website folder (a Vite ``dist``, a Next ``out``, a plain ``public``):
sitemap URLs against robots rules, canonical/title/description/lang on the
HTML that exists, noindex pages listed in a sitemap, and AI-crawler rules.
"""
from __future__ import annotations
from html.parser import HTMLParser
from pathlib import Path
from xml.etree import ElementTree as ET
from urllib.parse import urlsplit, unquote
import json
import re

NS = '{http://www.sitemaps.org/schemas/sitemap/0.9}'


class PageInspector(HTMLParser):
    def __init__(self):
        super().__init__()
        self.canonical = []
        self.alternates = {}
        self.robots = []
        self.titles = 0
        self.description = []
        self.lang = None

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        rel = (a.get('rel') or '').lower().split()
        if tag == 'html':
            self.lang = a.get('lang')
        if tag == 'title':
            self.titles += 1
        if tag == 'link' and 'canonical' in rel:
            self.canonical.append(a.get('href'))
        if tag == 'link' and 'alternate' in rel and a.get('hreflang'):
            self.alternates[a['hreflang']] = a.get('href')
        if tag == 'meta' and (a.get('name') or '').lower() == 'robots':
            self.robots.append(a.get('content', ''))
        if tag == 'meta' and (a.get('name') or '').lower() == 'description':
            self.description.append(a.get('content', ''))


# ---------- robots.txt (RFC 9309 matching: longest rule wins, Allow wins ties) ----------

def parse_robots(text):
    """Return {agent_lowercase: [(allow: bool, pattern)]} plus the Sitemap lines."""
    groups, sitemaps = {}, []
    agents, in_rules = [], False
    for raw in text.splitlines():
        line = raw.split('#', 1)[0].strip()
        if not line or ':' not in line:
            continue
        field, value = (s.strip() for s in line.split(':', 1))
        field = field.lower()
        if field == 'user-agent':
            if in_rules:
                agents, in_rules = [], False
            agents.append(value.lower())
            for agent in agents:
                groups.setdefault(agent, [])
        elif field in ('allow', 'disallow'):
            in_rules = True
            if value:
                for agent in agents:
                    groups.setdefault(agent, []).append((field == 'allow', value))
        elif field == 'sitemap':
            sitemaps.append(value)
    return groups, sitemaps


def _rule_matches(pattern, path):
    regex = re.escape(pattern).replace(r'\*', '.*')
    if regex.endswith(r'\$'):
        regex = regex[:-2] + '$'
    return re.match(regex, path) is not None


def robots_allows(groups, agent, path):
    rules = groups.get(agent.lower())
    if rules is None:
        rules = groups.get('*', [])
    best = None
    for allow, pattern in rules:
        if _rule_matches(pattern, path):
            key = (len(pattern), allow)
            if best is None or key > best:
                best = key
    return True if best is None else best[1]


def _sitemap_urls(root, base, errors):
    """Every <loc> in sitemap.xml and the sitemaps an index points to."""
    urls = []
    try:
        tree = ET.parse(root / 'sitemap.xml')
    except Exception as exc:
        errors.append(f'invalid sitemap XML: {exc}')
        return urls
    files = []
    if tree.getroot().tag == NS + 'sitemapindex':
        for node in tree.getroot().findall(NS + 'sitemap'):
            link = node.findtext(NS + 'loc') or ''
            if not link.startswith(base + '/'):
                errors.append(f'sitemap index points off-site: {link}')
                continue
            files.append(root / link.rsplit('/', 1)[-1])
    else:
        files = [root / 'sitemap.xml']
    for f in files:
        try:
            t = ET.parse(f)
        except Exception as exc:
            errors.append(f'invalid sitemap XML {f.name}: {exc}')
            continue
        for item in t.getroot().findall(NS + 'url'):
            urls.append((item.findtext(NS + 'loc') or '').strip())
    return urls


def _inspect(path):
    parser = PageInspector()
    parser.feed(path.read_text(encoding='utf-8', errors='replace'))
    return parser


def audit_site(root, reports=None):
    root = Path(root)
    report_dir = Path(reports) if reports is not None else root
    errors, warnings = [], []
    if not root.is_dir():
        return {'ok': False, 'errors': ['output directory absent'], 'warnings': []}
    try:
        manifest = json.loads((report_dir / 'manifest.json').read_text(encoding='utf-8'))
        base = manifest['base_url'].rstrip('/')
    except Exception as exc:
        return {'ok': False, 'errors': [f'invalid manifest: {exc}'], 'warnings': []}
    routes = set(manifest.get('route_urls', []))
    pages = {}
    # Only the pages USEL wrote: the folder may be a site's public/ with its own html.
    for rendered_url in manifest.get('rendered_urls', []):
        relative = unquote(urlsplit(rendered_url).path).strip('/') + '/index.html'
        path = root / relative
        if not path.is_file():
            errors.append(f'{relative}: listed in the manifest but missing')
            continue
        try:
            parser = _inspect(path)
        except Exception as exc:
            errors.append(f'{path}: invalid html: {exc}')
            continue
        url = rendered_url
        pages[url] = parser
        if len(parser.canonical) != 1 or parser.canonical[0] != url:
            errors.append(f'{relative}: missing/mismatched self canonical')
        if len(parser.description) != 1:
            errors.append(f'{relative}: missing description')
        if not parser.lang:
            errors.append(f'{relative}: missing document language')
        if parser.titles != 1:
            errors.append(f'{relative}: expected one title')
        if not parser.robots:
            errors.append(f'{relative}: robots meta missing')
        if 'noindex' in ','.join(parser.robots) and parser.alternates:
            warnings.append(f'{relative}: noindex has alternates')
    for url, parser in pages.items():
        if not parser.alternates:
            continue
        if parser.lang not in parser.alternates or parser.alternates[parser.lang] != url:
            errors.append(f'{url}: no reciprocal self hreflang')
        for locale, other_url in parser.alternates.items():
            if locale == 'x-default':
                continue
            if other_url in routes:
                continue  # served by the app; its tags are the app's to emit
            other = pages.get(other_url)
            if other is None or other.alternates.get(parser.lang) != url:
                errors.append(f'{url}: {locale} does not reciprocate {other_url}')
    urls = _sitemap_urls(root, base, errors) if manifest.get('sitemap_written', True) else []
    seen = set()
    for loc in urls:
        if loc in seen:
            errors.append(f'duplicate sitemap URL {loc}')
        seen.add(loc)
        if loc in routes:
            continue
        if loc not in pages:
            errors.append(f'sitemap URL missing generated HTML: {loc}')
        elif 'noindex' in ','.join(pages[loc].robots):
            errors.append(f'noindex URL in sitemap: {loc}')
    if manifest.get('sitemap_written', True) and len(seen) != manifest['indexed_pages']:
        errors.append('indexed pages count differs from sitemap')
    if manifest.get('robots_managed', True) and not manifest.get('allow_search', True):
        robots_file = root / 'robots.txt'
        groups, _ = parse_robots(robots_file.read_text(encoding='utf-8')) if robots_file.is_file() else ({}, [])
        if not robots_file.is_file() or robots_allows(groups, 'Googlebot', '/'):
            errors.append('robots.txt should keep every crawler out')
    elif manifest.get('robots_managed', True):
        try:
            robots_text = (root / 'robots.txt').read_text(encoding='utf-8')
            groups, maps = parse_robots(robots_text)
            if manifest.get('sitemap_written', True) and base + '/sitemap.xml' not in maps:
                errors.append('robots sitemap URL missing')
            for loc in seen:
                if not robots_allows(groups, 'Googlebot', unquote(urlsplit(loc).path) or '/'):
                    errors.append(f'robots.txt blocks a sitemap URL: {loc}')
        except FileNotFoundError:
            errors.append('robots.txt unavailable')
    if manifest['indexed_pages'] == 0 and manifest.get('allow_search', True):
        warnings.append('there are no indexable pages; expected if everything is noindex')
    return {'ok': not errors, 'errors': errors, 'warnings': warnings, 'rendered_files': len(pages),
            'indexed_pages': manifest['indexed_pages'],
            'tested_scope': 'generated HTML, sitemap, robots and reciprocal alternates only'}


AI_AGENTS_TO_CHECK = ('GPTBot', 'CCBot', 'ClaudeBot', 'Google-Extended', 'Bytespider')
SEARCH_AGENTS_TO_CHECK = ('Googlebot', 'Bingbot', 'YandexBot', 'Baiduspider', 'Yeti', 'SeznamBot', 'Applebot', 'DuckDuckBot')


def _html_for(root, path):
    clean = unquote(path).strip('/')
    candidates = [root / clean / 'index.html', root / f'{clean}.html'] if clean else [root / 'index.html']
    for c in candidates:
        if c.is_file() and c.resolve().is_relative_to(root.resolve()):
            return c
    return None


def check_site(root, base_url, expect_ai_blocked=True, expect_indexable=True):
    """Audit an existing built site folder against search-engine basics."""
    root = Path(root)
    base = base_url.rstrip('/')
    errors, warnings, notes = [], [], []
    if not root.is_dir():
        return {'ok': False, 'errors': [f'{root} is not a directory'], 'warnings': []}
    robots_path = root / 'robots.txt'
    groups, maps = ({}, [])
    if robots_path.is_file():
        groups, maps = parse_robots(robots_path.read_text(encoding='utf-8', errors='replace'))
    else:
        warnings.append('no robots.txt: every crawler, including AI-training crawlers, may read everything')
    if expect_ai_blocked:
        open_to = [a for a in AI_AGENTS_TO_CHECK if robots_allows(groups, a, '/')]
        if open_to:
            errors.append(f'robots.txt lets these AI-training crawlers read the site: {", ".join(open_to)}')
    if not expect_indexable:
        if any(robots_allows(groups, a, '/') for a in SEARCH_AGENTS_TO_CHECK):
            errors.append('site is meant to stay out of search, but robots.txt lets search crawlers in')
        return {'ok': not errors, 'errors': errors, 'warnings': warnings, 'notes': ['non-indexable site: sitemap not required'],
                'sitemap_urls': 0, 'checked_html': 0}
    blocked_search = [a for a in SEARCH_AGENTS_TO_CHECK if not robots_allows(groups, a, '/')]
    if blocked_search:
        errors.append(f'robots.txt blocks the home page for search crawlers: {", ".join(blocked_search)}')
    if not (root / 'sitemap.xml').is_file():
        errors.append('no sitemap.xml')
        return {'ok': False, 'errors': errors, 'warnings': warnings, 'sitemap_urls': 0, 'checked_html': 0}
    if f'{base}/sitemap.xml' not in maps:
        warnings.append('robots.txt does not name the sitemap (Sitemap: line)')
    urls = _sitemap_urls(root, base, errors)
    seen, checked = set(), 0
    for loc in urls:
        parts = urlsplit(loc)
        if loc in seen:
            errors.append(f'duplicate sitemap URL {loc}')
        seen.add(loc)
        if parts.scheme != 'https' or f'{parts.scheme}://{parts.netloc}' != base:
            errors.append(f'sitemap URL not on {base}: {loc}')
            continue
        path = parts.path or '/'
        for agent in ('Googlebot', 'Bingbot', 'YandexBot', 'Baiduspider', 'Yeti'):
            if not robots_allows(groups, agent, unquote(path)):
                errors.append(f'robots.txt blocks {agent} from a sitemap URL: {loc}')
                break
        page = _html_for(root, path)
        if page is None:
            notes.append(f'{path}: served by the app shell (no static html to inspect)')
            continue
        checked += 1
        p = _inspect(page)
        if 'noindex' in ','.join(p.robots).lower():
            errors.append(f'noindex page listed in the sitemap: {loc}')
        if p.titles != 1:
            warnings.append(f'{path}: expected one <title>')
        if len(p.description) != 1 or not p.description[0].strip():
            warnings.append(f'{path}: missing meta description')
        if not p.lang:
            warnings.append(f'{path}: missing <html lang>')
        if p.canonical:
            canonical = p.canonical[0]
            if not canonical.startswith(base):
                errors.append(f'{path}: canonical points off-site ({canonical})')
        else:
            warnings.append(f'{path}: no canonical link')
    if not urls:
        warnings.append('sitemap lists no URLs')
    return {'ok': not errors, 'errors': errors, 'warnings': warnings, 'notes': notes[:50],
            'sitemap_urls': len(seen), 'checked_html': checked,
            'tested_scope': 'robots, sitemap and static html in the folder; not live indexing or ranking'}
