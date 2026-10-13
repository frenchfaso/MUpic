import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {gzipSync} from 'node:zlib';
import {LutCache,loadLutAsset} from '../site/lut-cache.mjs';
import {QuantizerPool} from '../site/worker-pool.mjs';
const IDENTITY='a'.repeat(64),OTHER_IDENTITY='b'.repeat(64),MANIFEST_URL='https://example.test/assets/rgb24-lut.json';
const RAW_BYTES=128+16777216*3,sha=value=>createHash('sha256').update(value).digest('hex');
function fixture() {
  const raw=new Uint8Array(RAW_BYTES),compressed=new Uint8Array(gzipSync(raw));
  const manifest={schema:1,algorithm_identity:IDENTITY,asset:'rgb24-lut.bin.gz',compression:'gzip',
    compressed_bytes:compressed.length,compressed_sha256:sha(compressed),uncompressed_bytes:RAW_BYTES,uncompressed_sha256:sha(raw)};
  return {compressed,manifest};
}
const ASSET=fixture();
class MemoryIndexedDB {
  constructor(){this.databases=new Map();this.failWrites=false;}
  open(name,version) {
    const request={};queueMicrotask(()=>{
      const old=this.databases.get(name),upgrade=!old||old.version<version;
      if(!old)this.databases.set(name,{stores:new Map(),version,tail:Promise.resolve()});
      const record=this.databases.get(name),factory=this;record.version=version;
      request.result={closed:false,get objectStoreNames(){return [...record.stores.keys()];},
        createObjectStore(store){record.stores.set(store,new Map());},deleteObjectStore(store){record.stores.delete(store);},close(){this.closed=true;},
        transaction(stores,mode){
          if(this.closed)throw new DOMException('Closed','InvalidStateError');
          const operation={pending:[],aborted:false,abort(){this.aborted=true;},objectStore(store){
            const enqueue=action=>{const result={};operation.pending.push(()=>{result.result=action();result.onsuccess?.();});return result;};
            return {get(key){return enqueue(()=>structuredClone(operation.snapshot.get(store).get(key)));},
              put(value,key){return enqueue(()=>{
                if(mode!=='readwrite')throw new Error('Read-only');if(factory.failWrites)throw new DOMException('Quota exceeded','QuotaExceededError');
                operation.snapshot.get(store).set(key,structuredClone(value));
              });},delete(key){return enqueue(()=>operation.snapshot.get(store).delete(key));}};
          }};
          record.tail=record.tail.then(()=>{
            operation.snapshot=new Map(stores.map(store=>[store,new Map(record.stores.get(store))]));
            try {
              while(operation.pending.length&&!operation.aborted)operation.pending.shift()();
              if(operation.aborted){operation.onabort?.();return;}
              if(mode==='readwrite')for(const store of stores)record.stores.set(store,operation.snapshot.get(store));operation.oncomplete?.();
            }catch(error){operation.error=error;operation.onabort?.();}
          });return operation;
        },
      };
      if(upgrade)request.onupgradeneeded?.();request.onsuccess?.();
    });return request;
  }
}
function network({manifest=ASSET.manifest,compressed=ASSET.compressed,offline=false,status=200}={}) {
  const requests=[];
  return {requests,async fetch(url){
    requests.push(String(url));
    if(offline)throw new TypeError('Offline');
    if(String(url).includes('rgb24-lut.json'))return new Response(JSON.stringify(manifest),{status});
    return new Response(compressed,{status});
  }};
}
class Importer {
  constructor(){this.started=0;this.finished=0;this.aborted=0;this.active=false;}
  async begin(header){assert.equal(header.length,128);this.started++;this.hash=createHash('sha256').update(header);this.bytes=header.length;}
  async append(chunk){assert.ok(chunk.length<=65536);this.hash.update(chunk);this.bytes+=chunk.length;}
  async finish(expected){assert.equal(this.bytes,RAW_BYTES);assert.equal(this.hash.digest('hex'),expected);this.finished++;this.active=true;}
  async abort(){this.aborted++;this.active=false;}
}
const cacheOptions=factory=>({name:'test-lut',indexedDB:factory});
const load=(importer,factory,source,options={})=>loadLutAsset(MANIFEST_URL,IDENTITY,importer,{cacheOptions:cacheOptions(factory),fetch:source.fetch,...options});

