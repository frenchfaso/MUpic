import test from 'node:test';
import assert from 'node:assert/strict';
import {gzipSync} from 'node:zlib';
import {createHash} from 'node:crypto';
import {RemoteDisk} from '../site/remote-disk.js';
const CHUNK=1024,BYTES=CHUNK*9+512;
const payload=index=>Uint8Array.from({length:Math.min(CHUNK,BYTES-index*CHUNK)},(_,i)=>((i>>4)+index*17)&255);
const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
function fixture({change=()=>{},manifestChange=()=>{}}={}) {
  const chunks=Array.from({length:10},(_,i)=>i===0?null:digest(payload(i)));
  const encoded=Array.from({length:10},(_,i)=>i===0?null:gzipSync(payload(i),{mtime:0}));
  const storedBytes=encoded.map(bytes=>bytes?.length||0);manifestChange({chunks,storedBytes});
  const calls=[];
  const fetcher=async(url,options)=>{
    const index=Number(/(\d+)\.bin\.gz$/.exec(url)[1]);calls.push({index,url,options});
    const reply={data:encoded[index],headers:{'Content-Length':String(storedBytes[index])},status:200};change(reply,index);
    return new Response(new ReadableStream({start(controller){
      for(let at=0;at<reply.data.length;at+=13)controller.enqueue(reply.data.subarray(at,at+13));controller.close();
    }}),{status:reply.status,headers:reply.headers});
  };
  const disk=new RemoteDisk({url:'/immutable/block-',bytes:BYTES,chunkBytes:CHUNK,cacheBytes:CHUNK*8,
    transport:'chunks',chunks,compression:'gzip',storedBytes},{fetcher});
  return {disk,calls,chunks,storedBytes,memory:new WebAssembly.Memory({initial:1})};
}
async function read(f,offset,length) {
  new Uint8Array(f.memory.buffer).fill(0xa7);
  assert.equal(f.disk.read(offset,length,19,f.memory),1);
  assert.ok(new Uint8Array(f.memory.buffer).every(byte=>byte===0xa7));
  await f.disk.settle();assert.equal(f.disk.read(offset,length,19,f.memory),0);
  return new Uint8Array(f.memory.buffer,19,length).slice();
}
test('genuine gzip chunks inflate byte-exactly including the short disk tail',async()=>{
  const f=fixture();assert.deepEqual(await read(f,CHUNK,CHUNK),payload(1));
  assert.equal(f.calls[0].url,'/immutable/block-0001.bin.gz');assert.equal(f.calls.length,1);
  assert.ok(f.storedBytes[1]<CHUNK);assert.deepEqual(await read(f,BYTES-512,512),payload(9));
  assert.deepEqual(f.calls.map(call=>call.index),[1,9]);f.disk.close();
});
test('compressed zero blocks require no fetch or decompression',async()=>{
  const f=fixture();assert.deepEqual(await read(f,0,CHUNK),new Uint8Array(CHUNK));
  assert.equal(f.calls.length,0);f.disk.close();
});
test('an unaligned read crosses zero and gzip chunks without changing guest bytes',async()=>{
  const f=fixture();assert.deepEqual(await read(f,CHUNK-37,200),new Uint8Array([...new Uint8Array(37),...payload(1).subarray(0,163)]));f.disk.close();
});
for(const [name,change,manifestChange,expected] of [
  ['compressed input truncation',r=>{r.data=r.data.subarray(0,r.data.length-1);},()=>{},/truncated|unexpected end|decompress/i],
  ['compressed input overflow',r=>{r.data=Buffer.concat([r.data,Buffer.of(0)]);},()=>{},/too large/i],
  ['gzip CRC corruption',r=>{r.data=Buffer.from(r.data);r.data[r.data.length-8]^=1;},()=>{},/data check|decompress|invalid|CRC/i],
  ['wrong inflated SHA256',()=>{},m=>{m.chunks[1]='f'.repeat(64);},/SHA-256/i],
  ['short inflated chunk',r=>{r.data=gzipSync(payload(1).subarray(0,CHUNK-1));r.headers['Content-Length']=String(r.data.length);},m=>{m.storedBytes[1]=gzipSync(payload(1).subarray(0,CHUNK-1)).length;},/truncated/i],
  ['decompression bomb',r=>{r.data=gzipSync(new Uint8Array(CHUNK*1024));r.headers['Content-Length']=String(r.data.length);},m=>{m.storedBytes[1]=gzipSync(new Uint8Array(CHUNK*1024)).length;},/too large/i],
  ['transparent server decompression',r=>{r.headers['Content-Encoding']='gzip';},()=>{},/Content-Encoding/i],
  ['wrong compressed Content-Length',r=>{r.headers['Content-Length']='1';},()=>{},/stored length/i],
]) test(`${name} fails closed before modifying guest RAM`,async()=>{
  const f=fixture({change,manifestChange});new Uint8Array(f.memory.buffer).fill(0x8b);
  assert.equal(f.disk.read(CHUNK,512,0,f.memory),1);await assert.rejects(f.disk.settle(),expected);
  assert.ok(new Uint8Array(f.memory.buffer).every(byte=>byte===0x8b));assert.equal(f.disk.cache.size,0);
  assert.equal(f.disk.read(CHUNK,512,0,f.memory),-1);f.disk.close();
});
test('compressed chunk size table rejects holes, invalid zero lengths and unbounded input sizes',()=>{
  for(const change of [m=>{m.storedBytes.length=0;},m=>{m.storedBytes[0]=1;},m=>{m.storedBytes[1]=0;},m=>{delete m.storedBytes[1];},m=>{m.storedBytes[1]=CHUNK+65537;}])
    assert.throws(()=>fixture({manifestChange:change}),/compressed disk chunk lengths/);
});
test('a browser without DecompressionStream gets an actionable error before fetching',()=>{
  const original=globalThis.DecompressionStream;
  try{globalThis.DecompressionStream=undefined;assert.throws(()=>fixture(),/update your browser|native player/);}
  finally{globalThis.DecompressionStream=original;}
});
