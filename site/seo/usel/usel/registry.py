"""Versioned, evidence-aware registries. Unknown is never silently converted to supported."""
from __future__ import annotations
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'data'

def load_engines():
    base = json.loads((DATA / 'engines.json').read_text(encoding='utf-8'))['engines']
    overrides = DATA / 'engine_overrides.json'
    if overrides.exists():
        base.extend(json.loads(overrides.read_text(encoding='utf-8'))['engines'])
    return base

def load_countries():
    return json.loads((DATA / 'countries.json').read_text(encoding='utf-8'))['countries']

def engine_by_id(engine_id):
    for engine in load_engines():
        if engine['id'] == engine_id:
            return engine
    raise KeyError(f'unknown search engine: {engine_id}')

def validate_registry(engines=None):
    engines = engines if engines is not None else load_engines()
    seen = set()
    allowed = {'documented','research_needed'}
    for engine in engines:
        key = engine['id']
        if key in seen: raise ValueError(f'duplicate engine id: {key}')
        seen.add(key)
        if engine.get('verification') not in allowed:
            raise ValueError(f'invalid verification: {key}')
        if engine.get('capabilities') is None: raise ValueError(f'missing capabilities: {key}')
        for capability in ('sitemap','indexnow'):
            if engine['capabilities'].get(capability) not in (True, False, None):
                raise ValueError(f'invalid {capability}: {key}')
        if engine['verification']=='documented' and not engine.get('evidence'):
            raise ValueError(f'documented engine without evidence: {key}')
    return True
