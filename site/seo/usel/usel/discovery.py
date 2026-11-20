"""Safe discovery registry workflows; no speculative discovery claims."""
import csv
import json
import ssl
from pathlib import Path
from urllib.request import Request,urlopen
from .registry import DATA, load_engines, load_countries, validate_registry

PARTICIPANTS_URL='https://www.indexnow.org/searchengines.json'

def coverage_csv(path):
    countries=load_countries()
    known=load_engines()
    with open(path,'w',newline='',encoding='utf-8') as file:
        w=csv.writer(file)
        w.writerow(['iso2_or_jurisdiction','country_or_territory','candidate_engines_by_origin','verified_engines_in_market','coverage','remaining_research'])
        for c in countries:
            candidates=[e['id'] for e in known if e.get('origin_country')==c['code']]
            w.writerow([c['code'],c['name'],';'.join(candidates),';'.join(c['engines_verified']),
                        c['search_engine_discovery_status'],'research local-language engines, providers, crawlers and SEO rules'])
    return len(countries)

def discover_indexnow(path):
    # Fetch ONLY a fixed, official public URL; cap response bytes and timeout.
    req=Request(PARTICIPANTS_URL,headers={'User-Agent':'USEL-registry-research/0.1'})
    with urlopen(req,timeout=12,context=ssl.create_default_context()) as r:
        if r.status != 200: raise ValueError(f'indexnow manifest HTTP {r.status}')
        raw=r.read(65537)
        if len(raw)>65536: raise ValueError('remote response too large')
    data=json.loads(raw)
    if not isinstance(data,dict): raise ValueError('invalid partner data')
    if not all(isinstance(k,str) and isinstance(v,str) and v.startswith('https://') for k,v in data.items()):
        raise ValueError('invalid partner metadata')
    Path(path).write_text(json.dumps({'source':PARTICIPANTS_URL,'participants':data,
            'note':'auto-discovered participants need their own SEO review; this is not automatic compatibility'},indent=2),encoding='utf-8')
    return len(data)

def add_engine(record, destination=None):
    path=Path(destination) if destination else DATA/'engine_overrides.json'
    before=json.loads(path.read_text(encoding='utf-8'))['engines'] if path.exists() else []
    previous=load_engines()
    if record.get('id') in {e['id'] for e in previous+before}:
        raise ValueError('duplicate engine id; update explicitly in source instead')
    validate_registry([record])
    before.append(record)
    path.write_text(json.dumps({'engines':before},ensure_ascii=False,indent=2),encoding='utf-8')
    return path
