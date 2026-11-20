"""0.2: route pages, x-default, robots policy, reports outside public, site checks."""
from copy import deepcopy
from datetime import date
import json
import xml.etree.ElementTree as ET
import pytest
from fastapi.testclient import TestClient
from usel.models import read_spec, validate_spec, SpecError
from usel.compiler import compile_site, build_robots, AI_TRAINING_AGENTS
from usel.auditor import audit_site, check_site, parse_robots, robots_allows
from usel.cli import main
from usel.server import app

NS = '{http://www.sitemaps.org/schemas/sitemap/0.9}'
XNS = '{http://www.w3.org/1999/xhtml}'


def site_spec():
    return {
        'usel': 1, 'site_name': 'example', 'base_url': 'https://www.example.com',
        'default_locale': 'en', 'market_targets': ['US', 'DE'],
        'robots': {'disallow': ['/dashboard', '/api/']},
        'indexnow_key': 'a1b2c3d4e5f60718',
        'pages': [
            {'id': 'home', 'public': True, 'index': True, 'render': False, 'lastmod': '2026-11-18',
             'locales': {'en': {'path': '/'}, 'de': {'path': '/de/'}}},
            {'id': 'pricing', 'public': True, 'index': True, 'render': False,
             'locales': {'en': {'path': '/software'}}},
            {'id': 'guide', 'public': True, 'index': True,
             'locales': {'en': {'slug': 'guide', 'title': 'guide', 'description': 'the guide', 'body': 'text'},
                         'de': {'slug': 'anleitung', 'title': 'anleitung', 'description': 'die anleitung', 'body': 'text'}}},
            {'id': 'account', 'public': True, 'index': False, 'render': False,
             'locales': {'en': {'path': '/dashboard'}}},
        ],
    }


@pytest.fixture
def built(tmp_path):
    public, reports = tmp_path / 'public', tmp_path / 'reports'
    summary = compile_site(validate_spec(site_spec()), public, reports)
    return public, reports, summary


def test_route_pages_go_to_the_sitemap_without_writing_html(built):
    public, reports, summary = built
    assert summary['route_urls'] == 3 and summary['rendered_pages'] == 2
    assert not (public / 'software').exists() and not (public / 'index.html').exists()
    urls = [n.findtext(NS + 'loc') for n in ET.parse(public / 'sitemap.xml').getroot().findall(NS + 'url')]
    assert 'https://www.example.com/' in urls and 'https://www.example.com/software' in urls
    assert 'https://www.example.com/dashboard' not in urls  # index: false
    assert audit_site(public, reports)['ok']


def test_x_default_points_at_the_default_locale(built):
    public, _, _ = built
    root = ET.parse(public / 'sitemap.xml').getroot()
    home = next(n for n in root.findall(NS + 'url') if n.findtext(NS + 'loc') == 'https://www.example.com/')
    links = {l.get('hreflang'): l.get('href') for l in home.findall(XNS + 'link')}
    assert links == {'en': 'https://www.example.com/', 'de': 'https://www.example.com/de/', 'x-default': 'https://www.example.com/'}
    page = (public / 'en' / 'guide' / 'index.html').read_text(encoding='utf-8')
    assert 'hreflang="x-default" href="https://www.example.com/en/guide/"' in page


def test_reports_stay_out_of_the_public_folder(built):
    public, reports, _ = built
    for name in ('engine-guide.md', 'market-coverage.csv', 'manifest.json', 'indexnow-plan.json'):
        assert (reports / name).is_file() and not (public / name).exists()


def test_robots_blocks_ai_training_but_not_search(built):
    public, _, _ = built
    groups, maps = parse_robots((public / 'robots.txt').read_text(encoding='utf-8'))
    assert maps == ['https://www.example.com/sitemap.xml']
    for agent in ('GPTBot', 'CCBot', 'ClaudeBot', 'Google-Extended', 'Bytespider'):
        assert not robots_allows(groups, agent, '/')
    for agent in ('Googlebot', 'Bingbot', 'YandexBot', 'Baiduspider', 'Yeti', 'SeznamBot'):
        assert robots_allows(groups, agent, '/') and robots_allows(groups, agent, '/software')
        assert not robots_allows(groups, agent, '/dashboard/settings')


def test_robots_matching_follows_rfc9309():
    groups, _ = parse_robots('User-agent: *\nDisallow: /a\nAllow: /a/public\nDisallow: /*.pdf$\n')
    assert not robots_allows(groups, 'x', '/a/b')
    assert robots_allows(groups, 'x', '/a/public/page')  # longer rule wins
    assert not robots_allows(groups, 'x', '/files/doc.pdf')
    assert robots_allows(groups, 'x', '/files/doc.pdf?x=1')


