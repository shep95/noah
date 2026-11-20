'use strict';
let engines=[],countries=[];
const $=id=>document.getElementById(id);
const labels={overview:'global search coverage',compiler:'universal SEO compiler',engines:'engine intelligence',countries:'worldwide market register'};
for(const button of document.querySelectorAll('.nav')){
  button.addEventListener('click',()=>{
    document.querySelectorAll('.nav').forEach(x=>x.classList.remove('active'));
    document.querySelectorAll('.panel').forEach(x=>x.classList.remove('active'));
    button.classList.add('active');$(button.dataset.tab).classList.add('active');$('page-title').textContent=labels[button.dataset.tab];
  });
}
const td=(row,txt,cls)=>{const cell=document.createElement('td');cell.textContent=txt||'—';if(cls)cell.className=cls;row.appendChild(cell)};
function renderEngines(){
  const q=$('engine-search').value.toLowerCase().trim();$('engine-rows').replaceChildren();
  const frag=document.createDocumentFragment();
  for(const e of engines){
    const searchable=`${e.name} ${e.id} ${e.origin_country} ${e.kind} ${e.result_sources.join(' ')}`.toLowerCase();
    if(!searchable.includes(q))continue;
    const tr=document.createElement('tr');td(tr,e.name);td(tr,e.kind,'dim');
    for(const c of ['sitemap','indexnow']){const v=e.capabilities[c];td(tr,v===true?'documented':v===false?'not used*':'unknown',v===true?'yes':'unknown')}
    td(tr,e.verification==='documented'?'documented':'research needed',e.verification==='documented'?'yes':'unknown');tr.title=e.note;
    frag.appendChild(tr);
  }$('engine-rows').appendChild(frag);
}
function renderCountries(){
  const q=$('country-search').value.toLowerCase().trim();$('country-rows').replaceChildren();
  const frag=document.createDocumentFragment();
  for(const c of countries){if(!(`${c.name} ${c.code}`).toLowerCase().includes(q))continue;
    const tr=document.createElement('tr');td(tr,c.name);td(tr,c.code,'dim');td(tr,c.language_candidates.slice(0,4).map(x=>x.code).join(', '),'dim');td(tr,'not audited','unknown');frag.appendChild(tr);
  }$('country-rows').appendChild(frag);
}
async function loadExample(){try{const r=await fetch('/api/example');if(!r.ok)throw new Error('example unavailable');$('source').value=await r.text();$('compiler-status').textContent='example loaded. replace the .example domain before deployment.';}catch(ex){$('compiler-status').textContent=ex.message}}
$('engine-search').addEventListener('input',renderEngines);$('country-search').addEventListener('input',renderCountries);
$('load-example').addEventListener('click',loadExample);
$('compile').addEventListener('click',async()=>{
  const b=$('compile');b.disabled=true;b.textContent='compiling…';$('compiler-status').textContent='validating specification and generated SEO output…';
  try{
    const r=await fetch('/api/compile',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({source:$('source').value})});
    if(!r.ok){const err=await r.json();throw new Error(typeof err.detail==='string'?err.detail:JSON.stringify(err.detail||err))}
    const blob=await r.blob();const url=URL.createObjectURL(blob);const a=document.createElement('a');a.href=url;a.download='usel-output.zip';a.click();setTimeout(()=>URL.revokeObjectURL(url),4000);
    $('compiler-status').textContent=`compiled ${r.headers.get('X-USEL-Indexed-Pages')} indexable pages. static checks passed; actual search-engine indexing is unverified.`;
  }catch(ex){$('compiler-status').textContent='compile failed: '+ex.message}finally{b.disabled=false;b.textContent='compile & download'}
});
async function init(){try{
  const [s,e,c]=await Promise.all([fetch('/api/summary'),fetch('/api/engines'),fetch('/api/countries')]);
  if(!s.ok||!e.ok||!c.ok)throw new Error('API unavailable');
  const stats=await s.json();engines=await e.json();countries=await c.json();
  $('country-count').textContent=stats.countries_and_jurisdictions;
  $('engine-count').textContent=stats.engine_and_crawler_profiles;
  $('documented-count').textContent=stats.documented_profiles;renderEngines();renderCountries();await loadExample();
}catch(ex){$('country-count').textContent='error';$('compiler-status').textContent='API error: '+ex.message}}
init();
