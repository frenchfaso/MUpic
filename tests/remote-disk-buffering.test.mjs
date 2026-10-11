import test from 'node:test';
import assert from 'node:assert/strict';
import {RemoteDisk} from '../site/remote-disk.js';

const CHUNK=262144, BYTES=128*CHUNK;
const regions=[{start:CHUNK+4096,end:49*CHUNK-4096,streamStart:CHUNK+4096+65536},
  {start:65*CHUNK+4096,end:121*CHUNK-4096,streamStart:65*CHUNK+4096+65536}];
const deferred=()=>{let resolve;const promise=new Promise(ok=>{resolve=ok;});return {promise,resolve};};
function fixture({before=async()=>{},change=()=>{}}={}) {
  const calls=[];let active=0,peak=0;
  const fetcher=async(url,{headers,signal})=>{
    const [,startText,endText]=/^bytes=(\d+)-(\d+)$/.exec(headers.Range);
    const start=Number(startText),end=Number(endText),index=start/CHUNK;
    calls.push(index);active++;peak=Math.max(peak,active);
    try {
      await before(index,signal);
      if(signal.aborted)throw new DOMException('Cancelled','AbortError');
      const reply={status:206,data:new Uint8Array(end-start+1).fill(index)};change(reply,index);
      return new Response(reply.data,{status:reply.status,headers:{'Content-Range':`bytes ${start}-${end}/${BYTES}`,
        'Content-Length':String(reply.data.length),ETag:'"fixed"'}});
    } finally {active--;}
  };
  const disk=new RemoteDisk({url:'/disk',bytes:BYTES,chunkBytes:CHUNK,cacheBytes:64*CHUNK,regions},{fetcher});
  return {disk,calls,memory:new WebAssembly.Memory({initial:1}),get peak(){return peak;}};
}
async function read(f,offset) {
  const result=f.disk.read(offset,512,0,f.memory);
  if(result===1){await f.disk.settle();assert.equal(f.disk.read(offset,512,0,f.memory),0);}
  else assert.equal(result,0);
  assert.deepEqual(new Uint8Array(f.memory.buffer,0,512),new Uint8Array(512).fill(Math.floor(offset/CHUNK)));
}
async function drain(f){while(f.disk.inflight.size)await Promise.allSettled([...f.disk.inflight.values()]);}

test('browsing both movie headers never preloads either stream',async()=>{
  const f=fixture();
  await read(f,regions[0].start);await read(f,regions[1].start);await drain(f);
  assert.deepEqual(f.calls,[1,65]);assert.equal(f.disk.playback,null);f.disk.close();
});
test('page startup warms both movies without guest input and shares four download slots',async()=>{
  const f=fixture();await f.disk.preloadMovies();
  assert.equal(f.calls.length,48);assert.ok(f.peak<=4);
  assert.ok(f.disk.cache.size<=64);
  for(const first of [1,65])for(let i=first;i<first+24;i++)assert.ok(f.disk.cache.has(i));
  const calls=f.calls.length;await read(f,regions[1].streamStart);
  assert.equal(f.calls.length,calls,'already buffered playback needs no startup download');f.disk.close();
});
test('an early user selection waits only for its own movie, while the other keeps warming',async()=>{
  const other=deferred(),started=deferred();
  const f=fixture({before(index){if(index>=65){started.resolve();return other.promise;}}});
  const warm=f.disk.preloadMovies();await started.promise;
  assert.equal(f.disk.read(regions[0].streamStart,512,0,f.memory),1);
  await f.disk.settle();assert.equal(f.disk.read(regions[0].streamStart,512,0,f.memory),0);
  assert.equal(f.disk.movieWindows.get(regions[1]).initializing,true);
  other.resolve();await warm;f.disk.close();
});
test('first playback read waits for about nine seconds even when its sector is cached',async()=>{
  const gate=deferred(),started=deferred();let held=0;
  const f=fixture({before(index){if(index>1){if(++held===4)started.resolve();return gate.promise;}}});
  await read(f,regions[0].start);
  new Uint8Array(f.memory.buffer).fill(0xa5);
  assert.equal(f.disk.read(regions[0].streamStart,512,0,f.memory),1);
  let ready=false;const warm=f.disk.settle().then(()=>{ready=true;});
  await started.promise;
  assert.equal(ready,false);assert.ok(new Uint8Array(f.memory.buffer).every(b=>b===0xa5));
  assert.equal(f.peak,4,'startup downloads have bounded concurrency');
  gate.resolve();await warm;
  assert.deepEqual(f.calls,[1,...Array.from({length:23},(_,i)=>i+2)]);
  const buffered=(25*CHUNK-regions[0].streamStart)/CHUNK*20/(985248/(63*312));
  assert.ok(buffered>=8&&buffered<=10);
  await read(f,regions[0].streamStart);assert.equal(f.disk.playback.initializing,false);f.disk.close();
});
test('playback maintains its window and protects unread buffered chunks from cache eviction',async()=>{
  const f=fixture();await read(f,regions[0].streamStart);
  for(let i=0;i<90;i++)await f.disk.chunk(i);
  assert.ok(f.disk.cache.size<=64);
  for(let i=1;i<=24;i++)assert.ok(f.disk.cache.has(i),`unread chunk ${i} survives`);
  const before=f.calls.length;
  await read(f,2*CHUNK);await drain(f);
  assert.ok(f.disk.cache.has(25));assert.ok(f.calls.length<=before+1);
  assert.equal(f.disk.playback.through,25);assert.ok(f.disk.cache.size<=64);f.disk.close();
});
test('switching movies preloads only the selected stream and EOF caps the window',async()=>{
  const f=fixture();await read(f,regions[0].streamStart);await drain(f);
  assert.ok(f.calls.every(i=>i<49));
  await read(f,regions[1].start);const headers=f.calls.length;
  await drain(f);assert.equal(f.calls.length,headers);
  await read(f,regions[1].streamStart);await drain(f);
  assert.equal(f.disk.playback.region,regions[1]);assert.equal(f.disk.playback.through,88);
  await read(f,120*CHUNK);await drain(f);
  assert.equal(f.disk.playback.through,120);assert.ok(!f.calls.includes(121));
  await read(f,regions[1].streamStart);assert.equal(f.disk.playback.first,65);f.disk.close();
});
test('failure in the startup window leaves guest RAM untouched',async()=>{
  const f=fixture({change(reply,index){if(index===10)reply.status=404;}});
  new Uint8Array(f.memory.buffer).fill(0xa5);
  assert.equal(f.disk.read(regions[0].streamStart,512,0,f.memory),1);
  await assert.rejects(f.disk.settle(),/Range/);
  assert.ok(new Uint8Array(f.memory.buffer).every(b=>b===0xa5));f.disk.close();await drain(f);
});
test('closing during prebuffering aborts requests and launches no further downloads',async()=>{
  const started=deferred();let held=0;
  const f=fixture({before(index,signal){if(index>1){if(++held===4)started.resolve();return new Promise((ok,bad)=>
    signal.addEventListener('abort',()=>bad(new DOMException('Cancelled','AbortError')),{once:true}));}}});
  assert.equal(f.disk.read(regions[0].streamStart,512,0,f.memory),1);
  const warm=f.disk.settle();await started.promise;const count=f.calls.length;
  f.disk.close();await assert.rejects(warm,{name:'AbortError'});await drain(f);
  assert.equal(f.calls.length,count);assert.equal(f.disk.controllers.size,0);
});
