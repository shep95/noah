"""Validate the small USEL declarative language, written as YAML or JSON.

A page is either *rendered* (USEL writes its HTML under /<locale>/<slug>/) or a
*route* (``render: false``): a URL the site's own app already serves, which USEL
only lists in the sitemap, with hreflang alternates, and checks against robots.
"""
from __future__ import annotations
import json
import re
from datetime import date, datetime
from pathlib import PurePosixPath
from urllib.parse import urlsplit, quote

LOCALE_RE = re.compile(r'^[a-z]{2,3}(?:-[A-Za-z0-9]{2,8}){0,2}$')
SLUG_RE = re.compile(r'^[^?#\\\\]+$')
ROUTE_RE = re.compile(r'^/(?:[^?#\s\\\\]*)$')
STYLESHEET_RE = re.compile(r'^/[A-Za-z0-9._~/-]+\.css$')
INDEXNOW_KEY_RE = re.compile(r'^[A-Za-z0-9-]{8,128}$')
ROBOTS_PATH_RE = re.compile(r'^/[^\s#]*$')
MAX_SOURCE_BYTES = 1024 * 1024


class SpecError(ValueError):
    pass


def read_spec(text):
    if len(text.encode('utf-8')) > MAX_SOURCE_BYTES:
        raise SpecError('input exceeds 1 MB')
    stripped = text.lstrip()
    try:
        if stripped.startswith('{'):
            obj = json.loads(text)
        else:
            try:
                import yaml
            except ImportError as exc:
                raise SpecError('YAML source needs PyYAML; install it or write the spec as JSON') from exc
            obj = yaml.safe_load(text)
    except SpecError:
        raise
    except Exception as ex:
        raise SpecError(f'cannot parse source: {ex}') from ex
    if not isinstance(obj, dict):
        raise SpecError('specification must be an object (YAML or JSON)')
    return validate_spec(obj)


def require_https(url, label):
    if not isinstance(url, str):
        raise SpecError(f'{label}: expected URL')
    parts = urlsplit(url)
    if parts.scheme != 'https' or not parts.hostname or parts.username or parts.password or parts.query or parts.fragment:
        raise SpecError(f'{label}: expected absolute https URL without query, fragment or credentials')
    if parts.path not in ('', '/'):
        raise SpecError(f'{label}: base_url must have no path')
    return url.rstrip('/')


def _normalize_lastmod(page, pid):
    value = page.get('lastmod')
    if value is None:
        return
    # YAML turns an unquoted 2026-11-14 into a date; accept it rather than fail.
    if isinstance(value, datetime):
        value = value.date()
    if isinstance(value, date):
        page['lastmod'] = value.isoformat()
        return
    try:
        parsed = date.fromisoformat(value)
        if parsed.isoformat() != value:
            raise ValueError('non-canonical date')
    except (ValueError, TypeError):
        raise SpecError(f'page {pid}: lastmod must be a valid YYYY-MM-DD date') from None


def _validate_robots(spec):
    robots = spec.get('robots')
    if robots is None:
        spec['robots'] = {'disallow': [], 'block_ai_training': True, 'manage': True,
                          'allow_search': True, 'extra_ai_agents': []}
        return
    if not isinstance(robots, dict):
        raise SpecError('robots must be an object')
    unknown = set(robots) - {'disallow', 'block_ai_training', 'manage', 'extra_ai_agents', 'allow_search'}
    if unknown:
        raise SpecError(f'robots: unknown keys {sorted(unknown)}')
    disallow = robots.setdefault('disallow', [])
    if not isinstance(disallow, list) or not all(isinstance(p, str) and ROBOTS_PATH_RE.fullmatch(p) for p in disallow):
        raise SpecError('robots.disallow must be a list of paths starting with /')
    for key in ('block_ai_training', 'manage', 'allow_search'):
        robots.setdefault(key, True)
        if not isinstance(robots[key], bool):
            raise SpecError(f'robots.{key} must be boolean')
    extra = robots.setdefault('extra_ai_agents', [])
    if not isinstance(extra, list) or not all(isinstance(a, str) and re.fullmatch(r'[A-Za-z0-9._-]{2,64}', a) for a in extra):
        raise SpecError('robots.extra_ai_agents must be a list of user-agent tokens')


def page_url_path(page, locale, entry):
    """Site-relative URL path of one page in one locale."""
    if page.get('render', True):
        slug = entry['slug'].strip('/')
        return f'/{quote(locale, safe="")}/{quote(slug, safe="/")}/'
    return quote(entry['path'], safe='/-._~%')


