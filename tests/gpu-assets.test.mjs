import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {checkedBytes,mapConcurrent} from '../site/gpu.js';
test('CRT asset downloads retain URL context for request and interrupted-body errors',async()=>{
  const original=globalThis.fetch;let reads=0;
  try {
    globalThis.fetch=async()=>{throw new TypeError('Failed to fetch');};
    await assert.rejects(checkedBytes(new URL('https://example.test/MUpic/crt/pass-0.wgsl'),{}),/crt\/pass-0\.wgsl.*request.*Failed to fetch/);
    globalThis.fetch=async()=>({ok:true,headers:new Headers(),body:{getReader(){return {async read(){if(!reads++)return {value:Uint8Array.of(1,2),done:false};throw new TypeError('Failed to fetch');}};}}});
    await assert.rejects(checkedBytes(new URL('https://example.test/MUpic/crt/lut.rgba'),{}),/crt\/lut\.rgba.*response body, 2 bytes.*Failed to fetch/);
  } finally {globalThis.fetch=original;}
});
test('CRT assets still require exact native bytes and SHA256',async()=>{
  const original=globalThis.fetch,data=Uint8Array.of(1,2,3,4),url=new URL('https://example.test/MUpic/crt/test.wgsl');
  try {
    globalThis.fetch=async()=>new Response(data);const sha256=createHash('sha256').update(data).digest('hex');
    assert.deepEqual(new Uint8Array(await checkedBytes(url,{bytes:4,sha256})),data);
    await assert.rejects(checkedBytes(url,{bytes:5,sha256}),/size mismatch/);
    await assert.rejects(checkedBytes(url,{bytes:4,sha256:'0'.repeat(64)}),/hash mismatch/);
  } finally {globalThis.fetch=original;}
});
test('bounded startup scheduling limits active downloads and preserves native asset order',async()=>{
  let active=0,peak=0;
  const result=await mapConcurrent(Array.from({length:42},(_,i)=>i),6,async index=>{
    active++;peak=Math.max(peak,active);await new Promise(resolve=>setTimeout(resolve,index%4));active--;return index*7;
  });
  assert.equal(peak,6);assert.equal(active,0);assert.deepEqual(result,Array.from({length:42},(_,i)=>i*7));
  await assert.rejects(mapConcurrent([1],0,async()=>{}),/concurrency/);
});
