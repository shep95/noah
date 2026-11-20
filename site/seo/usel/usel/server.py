"""Local-use interactive UI. Do not expose publicly without production security review."""
from __future__ import annotations
from fastapi import FastAPI,HTTPException,Request
from fastapi.responses import FileResponse,JSONResponse,StreamingResponse
from pydantic import BaseModel
from pathlib import Path
from tempfile import TemporaryDirectory
from io import BytesIO
import zipfile, json
from .registry import load_engines,load_countries
from .models import read_spec,SpecError
from .compiler import compile_site
from .auditor import audit_site

ROOT=Path(__file__).resolve().parents[1]
app=FastAPI(title='USEL local studio',docs_url=None,redoc_url=None,openapi_url=None)
MAX_BODY=1_100_000

# Local-only tool, still served like a public page: no inline code, no framing,
# no third-party anything, and a request body cap before parsing.
STUDIO_HEADERS={
    'Content-Security-Policy':"default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    'X-Content-Type-Options':'nosniff','X-Frame-Options':'DENY','Referrer-Policy':'no-referrer',
    'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Resource-Policy':'same-origin',
    'Permissions-Policy':'camera=(), microphone=(), geolocation=()','Cache-Control':'no-store',
}
LOCAL_HOSTS={'127.0.0.1','localhost','[::1]','::1','testserver'}

@app.middleware('http')
async def guard(request:Request,call_next):
    host=(request.headers.get('host') or '').rsplit(':',1)[0] if not (request.headers.get('host') or '').startswith('[') else (request.headers.get('host') or '').split(']')[0]+']'
    # DNS rebinding: a page on another site cannot reach this server through its own hostname.
    if host not in LOCAL_HOSTS:
        return JSONResponse({'detail':'local use only'},status_code=421,headers=STUDIO_HEADERS)
    length=request.headers.get('content-length')
    if length and (not length.isdigit() or int(length)>MAX_BODY):
        return JSONResponse({'detail':'request body too large'},status_code=413,headers=STUDIO_HEADERS)
    response=await call_next(request)
    for key,value in STUDIO_HEADERS.items():
        response.headers.setdefault(key,value)
    return response

class Payload(BaseModel):
    source:str

@app.get('/')
def home():return FileResponse(ROOT/'ui/index.html',media_type='text/html')
@app.get('/style.css')
def style():return FileResponse(ROOT/'ui/style.css',media_type='text/css')
@app.get('/app.js')
def script():return FileResponse(ROOT/'ui/app.js',media_type='text/javascript')
@app.get('/api/example')
def example():return FileResponse(ROOT/'examples/bosley.usel.yaml',media_type='text/plain')
@app.get('/api/summary')
def summary():
    engines=load_engines();countries=load_countries()
    return {'countries_and_jurisdictions':len(countries),'engine_and_crawler_profiles':len(engines),
            'documented_profiles':sum(e['verification']=='documented' for e in engines),
            'unresearched_country_markets':sum(c['search_engine_discovery_status']=='not_audited' for c in countries),
            'verification':'documentation reviewed only; not live indexing/traffic validation'}
@app.get('/api/engines')
def engines():return load_engines()
@app.get('/api/countries')
def countries():return load_countries()
@app.post('/api/compile')
def compile_endpoint(data:Payload):
    try:
        spec=read_spec(data.source)
        with TemporaryDirectory() as folder:
            summary=compile_site(spec,folder)
            audit=audit_site(folder)
            if not audit['ok']:
                raise HTTPException(status_code=422,detail=audit)
            blob=BytesIO()
            with zipfile.ZipFile(blob,'w',zipfile.ZIP_DEFLATED) as zf:
                for p in Path(folder).rglob('*'):
                    if p.is_file():zf.write(p,p.relative_to(folder).as_posix())
            blob.seek(0)
            return StreamingResponse(blob,media_type='application/zip',headers={'Content-Disposition':'attachment; filename="usel-output.zip"',
                'X-USEL-Indexed-Pages':str(summary['indexed_pages']),'X-USEL-Verified':'static-output-audit'})
    except SpecError as ex:raise HTTPException(400,str(ex))