def validate_spec(spec):
    if spec.get('usel') != 1:
        raise SpecError('usel must be 1')
    require_https(spec.get('base_url'), 'base_url')
    if not isinstance(spec.get('site_name'), str) or not spec['site_name'].strip():
        raise SpecError('site_name required')
    # Geographic targeting is independent of which translations actually exist.
    from .registry import load_countries
    allowed_markets = {c['code'] for c in load_countries()}
    markets = spec.get('market_targets', 'all')
    if markets != 'all' and (not isinstance(markets, list) or not markets or any(not isinstance(m, str) or m not in allowed_markets for m in markets)):
        raise SpecError('market_targets must be all or an array of known ISO/jurisdiction codes')
    default_locale = spec.get('default_locale')
    if default_locale is not None and (not isinstance(default_locale, str) or not LOCALE_RE.fullmatch(default_locale)):
        raise SpecError('default_locale must be a locale code such as en-US')
    stylesheet = spec.get('stylesheet')
    if stylesheet is not None and (not isinstance(stylesheet, str) or not STYLESHEET_RE.fullmatch(stylesheet) or '..' in stylesheet):
        raise SpecError('stylesheet must be a same-origin path to a .css file, such as /assets/seo.css')
    key = spec.get('indexnow_key')
    if key is not None and (not isinstance(key, str) or not INDEXNOW_KEY_RE.fullmatch(key)):
        raise SpecError('indexnow_key must be 8-128 letters, digits or hyphens')
    _validate_robots(spec)
    sitemap = spec.setdefault('sitemap', True)
    if not isinstance(sitemap, bool):
        raise SpecError('sitemap must be boolean (false when the site writes its own sitemap.xml)')
    pages = spec.get('pages')
    if not isinstance(pages, list) or not pages:
        raise SpecError('pages must be a nonempty list')
    ids, paths = set(), set()
    for page in pages:
        if not isinstance(page, dict):
            raise SpecError('invalid page')
        pid = page.get('id')
        if not isinstance(pid, str) or not re.fullmatch(r'[a-z0-9][a-z0-9_-]{0,79}', pid):
            raise SpecError('bad page id')
        if pid in ids:
            raise SpecError(f'duplicate page id: {pid}')
        ids.add(pid)
        if not isinstance(page.get('public'), bool):
            raise SpecError(f'page {pid}: public must be boolean')
        if not isinstance(page.get('index'), bool):
            raise SpecError(f'page {pid}: index must be boolean')
        if not page['public'] and page['index']:
            raise SpecError(f'page {pid}: private content cannot be indexed')
        render = page.setdefault('render', True)
        if not isinstance(render, bool):
            raise SpecError(f'page {pid}: render must be boolean')
        _normalize_lastmod(page, pid)
        locales = page.get('locales')
        if not isinstance(locales, dict) or not locales:
            raise SpecError(f'page {pid}: locales must be nonempty')
        for locale, content in locales.items():
            if not isinstance(locale, str) or not LOCALE_RE.fullmatch(locale):
                raise SpecError(f'invalid locale: {locale}')
            if not isinstance(content, dict):
                raise SpecError(f'{pid}:{locale}: missing fields')
            if render:
                for field in ('slug', 'title', 'description', 'body'):
                    if not isinstance(content.get(field), str) or not content[field].strip():
                        raise SpecError(f'{pid}:{locale}: {field} must be nonempty text')
                slug = content['slug'].strip('/')
                if (not slug or not SLUG_RE.fullmatch(slug) or '..' in PurePosixPath(slug).parts
                        or any(s in ('.', '') for s in slug.split('/')) or slug.startswith('.')):
                    raise SpecError(f'{pid}:{locale}: invalid slug')
            else:
                route = content.get('path')
                if not isinstance(route, str) or not ROUTE_RE.fullmatch(route) or '/../' in route + '/' or '//' in route:
                    raise SpecError(f'{pid}:{locale}: a route page (render: false) needs a path starting with /')
            path = page_url_path(page, locale, content)
            if path in paths:
                raise SpecError(f'URL collision {path}')
            paths.add(path)
    if not spec['robots']['allow_search'] and any(p['index'] for p in pages):
        raise SpecError('robots.allow_search is false, so no page can be index: true')
    engines = spec.get('engines', ['all'])
    if not isinstance(engines, list) or not engines or not all(isinstance(s, str) for s in engines):
        raise SpecError('engines must be an array')
    if 'all' in engines and len(engines) > 1:
        raise SpecError('use all alone or explicit engine ids')
    spec.setdefault('engines', engines)
    return spec
