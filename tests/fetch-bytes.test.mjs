import test from 'node:test';
import assert from 'node:assert/strict';
import {fetchBytes} from '../site/fetch-bytes.mjs';
const baseUrl='https://example.test/MUpic/worker.js';
test('boot assets resolve relative to the project Pages path and retain every streamed byte',async()=>{
  const progress=[];
  const bytes=await fetchBytes('data/firmware.ue2',(loaded,total)=>progress.push([loaded,total]),{baseUrl,fetcher:async(url,options)=>{
    assert.equal(url,'https://example.test/MUpic/data/firmware.ue2');assert.equal(options.cache,'no-store');
    return new Response(new ReadableStream({start(stream){stream.enqueue(Uint8Array.of(1,2));stream.enqueue(Uint8Array.of(3,4,5));stream.close();}}),{headers:{'Content-Length':'5'}});
  }});
  assert.deepEqual([...bytes],[1,2,3,4,5]);assert.deepEqual(progress.at(-1),[5,5]);
});
test('initial network failures identify the asset URL and request stage',async()=>{
  await assert.rejects(fetchBytes('data/firmware.ue2',()=>{},{baseUrl,fetcher:async()=>{throw new TypeError('Failed to fetch');}}),
    /MUpic\/data\/firmware\.ue2 \(request, 0 bytes\): Failed to fetch/);
});
test('interrupted HTTP200 bodies identify the asset and bytes received',async()=>{
  let reads=0;
  await assert.rejects(fetchBytes('data/flash.bin',()=>{},{baseUrl,fetcher:async()=>({ok:true,headers:new Headers(),body:{getReader(){return {async read(){if(!reads++){return {value:Uint8Array.of(1,2,3),done:false};}throw new TypeError('Failed to fetch');}};}}})}),
    /MUpic\/data\/flash\.bin \(response body, 3 bytes\): Failed to fetch/);
});
test('HTTP failure context includes the requested asset and server status',async()=>{
  await assert.rejects(fetchBytes('data/mupic.prg',()=>{},{baseUrl,fetcher:async()=>new Response(null,{status:404})}),/mupic\.prg.*HTTP 404/);
});
