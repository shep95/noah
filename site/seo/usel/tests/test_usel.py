from pathlib import Path
from copy import deepcopy
import json
import xml.etree.ElementTree as ET
import pytest
from fastapi.testclient import TestClient
from usel.models import read_spec,validate_spec,SpecError
from usel.compiler import compile_site,url_path
from usel.registry import load_countries, load_engines,engine_by_id,validate_registry
from usel.auditor import audit_site
from usel.cli import submit_indexnow
from usel.server import app

SOURCE=Path(__file__).resolve().parents[1]/'examples/bosley.usel.yaml'

@pytest.fixture
def example():return read_spec(SOURCE.read_text(encoding='utf-8'))

@pytest.fixture
def output(example,tmp_path):
    compile_site(example,tmp_path)
    return tmp_path

def test_global_geo_count():
    countries=load_countries()
    assert len(countries)==250
    assert len({c['code'] for c in countries})==250
    assert all(c['search_engine_discovery_status']=='not_audited' for c in countries)

def test_engine_registry_labels():
    engines=load_engines()
    assert len(engines)==50 and validate_registry()
    assert len([e for e in engines if e['verification']=='documented'])==15
    assert engine_by_id('naver')['capabilities']['indexnow'] is True
    assert engine_by_id('mojeek')['capabilities']['sitemap'] is None
    assert engine_by_id('duckduckgo')['capabilities']['sitemap'] is None
    assert engine_by_id('ecosia')['result_sources']==['bing','google','eusp']

def test_compiles_multilingual_pages(output):
    a=audit_site(output)
    assert a['ok'] and a['rendered_files']==8 and a['indexed_pages']==8
    assert len(list(output.rglob('index.html')))==8

def test_multilingual_namespaces_and_reciprocity(output):
    tree=ET.parse(output/'sitemap.xml')
    ns='{http://www.sitemaps.org/schemas/sitemap/0.9}'
    xns='{http://www.w3.org/1999/xhtml}'
    entries=tree.getroot().findall(ns+'url')
    assert len(entries)==8
    assert all(len(n.findall(xns+'link'))==8 for n in entries)

def test_private_content_not_exported(output):
    assert not any('internal-message' in p.as_posix() for p in output.rglob('*'))
    for p in output.rglob('*'):
        if p.is_file(): assert b'sensitive demo text' not in p.read_bytes()

def test_html_escapes_untrusted_data(example,tmp_path):
    example['pages'][0]['locales']['en-US']['title']='hello <script>alert(1)</script>'
    example['pages'][0]['locales']['en-US']['body']='<img src=x onerror=alert(1)>'
    compile_site(example,tmp_path)
    html=(tmp_path/'en-US/discussions/welcome/index.html').read_text()
    assert '<script>alert(1)</script>' not in html
    assert '&lt;script&gt;' in html
    assert '&lt;img' in html
    assert 'hello \\u003cscript>' in html

def test_public_noindex_not_in_sitemap(example,tmp_path):
    example['pages'][0]['index']=False
    compile_site(example,tmp_path)
    doc=(tmp_path/'en-US/discussions/welcome/index.html').read_text()
    assert 'noindex,nofollow' in doc
    assert 'https://bosley.example/en-US' not in (tmp_path/'sitemap.xml').read_text()
    assert audit_site(tmp_path)['ok']

def test_private_cannot_index(example):
    example['pages'][1]['index']=True
    with pytest.raises(SpecError,match='private'):validate_spec(example)

def test_duplicate_pages_rejected(example):
    example['pages'].append(deepcopy(example['pages'][0]))
    with pytest.raises(SpecError,match='duplicate'):validate_spec(example)

def test_duplicate_paths_rejected(example):
    x=deepcopy(example['pages'][0]);x['id']='another'
    example['pages'].append(x)
    with pytest.raises(SpecError,match='collision'):validate_spec(example)

@pytest.mark.parametrize('slug',['../secret','a/../secret','a//b','a?b','a#b','a\\b'])
def test_unsafe_slug_rejected(example,slug):
    example['pages'][0]['locales']['en-US']['slug']=slug
    with pytest.raises(SpecError):validate_spec(example)

def test_invalid_locale_rejected(example):
    bad=example['pages'][0]['locales'].pop('ko-KR')
    example['pages'][0]['locales']['KR']=bad
    with pytest.raises(SpecError):validate_spec(example)

def test_invalid_lastmod_rejected(example):
    example['pages'][0]['lastmod']='2026-13-14'
    with pytest.raises(SpecError):validate_spec(example)

def test_http_base_rejected(example):
    example['base_url']='http://bosley.example'
    with pytest.raises(SpecError):validate_spec(example)

def test_unknown_engine_rejected(example,tmp_path):
    example['engines']=['made_up']
    with pytest.raises(SpecError,match='unknown engine'):compile_site(example,tmp_path)

def test_country_market_report(output):
    import csv
    rows=list(csv.DictReader((output/'market-coverage.csv').open()))
    assert len(rows)==250
    assert next(x for x in rows if x['code']=='KR')['status']=='country_locale'
    assert any(x['status']=='unlocalized' for x in rows)

def test_fewer_market_targets(example,tmp_path):
    example['market_targets']=['US','IN','KR']
    result=compile_site(example,tmp_path)
    assert result['market_coverage']['targeted_markets']==3

def test_auditor_detects_corrupt_canonical(output):
    p=output/'en-US/discussions/welcome/index.html'
    p.write_text(p.read_text().replace('rel="canonical" href="https://bosley.example/en-US/discussions/welcome/"',
                                         'rel="canonical" href="https://wrong.example/"'))
    result=audit_site(output)
    assert not result['ok']
    assert any('canonical' in x for x in result['errors'])

def test_submit_is_dry_run(output):
    result=submit_indexnow(output/'indexnow-plan.json','dummy123','https://bosley.example')
    assert result['submitted']==0 and result['would_notify']==8

def test_submit_host_mismatch_blocked(output):
    with pytest.raises(ValueError,match='same HTTPS hostname'):
        submit_indexnow(output/'indexnow-plan.json','dummy123','https://evil.example')

def test_http_api_compile(example):
    with TestClient(app) as client:
        r=client.get('/api/summary')
        assert r.status_code==200 and r.json()['engine_and_crawler_profiles']==50
        r=client.post('/api/compile',json={'source':SOURCE.read_text()})
        assert r.status_code==200 and r.headers['content-type']=='application/zip'
        import zipfile,io
        assert 'manifest.json' in zipfile.ZipFile(io.BytesIO(r.content)).namelist()

def test_rejected_invalid_source():
    with TestClient(app) as client:
        r=client.post('/api/compile',json={'source':'not: a valid useL definition'})
        assert r.status_code==400