def test_indexnow_key_file_is_published(built):
    public, reports, _ = built
    assert (public / 'a1b2c3d4e5f60718.txt').read_text() == 'a1b2c3d4e5f60718'
    plan = json.loads((reports / 'indexnow-plan.json').read_text())
    assert plan['key'] == 'a1b2c3d4e5f60718' and 'https://www.example.com/software' in plan['urlList']


def test_audit_catches_a_robots_rule_that_hides_a_sitemap_url(tmp_path):
    spec = site_spec()
    spec['robots']['disallow'].append('/software')
    public, reports = tmp_path / 'p', tmp_path / 'r'
    compile_site(validate_spec(spec), public, reports)
    result = audit_site(public, reports)
    assert not result['ok'] and any('blocks a sitemap URL' in e for e in result['errors'])


def test_non_ascii_pages_are_written_where_a_host_finds_them(tmp_path):
    spec = site_spec()
    spec['pages'][2]['locales']['zh-CN'] = {'slug': '讨论/欢迎', 'title': '欢迎', 'description': '公开讨论', 'body': '内容'}
    compile_site(validate_spec(spec), tmp_path / 'p', tmp_path / 'r')
    assert (tmp_path / 'p' / 'zh-CN' / '讨论' / '欢迎' / 'index.html').is_file()
    sitemap = (tmp_path / 'p' / 'sitemap.xml').read_text(encoding='utf-8')
    assert 'https://www.example.com/zh-CN/%E8%AE%A8%E8%AE%BA/%E6%AC%A2%E8%BF%8E/' in sitemap
    assert audit_site(tmp_path / 'p', tmp_path / 'r')['ok']


def test_full_locale_in_structured_data_and_optional_stylesheet(tmp_path):
    spec = site_spec()
    spec['stylesheet'] = '/assets/seo.css'
    spec['pages'][2]['locales']['de']['slug'] = 'anleitung'
    compile_site(validate_spec(spec), tmp_path / 'p', tmp_path / 'r')
    page = (tmp_path / 'p' / 'de' / 'anleitung' / 'index.html').read_text(encoding='utf-8')
    assert '"inLanguage": "de"' in page and '<link rel="stylesheet" href="/assets/seo.css">' in page
    assert 'style=' not in page and '<style' not in page  # nothing a strict CSP would block


@pytest.mark.parametrize('field,value', [
    ('stylesheet', 'https://evil.example/x.css'), ('stylesheet', '/../x.css'),
    ('indexnow_key', 'short'), ('indexnow_key', 'has space key123'),
    ('robots', {'disallow': ['no-slash']}), ('robots', {'block_ai_training': 'yes'}),
    ('robots', {'unknown': True}), ('default_locale', 'english please'),
])
def test_rejects_bad_new_fields(field, value):
    spec = site_spec()
    spec[field] = value
    with pytest.raises(SpecError):
        validate_spec(spec)


@pytest.mark.parametrize('path', ['software', '/a//b', '/a/../b', '/a b', '/x?y=1'])
def test_rejects_bad_route_paths(path):
    spec = site_spec()
    spec['pages'][1]['locales']['en']['path'] = path
    with pytest.raises(SpecError):
        validate_spec(spec)


def test_yaml_dates_are_accepted():
    spec = site_spec()
    spec['pages'][0]['lastmod'] = date(2026, 11, 18)
    assert validate_spec(spec)['pages'][0]['lastmod'] == '2026-11-18'
    text = 'usel: 1\nsite_name: x\nbase_url: https://x.example\npages:\n  - id: a\n    public: true\n    index: true\n    render: false\n    lastmod: 2026-11-18\n    locales:\n      en: {path: /}\n'
    assert read_spec(text)['pages'][0]['lastmod'] == '2026-11-18'


def test_unmanaged_robots_leaves_the_sites_file_alone(tmp_path):
    spec = site_spec()
    spec['robots'] = {'manage': False}
    public = tmp_path / 'p'
    public.mkdir()
    (public / 'robots.txt').write_text('hand written\n')
    compile_site(validate_spec(spec), public, tmp_path / 'r')
    assert (public / 'robots.txt').read_text() == 'hand written\n'
    assert audit_site(public, tmp_path / 'r')['ok']


def test_check_site_on_a_good_build(built):
    public, _, _ = built
    (public / 'index.html').write_text('<html lang="en"><head><title>x</title><meta name="description" content="d">'
                                       '<link rel="canonical" href="https://www.example.com/"></head></html>')
    result = check_site(public, 'https://www.example.com')
    assert result['ok'], result['errors']
    assert result['sitemap_urls'] == 5 and result['checked_html'] == 3