test('the verified compressed asset is streamed once and reused without downloading on reload',async()=>{
  const factory=new MemoryIndexedDB(),first=network(),importer=new Importer(),progress=[];
  assert.deepEqual(await load(importer,factory,first,{onProgress:state=>progress.push(state)}),{cached:false,saved:true});
  assert.equal(importer.finished,1);assert.equal(first.requests.length,2);assert.ok(progress.some(state=>state.phase==='downloading'));
  assert.ok(progress.some(state=>state.phase==='decompressing'));assert.equal(progress.at(-1).phase,'validating');
  const cached=await LutCache.open(cacheOptions(factory)),record=await cached.read();cached.close();
  assert.equal(record.compressed.length,ASSET.compressed.length);assert.ok(record.compressed.length<RAW_BYTES);
  const second=network(),restored=new Importer();assert.deepEqual(await load(restored,factory,second),{cached:true,saved:true});
  assert.equal(second.requests.length,1);assert.equal(restored.finished,1);
});

test('saved manifest and compressed bytes remain usable when the network is offline',async()=>{
  const factory=new MemoryIndexedDB();await load(new Importer(),factory,network());
  const importer=new Importer(),offline=network({offline:true});assert.deepEqual(await load(importer,factory,offline),{cached:true,saved:true});
  assert.equal(importer.finished,1);assert.equal(offline.requests.length,1);
});

test('a corrupt compressed checkpoint is replaced after a new download passes validation',async()=>{
  const factory=new MemoryIndexedDB();await load(new Importer(),factory,network());
  const cache=await LutCache.open(cacheOptions(factory)),record=await cache.read();record.compressed[record.compressed.length-1]^=1;
  await cache.write(record);cache.close();
  const source=network(),importer=new Importer();assert.deepEqual(await load(importer,factory,source),{cached:false,saved:true});
  assert.equal(source.requests.length,2);assert.equal(importer.started,1);assert.equal(importer.finished,1);
});

test('a failed cached import preserves the checkpoint and stops retrying a dead coordinator',async()=>{
  const factory=new MemoryIndexedDB();await load(new Importer(),factory,network());
  const cache=await LutCache.open(cacheOptions(factory)),saved=await cache.read();cache.close();
  const importer=new Importer(),source=network();
  importer.begin=async()=>{throw new Error('The encoder worker stopped');};importer.available=()=>false;
  await assert.rejects(load(importer,factory,source),/worker stopped/);assert.equal(source.requests.length,1);
  const restored=await LutCache.open(cacheOptions(factory));assert.equal((await restored.read()).key,saved.key);restored.close();
});

test('a browser without gzip decompression keeps its valid cached asset',async()=>{
  const factory=new MemoryIndexedDB();await load(new Importer(),factory,network());
  const decompress=globalThis.DecompressionStream,source=network();
  try {
    globalThis.DecompressionStream=undefined;
    await assert.rejects(load(new Importer(),factory,source),/cannot decompress/);assert.equal(source.requests.length,1);
  }finally{globalThis.DecompressionStream=decompress;}
  const cache=await LutCache.open(cacheOptions(factory));assert.ok(await cache.read());cache.close();
});

test('a slow older tab cannot replace a newly saved asset',async()=>{
  const factory=new MemoryIndexedDB(),first=await LutCache.open(cacheOptions(factory));
  const old={key:'old',manifest:Uint8Array.of(1),compressed:Uint8Array.of(2)};
  assert.equal(await first.write(old,{expectedKey:null}),true);
  const second=await LutCache.open(cacheOptions(factory)),observed=await first.read();
  const updated={...old,key:'updated',compressed:Uint8Array.of(3)};
  assert.equal(await second.write(updated,{expectedKey:observed.key}),true);
  assert.equal(await first.write(old,{expectedKey:observed.key}),false);
  assert.equal((await first.read()).key,'updated');
  assert.equal(await second.write(updated,{expectedKey:'old'}),true);
  first.close();second.close();
});

test('a malformed IndexedDB record can be replaced without bypassing the conditional write',async()=>{
  const factory=new MemoryIndexedDB(),cache=await LutCache.open(cacheOptions(factory));
  await cache.write({key:'invalid',manifest:new Uint8Array(),compressed:Uint8Array.of(1)});cache.close();
  assert.deepEqual(await load(new Importer(),factory,network()),{cached:false,saved:true});
  const restored=await LutCache.open(cacheOptions(factory)),record=await restored.read();
  assert.ok(record);assert.notEqual(record.key,'invalid');restored.close();
});

