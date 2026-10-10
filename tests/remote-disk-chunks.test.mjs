import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {RemoteDisk} from '../site/remote-disk.js';
const CHUNK=1024,BYTES=CHUNK*10+512;
const payload=index=>Uint8Array.from({length:Math.min(CHUNK,BYTES-index*CHUNK)},(_,i)=>(index*71+i*13)&255);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const chunks=Array.from({length:Math.ceil(BYTES/CHUNK)},(_,i)=>i===0||i===2?null:hash(payload(i)));
function fixture({change=()=>{},manifest=chunks,bytes=BYTES}={}) {
  const calls=[];
  const fetcher=async(url,options)=>{
    const index=Number(/(\d+)\.bin$/.exec(url)[1]);calls.push({url,options,index});
    const reply={data:payload(index),status:200};change(reply,index);
    return new Response(new ReadableStream({start(controller){controller.enqueue(reply.data.subarray(0,173));controller.enqueue(reply.data.subarray(173));controller.close();}}),{status:reply.status});
  };
  const disk=new RemoteDisk({url:'/immutable/disk-',bytes,chunkBytes:CHUNK,cacheBytes:CHUNK*8,transport:'chunks',chunks:manifest},{fetcher});
  return {disk,calls,memory:new WebAssembly.Memory({initial:1})};
}
async function read(f,offset,length,destination=17) {
  new Uint8Array(f.memory.buffer).fill(0xa5);
  assert.equal(f.disk.read(offset,length,destination,f.memory),1);
  assert.ok(new Uint8Array(f.memory.buffer).every(byte=>byte===0xa5));
  await f.disk.settle();assert.equal(f.disk.read(offset,length,destination,f.memory),0);
  return new Uint8Array(f.memory.buffer,destination,length).slice();
}
test('zero chunks synthesize exact disk bytes without any HTTP request',async()=>{
  const f=fixture();assert.deepEqual(await read(f,0,CHUNK),new Uint8Array(CHUNK));
  assert.equal(f.calls.length,0);f.disk.close();
});
test('hash-authenticated chunks preserve unaligned zero/data boundary and short disk tail',async()=>{
  const f=fixture();const boundary=await read(f,CHUNK-73,200);
  assert.deepEqual(boundary,new Uint8Array([...new Uint8Array(73),...payload(1).subarray(0,127)]));
  assert.equal(f.calls.length,1);assert.equal(f.calls[0].url,'/immutable/disk-0001.bin');
  assert.deepEqual(f.calls[0].options.headers,{});assert.equal(f.calls[0].options.cache,'force-cache');
  const tail=await read(f,BYTES-512,512);assert.deepEqual(tail,payload(10));
  assert.deepEqual(f.calls.map(call=>call.index),[1,10]);f.disk.close();
});
for(const [label,change,expected] of [
  ['SHA-256 mismatch',reply=>{reply.data=reply.data.slice();reply.data[100]^=1;},/SHA-256/],
  ['truncated chunk',reply=>{reply.data=reply.data.subarray(0,CHUNK-1);},/truncated/],
  ['oversized chunk',reply=>{reply.data=new Uint8Array(CHUNK+1);},/too large/],
  ['missing chunk',reply=>{reply.status=404;},/404/],
]) test(`${label} leaves guest memory unchanged and becomes a fatal disk error`,async()=>{
  const f=fixture({change});new Uint8Array(f.memory.buffer).fill(0x91);
  assert.equal(f.disk.read(CHUNK,512,100,f.memory),1);await assert.rejects(f.disk.settle(),expected);
  assert.ok(new Uint8Array(f.memory.buffer).every(byte=>byte===0x91));assert.equal(f.disk.cache.size,0);
  assert.equal(f.disk.read(CHUNK,512,100,f.memory),-1);f.disk.close();
});
test('malformed, sparse or wrong-sized immutable chunk manifests are rejected',()=>{
  for(const manifest of [[],Array(chunks.length),chunks.map((h,i)=>i===0?undefined:h),chunks.map((h,i)=>i===0?'G'.repeat(64):h)])
    assert.throws(()=>fixture({manifest}),/immutable disk chunks/);
});
test('negative and fractional guest destinations fail without network or exceptions',()=>{
  for(const destination of [-1,0.5,Number.MAX_SAFE_INTEGER]) {
    const f=fixture();assert.equal(f.disk.read(0,512,destination,f.memory),-1);assert.equal(f.calls.length,0);f.disk.close();
  }
});