def test_check_site_flags_open_ai_crawlers_and_bad_sitemaps(tmp_path):
    (tmp_path / 'robots.txt').write_text('User-agent: *\nDisallow: /private\n')
    (tmp_path / 'sitemap.xml').write_text(
        '<?xml version="1.0"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
        '<url><loc>https://www.example.com/private/x</loc></url>'
        '<url><loc>http://www.example.com/</loc></url>'
        '<url><loc>https://other.example/</loc></url></urlset>')
    (tmp_path / 'index.html').write_text('<html><head><meta name="robots" content="noindex"></head></html>')
    errors = ' | '.join(check_site(tmp_path, 'https://www.example.com')['errors'])
    assert 'AI-training crawlers' in errors and 'blocks Googlebot from a sitemap URL' in errors
    assert errors.count('not on https://www.example.com') == 2


def test_check_site_for_a_site_that_stays_out_of_search(tmp_path):
    (tmp_path / 'robots.txt').write_text('User-agent: *\nDisallow: /\n')
    assert check_site(tmp_path, 'https://adam.example', expect_indexable=False)['ok']
    (tmp_path / 'robots.txt').write_text('User-agent: *\nAllow: /\n')
    assert not check_site(tmp_path, 'https://adam.example', expect_indexable=False)['ok']


def test_cli_compile_with_reports_and_check_site(tmp_path, capsys):
    source = tmp_path / 'site.json'
    source.write_text(json.dumps(site_spec()))
    assert main(['compile', str(source), '--output', str(tmp_path / 'pub'), '--reports', str(tmp_path / 'rep')]) == 0
    assert json.loads(capsys.readouterr().out)['audit']['ok']
    assert main(['check-site', str(tmp_path / 'pub'), '--base-url', 'https://www.example.com']) == 0
    assert main(['indexnow-key']) == 0
    key = json.loads(capsys.readouterr().out.split('\n}\n')[-2] + '\n}')['indexnow_key']
    assert len(key) == 32 and all(c in '0123456789abcdef' for c in key)


def test_generated_robots_lists_every_ai_agent_once():
    text = build_robots(validate_spec(site_spec()), 'https://www.example.com')
    for agent in AI_TRAINING_AGENTS:
        assert text.count(f'User-agent: {agent}\n') == 1


def test_studio_sends_security_headers_and_refuses_foreign_hosts():
    client = TestClient(app)
    r = client.get('/api/summary')
    assert r.status_code == 200
    assert "frame-ancestors 'none'" in r.headers['content-security-policy']
    assert r.headers['x-frame-options'] == 'DENY' and r.headers['x-content-type-options'] == 'nosniff'
    assert client.get('/api/summary', headers={'host': 'attacker.example'}).status_code == 421
    big = client.post('/api/compile', content=b'x' * 1_200_000, headers={'content-type': 'application/json'})
    assert big.status_code == 413


def test_sitemap_false_leaves_the_sites_sitemap_alone(tmp_path):
    spec = site_spec()
    spec['sitemap'] = False
    spec['robots']['manage'] = False
    public = tmp_path / 'p'
    public.mkdir()
    (public / 'sitemap.xml').write_text('site owned')
    compile_site(validate_spec(spec), public, tmp_path / 'r')
    assert (public / 'sitemap.xml').read_text() == 'site owned'
    assert (public / 'a1b2c3d4e5f60718.txt').is_file()
    assert audit_site(public, tmp_path / 'r')['ok']


def test_a_site_that_stays_out_of_search(tmp_path):
    spec = site_spec()
    spec['robots'] = {'allow_search': False}
    for page in spec['pages']:
        page['index'] = False
    public = tmp_path / 'p'
    compile_site(validate_spec(spec), public, tmp_path / 'r')
    groups, maps = parse_robots((public / 'robots.txt').read_text())
    assert not robots_allows(groups, 'Googlebot', '/') and maps == []
    assert not (public / 'sitemap.xml').exists() and not (public / 'a1b2c3d4e5f60718.txt').exists()
    assert audit_site(public, tmp_path / 'r')['ok']
    assert check_site(public, 'https://www.example.com', expect_indexable=False)['ok']


def test_allow_search_false_rejects_indexable_pages():
    spec = site_spec()
    spec['robots'] = {'allow_search': False}
    with pytest.raises(SpecError):
        validate_spec(spec)


def test_indexnow_dry_run_from_a_sitemap(built):
    from usel.cli import submit_indexnow
    public, _, _ = built
    result = submit_indexnow(None, 'a1b2c3d4e5f60718', 'https://www.example.com', sitemap=public / 'sitemap.xml')
    assert result['submitted'] == 0 and result['would_notify'] == 5
    with pytest.raises(ValueError):
        submit_indexnow(None, 'a1b2c3d4e5f60718', 'https://other.example', sitemap=public / 'sitemap.xml')