test('a new asset checksum under the same algorithm replaces the previous cached asset',async()=>{
  const factory=new MemoryIndexedDB();await load(new Importer(),factory,network());
  const compressed=new Uint8Array(gzipSync(new Uint8Array(RAW_BYTES),{level:1}));
  const manifest={...ASSET.manifest,compressed_bytes:compressed.length,compressed_sha256:sha(compressed)};
  assert.notEqual(manifest.compressed_sha256,ASSET.manifest.compressed_sha256);
  const changed=network({manifest,compressed});assert.deepEqual(await load(new Importer(),factory,changed),{cached:false,saved:true});
  assert.equal(changed.requests.length,2);
  const restored=network({manifest,compressed});assert.deepEqual(await load(new Importer(),factory,restored),{cached:true,saved:true});
  assert.equal(restored.requests.length,1);
});

test('manifest size, namespace and URL constraints reject invalid assets before downloading',async()=>{
  for(const changed of [{uncompressed_bytes:RAW_BYTES+1},{compressed_bytes:65*1048576+1},
    {algorithm_identity:OTHER_IDENTITY},{asset:'https://other.test/table.gz'},{compression:'br'}]) {
    const source=network({manifest:{...ASSET.manifest,...changed}}),importer=new Importer();
    await assert.rejects(load(importer,new MemoryIndexedDB(),source),/does not match/);
    assert.equal(source.requests.length,1);assert.equal(importer.started,0);
  }
});

test('bad or truncated downloads and invalid Rust validation never become cached tables',async()=>{
  const bad=ASSET.compressed.slice();bad[20]^=1;
  for(const compressed of [bad,ASSET.compressed.slice(0,-1)]) {
    const factory=new MemoryIndexedDB(),importer=new Importer();await assert.rejects(load(importer,factory,network({compressed})),/checksum|incomplete/);
    assert.equal(importer.active,false);const cache=await LutCache.open(cacheOptions(factory));assert.equal(await cache.read(),null);cache.close();
  }
  const importer=new Importer();importer.begin=async()=>{throw new Error('Rust rejected the header');};
  await assert.rejects(load(importer,new MemoryIndexedDB(),network()),/rejected the header/);assert.equal(importer.aborted,1);
  const mismatch=new Importer();
  await assert.rejects(load(mismatch,new MemoryIndexedDB(),network({manifest:{...ASSET.manifest,uncompressed_sha256:'0'.repeat(64)}})),error=>error.code==='ERR_ASSERTION');
  assert.equal(mismatch.finished,0);assert.equal(mismatch.aborted,1);assert.equal(mismatch.active,false);
});

test('denied persistence and quota do not prevent a validated table from serving the current page',async()=>{
  const denied={open(){throw new DOMException('Denied','SecurityError');}},first=new Importer();
  assert.deepEqual(await load(first,denied,network()),{cached:false,saved:false});assert.equal(first.finished,1);
  const factory=new MemoryIndexedDB();factory.failWrites=true;const second=new Importer();
  assert.deepEqual(await load(second,factory,network()),{cached:false,saved:false});assert.equal(second.finished,1);
});

test('cancellation releases the staging importer and does not store an incomplete asset',async()=>{
  const factory=new MemoryIndexedDB(),controller=new AbortController(),importer=new Importer();
  const append=importer.append.bind(importer);importer.append=async chunk=>{await append(chunk);controller.abort();};
  await assert.rejects(load(importer,factory,network(),{signal:controller.signal}),/cancelled/);
  assert.equal(importer.active,false);assert.equal(importer.finished,0);assert.equal(importer.aborted,1);
  const cache=await LutCache.open(cacheOptions(factory));assert.equal(await cache.read(),null);cache.close();
});

test('cache schema upgrade removes the old block stores and keeps a single current compressed record',async()=>{
  const factory=new MemoryIndexedDB();factory.databases.set('test-lut',{version:1,stores:new Map([['metadata',new Map()],['blocks',new Map([[0,new Uint8Array(16464)]])]]),tail:Promise.resolve()});
  await load(new Importer(),factory,network());const database=factory.databases.get('test-lut');
  assert.deepEqual([...database.stores.keys()],['asset']);assert.equal(database.stores.get('asset').size,1);
  const changed={...ASSET.manifest,algorithm_identity:OTHER_IDENTITY};
  assert.deepEqual(await loadLutAsset(MANIFEST_URL,OTHER_IDENTITY,new Importer(),{cacheOptions:cacheOptions(factory),fetch:network({manifest:changed}).fetch}),{cached:false,saved:true});
  assert.equal(database.stores.get('asset').size,1);
});

