import test from 'node:test';
import assert from 'node:assert/strict';
import {gzipSync,gunzipSync} from 'node:zlib';
import {inScope,isolatedResponse,ensureIsolation} from '../site/isolation.mjs';

const url=new URL('https://example.test/MUpic/isolation.mjs?v=release');
function browser({isolated=false,controlled=false,storage=true}={}) {
  const sw=new EventTarget(),values=new Map(),timers=new Map(),calls=[];
  let next=0,reloads=0;
  sw.controller=controlled?{scriptURL:url.href}:null;
  sw.register=async(...args)=>{calls.push(args);return {};};
  const env={crossOriginIsolated:isolated,isSecureContext:true,navigator:{serviceWorker:sw},
    location:{reload(){reloads++;}},
    sessionStorage:{getItem:key=>values.get(key),setItem(key,value){
      if(!storage)throw new Error('Storage denied');values.set(key,value);
    },removeItem:key=>values.delete(key)},
    setTimeout(fn){timers.set(++next,fn);return next;},clearTimeout:id=>timers.delete(id)};
  return {env,sw,values,timers,calls,get reloads(){return reloads;},
    claim(){sw.controller={scriptURL:url.href};sw.dispatchEvent(new Event('controllerchange'));}};
}
const turn=()=>new Promise(resolve=>setImmediate(resolve));

test('isolates streamed gzip and range responses without changing bytes or metadata',async()=>{
  const original=Buffer.from('MUP movie block'.repeat(1024)),packed=gzipSync(original);
  let sent=false;
  const body=new ReadableStream({pull(controller){
    if(!sent){controller.enqueue(packed);sent=true;}else controller.close();
  }});
  const response=new Response(body,{status:206,statusText:'Partial Content',headers:{
    'Content-Type':'application/octet-stream','Content-Length':String(packed.length),
    'Content-Range':`bytes 0-${packed.length-1}/${packed.length}`,'ETag':'"immutable"',
    'Cache-Control':'public, max-age=31536000, immutable',
  }});
  const wrapped=isolatedResponse(response);
  assert.equal(response.bodyUsed,false);
  assert.equal(wrapped.status,206);assert.equal(wrapped.statusText,'Partial Content');
  for(const [name,value] of response.headers)assert.equal(wrapped.headers.get(name),value);
  assert.equal(wrapped.headers.get('Cross-Origin-Opener-Policy'),'same-origin');
  assert.equal(wrapped.headers.get('Cross-Origin-Embedder-Policy'),'require-corp');
  assert.equal(wrapped.headers.get('Cross-Origin-Resource-Policy'),'same-origin');
  const received=Buffer.from(await wrapped.arrayBuffer());
  assert.deepEqual(received,packed);assert.deepEqual(gunzipSync(received),original);
  assert.equal(isolatedResponse(Response.error()).status,0);
  assert.equal(isolatedResponse(new Response(null,{status:204})).status,204);
});
test('the Pages scope excludes other projects, cross-origin requests and writes',()=>{
  const scope='https://example.test/MUpic/';
  assert.ok(inScope(new Request(scope+'worker.js?v=123'),scope));
  assert.ok(inScope(new Request(scope+'convert.html'),scope));
  for(const path of ['/Other/worker.js','/MUpic-other/index.html','/MUpic/../Other/a'])
    assert.equal(inScope(new Request('https://example.test'+path),scope),false);
  assert.equal(inScope(new Request('https://other.test/MUpic/a'),scope),false);
  assert.equal(inScope(new Request(scope+'a',{method:'POST',body:'x'}),scope),false);
  assert.equal(inScope({url:scope+'a',method:'GET',cache:'only-if-cached',mode:'no-cors'},scope),false);
});
test('native headers start immediately without installing a worker',async()=>{
  const b=browser({isolated:true});
  assert.equal(await ensureIsolation(b.env,url),true);
  assert.equal(b.calls.length,0);assert.equal(b.reloads,0);
});
test('a fresh Pages visit waits for its own controller, guards reload and never starts the old document',async()=>{
  const b=browser();let settled=false;
  ensureIsolation(b.env,url).then(()=>{settled=true;});await turn();
  assert.equal(b.calls[0][0].href,url.href);
  assert.deepEqual(b.calls[0][1],{scope:'https://example.test/MUpic/',type:'module',updateViaCache:'none'});
  assert.equal(b.reloads,0);assert.equal(settled,false);
  b.sw.controller={scriptURL:'https://example.test/Other/worker.js'};
  b.sw.dispatchEvent(new Event('controllerchange'));await turn();assert.equal(b.reloads,0);
  b.claim();await turn();
  assert.equal(b.reloads,1);assert.equal(settled,false);
  assert.equal(b.values.size,1);assert.equal(b.timers.size,0);
  assert.equal(await ensureIsolation(b.env,url),false);assert.equal(b.reloads,1);
});
test('isolated repeat visits refresh the worker without losing a loaded video',async()=>{
  const b=browser({isolated:true,controlled:true});
  b.values.set(`mupic-isolation:${url.href}`,'1');
  assert.equal(await ensureIsolation(b.env,url),true);
  assert.equal(b.calls.length,1);assert.equal(b.reloads,0);assert.equal(b.values.size,0);
});
test('storage rejection, registration failure and blocked isolation keep the serial fallback',async()=>{
  const denied=browser({controlled:true,storage:false});
  assert.equal(await ensureIsolation(denied.env,url),false);assert.equal(denied.reloads,0);
  const failed=browser();failed.sw.register=()=>Promise.reject(new Error('Offline'));
  assert.equal(await ensureIsolation(failed.env,url),false);assert.equal(failed.timers.size,0);
  const blocked=browser(),waiting=ensureIsolation(blocked.env,url);await turn();
  assert.equal(blocked.timers.size,1);
  [...blocked.timers.values()][0]();assert.equal(await waiting,false);
  blocked.claim();await turn();assert.equal(blocked.reloads,0);assert.equal(blocked.timers.size,0);
});
