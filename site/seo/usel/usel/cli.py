"""Command-line interface. 'compile' has NO network side effects."""
from __future__ import annotations
import argparse
import secrets
import json
import sys
import tempfile
import zipfile
from pathlib import Path
from urllib.parse import urlsplit
from urllib.request import Request,urlopen
from .models import read_spec, SpecError
from .registry import load_countries, load_engines,validate_registry
from .compiler import compile_site
from .auditor import audit_site, check_site
from .discovery import coverage_csv, discover_indexnow,add_engine


def urls_from_sitemap(path):
    # A site's own sitemap (or sitemap index next to its parts) as the URL list.
    import xml.etree.ElementTree as ET
    ns='{http://www.sitemaps.org/schemas/sitemap/0.9}'
    root=Path(path)
    tree=ET.parse(root)
    if tree.getroot().tag==ns+'sitemapindex':
        urls=[]
        for node in tree.getroot().findall(ns+'sitemap'):
            urls+=urls_from_sitemap(root.parent/(node.findtext(ns+'loc') or '').rsplit('/',1)[-1])
        return urls
    return [(n.findtext(ns+'loc') or '').strip() for n in tree.getroot().findall(ns+'url')]

def submit_indexnow(plan, key, host, execute=False, sitemap=None):
    # For only publicly published URLs on the SAME host. User must host <key>.txt before sending.
    if not key or not all(c.isalnum() or c in '-_' for c in key) or len(key)>128:
        raise ValueError('IndexNow key must be nonempty, alphanumeric/hyphen/underscore, <=128 chars')
    if sitemap:
        urls=urls_from_sitemap(sitemap)
    elif plan:
        urls=json.loads(Path(plan).read_text(encoding='utf-8'))['urlList']
    else:
        raise ValueError('give a plan file or --sitemap')
    parsed=urlsplit(host)
    if parsed.scheme!='https' or not parsed.hostname or parsed.path not in ('','/') or parsed.query or parsed.fragment:
        raise ValueError('host must be a bare https site origin')
    hostname=parsed.hostname
    if not urls: return {'submitted':0,'message':'no URLs to submit'}
    if any(urlsplit(u).scheme!='https' or urlsplit(u).hostname != hostname for u in urls):
        raise ValueError('all submitted URLs must use the same HTTPS hostname')
    data={'host':hostname,'key':key,'keyLocation':host.rstrip('/')+'/'+key+'.txt','urlList':urls[:10000]}
    if len(urls)>10000: raise ValueError('submit in <=10000 URL batches')
    if not execute: return {'submitted':0,'would_notify':len(urls),'endpoint':'https://api.indexnow.org/IndexNow',
        'note':'dry run. host the key file at keyLocation, then use --execute.'}
    request=Request('https://api.indexnow.org/IndexNow',data=json.dumps(data).encode('utf-8'),
                    headers={'Content-Type':'application/json; charset=utf-8','User-Agent':'USEL/0.2'},method='POST')
    with urlopen(request,timeout=15) as response:
        status=response.status
    return {'submitted':len(urls) if status in (200,202) else 0,'http_status':status,
            'note':'successful submission is NOT a guarantee of crawling/indexing/ranking'}

def main(argv=None):
    ap=argparse.ArgumentParser(prog='usel',description='universal SEO compiler — evidence-aware, locale-first')
    sub=ap.add_subparsers(dest='command',required=True)
    p=sub.add_parser('compile');p.add_argument('source');p.add_argument('--output',default='build')
    p.add_argument('--reports',default=None,help='folder for engine guide, coverage, manifest and IndexNow plan (default: --output). keep it out of a public folder.')
    p=sub.add_parser('audit');p.add_argument('output');p.add_argument('--reports',default=None)
    p=sub.add_parser('check-site',help='audit an existing built site folder: robots, sitemap, canonical, AI-crawler rules')
    p.add_argument('folder');p.add_argument('--base-url',required=True)
    p.add_argument('--allow-ai',action='store_true',help='do not require AI-training crawlers to be blocked')
    p.add_argument('--not-indexable',action='store_true',help='the site is meant to stay out of search')
    p=sub.add_parser('indexnow-key',help='print a new random IndexNow key for the spec')
    p=sub.add_parser('engines');p.add_argument('--documented-only',action='store_true')
    p=sub.add_parser('countries');p.add_argument('--only-unreviewed',action='store_true')
    p=sub.add_parser('coverage');p.add_argument('--output',default='coverage.csv')
    p=sub.add_parser('refresh-indexnow');p.add_argument('--output',default='indexnow_live.json')
    p=sub.add_parser('add-engine');p.add_argument('json_file')
    p=sub.add_parser('submit-indexnow');p.add_argument('plan',nargs='?');p.add_argument('--sitemap',help='take the URL list from a built sitemap.xml instead of a plan')
    p.add_argument('--key',required=True);p.add_argument('--host',required=True);p.add_argument('--execute',action='store_true')
    p=sub.add_parser('serve');p.add_argument('--host',default='127.0.0.1');p.add_argument('--port',type=int,default=8090)
    a=ap.parse_args(argv)
    try:
        if a.command=='compile':
            spec=read_spec(Path(a.source).read_text(encoding='utf-8'))
            result=compile_site(spec,a.output,a.reports)
            result['audit']=audit_site(a.output,a.reports)
            result['ok']=result['audit']['ok']
        elif a.command=='audit':result=audit_site(a.output,a.reports)
        elif a.command=='check-site':
            result=check_site(a.folder,a.base_url,expect_ai_blocked=not a.allow_ai,expect_indexable=not a.not_indexable)
        elif a.command=='indexnow-key':result={'indexnow_key':secrets.token_hex(16),
            'next':'put it in the spec as indexnow_key and compile; the key file is written next to sitemap.xml'}
        elif a.command=='engines':
            result=[{'id':e['id'],'name':e['name'],'verification':e['verification'],'kind':e['kind']} for e in load_engines() if not a.documented_only or e['verification']=='documented']
        elif a.command=='countries':result=[{'code':c['code'],'name':c['name'],'coverage':c['search_engine_discovery_status']} for c in load_countries() if not a.only_unreviewed or c['search_engine_discovery_status']=='not_audited']
        elif a.command=='coverage':result={'countries_written':coverage_csv(a.output),'output':a.output}
        elif a.command=='refresh-indexnow':result={'indexnow_participants':discover_indexnow(a.output),'output':a.output}
        elif a.command=='add-engine':
            result={'saved_to':str(add_engine(json.loads(Path(a.json_file).read_text(encoding='utf-8'))))}
        elif a.command=='submit-indexnow':result=submit_indexnow(a.plan,a.key,a.host,a.execute,a.sitemap)
        elif a.command=='serve':
            import uvicorn
            uvicorn.run('usel.server:app',host=a.host,port=a.port,log_level='info')
            return 0
        print(json.dumps(result,ensure_ascii=False,indent=2))
        return 0 if not isinstance(result,dict) or result.get('ok',True) else 1
    except (ValueError,KeyError,FileNotFoundError,ImportError,OSError) as exc:
        print(json.dumps({'error':str(exc)}),file=sys.stderr)
        return 2
if __name__=='__main__':sys.exit(main())