class FakeWorker {
  static live=[];
  constructor(){this.closed=false;this.ready=false;this.loading=false;this.calls=[];FakeWorker.live.push(this);}
  postMessage({id,method,args}) {
    this.calls.push(method);let result;
    if(method==='init')result={ready:true};
    else if(method==='lutIdentity')result=IDENTITY;
    else if(method==='lutInfo')result={blockCount:4096,loadedBlocks:this.ready?4096:0,totalColors:16777216};
    else if(method==='beginLutAsset'){assert.equal(args.header.length,128);this.loading=true;}
    else if(method==='appendLutAsset'){assert.equal(this.loading,true);assert.ok(args.chunk.length<=65536);}
    else if(method==='finishLutAsset'){assert.equal(args.expectedSha,ASSET.manifest.uncompressed_sha256);this.ready=true;this.loading=false;}
    else if(method==='abortLutAsset')this.loading=false;
    else if(method==='prepare')result=(this.ready?new Uint32Array():Uint32Array.of(1,2,3)).buffer;
    else if(method==='plan')result=new Uint32Array(args.keys).map(()=>7).buffer;
    else if(method==='pack')result=[Uint8Array.of(42).buffer];
    else if(method==='quantizeBatch')result=args.frames.map(()=>Uint8Array.of(42).buffer);
    else throw new Error(`Unexpected RPC: ${method}`);
    queueMicrotask(()=>{if(!this.closed)this.onmessage({data:{id,result}});});
  }
  terminate(){this.closed=true;}
}
globalThis.Worker=FakeWorker;
test('a ready LUT keeps one coordinator across conversions and fallback workers are created only for searches',async()=>{
  FakeWorker.live=[];const pool=new QuantizerPool(4,'worker.mjs');await pool.init('encoder.wasm');assert.equal(FakeWorker.live.length,1);
  await pool.loadLut(MANIFEST_URL,{cacheOptions:cacheOptions(new MemoryIndexedDB()),fetch:network().fetch});
  const owner=pool.workers[0];await pool.batch([Uint8Array.of(1)]);await pool.batch([Uint8Array.of(2)]);
  assert.equal(pool.lutReady,true);assert.equal(pool.workers[0],owner);assert.equal(FakeWorker.live.length,1);assert.equal(owner.worker.calls.includes('plan'),false);pool.close();
  const fallback=new QuantizerPool(4,'worker.mjs');await fallback.init('encoder.wasm');await fallback.batch([Uint8Array.of(1)]);
  assert.equal(fallback.workers.length,4);fallback.close();
});

test('a stopped coordinator after LUT load requests a reload rather than reporting cancellation',async()=>{
  const pool=new QuantizerPool(2,'worker.mjs');await pool.init('encoder.wasm');
  await pool.loadLut(MANIFEST_URL,{cacheOptions:cacheOptions(new MemoryIndexedDB()),fetch:network().fetch});
  pool.workers[0].worker.onerror({message:'Unexpected worker exit'});
  assert.throws(()=>pool.checkOwner(),error=>error.name==='Error'&&/Retry loading the color table/.test(error.message));
  assert.equal(pool.closed,true);
});
test('shared allocation failure preserves the verified LUT and serial encoding',async()=>{
  const pool=new QuantizerPool(3,'worker.mjs',{shared:true});await pool.init('encoder.wasm');
  const worker=pool.workers[0].worker,send=worker.postMessage.bind(worker);
  worker.postMessage=message=>{
    if(message.method==='shareLut')queueMicrotask(()=>worker.onmessage({data:{id:message.id,error:'Shared buffer allocation failed'}}));
    else send(message);
  };
  await pool.loadLut(MANIFEST_URL,{cacheOptions:cacheOptions(new MemoryIndexedDB()),fetch:network().fetch});
  assert.equal(pool.lutReady,true);assert.equal(pool.shared,false);assert.equal(pool.sharedLut,null);
  assert.deepEqual(new Uint8Array((await pool.batch([Uint8Array.of(1)]))[0]),Uint8Array.of(42));
  assert.equal(pool.workers.length,1);pool.close();
});

test('worker death during cached LUT load preserves storage and avoids a second network request',async()=>{
  const factory=new MemoryIndexedDB();await load(new Importer(),factory,network());
  const pool=new QuantizerPool(2,'worker.mjs');await pool.init('encoder.wasm');
  const owner=pool.workers[0].worker,send=owner.postMessage.bind(owner),source=network();
  owner.postMessage=message=>{
    if(message.method==='beginLutAsset')owner.onerror({message:'Unexpected worker exit'});else send(message);
  };
  await assert.rejects(pool.loadLut(MANIFEST_URL,{cacheOptions:cacheOptions(factory),fetch:source.fetch}),/Unexpected worker exit/);
  assert.equal(source.requests.length,1);assert.equal(pool.loadingController,null);
  const cache=await LutCache.open(cacheOptions(factory));assert.ok(await cache.read());cache.close();pool.close();
});
