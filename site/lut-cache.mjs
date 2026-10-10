const DATABASE='mupic-rgb24-lut',MANIFEST_LIMIT=16384,COMPRESSED_LIMIT=65*1048576;
const DOMAIN=16777216,HEADER_BYTES=128,RAW_SIZES=new Set([HEADER_BYTES+DOMAIN*3,HEADER_BYTES+DOMAIN*4]);
const hex=bytes=>Array.from(bytes,value=>value.toString(16).padStart(2,'0')).join('');
const check=signal=>{if(signal?.aborted)throw new DOMException('Color table loading was cancelled.','AbortError');};
const bytes=value=>value instanceof Uint8Array?value:value instanceof ArrayBuffer?new Uint8Array(value):null;
const recordKey=record=>typeof record?.key==='string'?record.key:null;
async function sha256(value){return hex(new Uint8Array(await crypto.subtle.digest('SHA-256',value)));}
function transaction(database,mode,run) {
  return new Promise((resolve,reject)=>{
    let operation,result;
    try {
      operation=database.transaction(['asset'],mode);
      operation.oncomplete=()=>resolve(result);
      operation.onabort=operation.onerror=()=>reject(operation.error||new Error('Browser storage is unavailable.'));
      result=run(operation.objectStore('asset'));
    }catch(error){try{operation?.abort();}catch{}reject(error);}
  });
}
/** One compressed, verified asset replaces older versions atomically. */
export class LutCache {
  static async open({name=DATABASE,indexedDB=globalThis.indexedDB}={}) {
    if(!indexedDB)return null;let database;
    try {
      database=await new Promise((resolve,reject)=>{
        const request=indexedDB.open(name,2);let blocked=false;
        request.onupgradeneeded=()=>{
          const current=request.result;
          for(const store of Array.from(current.objectStoreNames))current.deleteObjectStore(store);
          current.createObjectStore('asset');
        };
        request.onerror=()=>reject(request.error);
        request.onblocked=()=>{blocked=true;reject(new Error('Browser storage is unavailable.'));};
        request.onsuccess=()=>{if(blocked)request.result.close();else resolve(request.result);};
      });
      return new LutCache(database);
    }catch{database?.close();return null;}
  }
  constructor(database){this.database=database;this.closed=false;database.onversionchange=()=>this.close();}
  async read() {
    if(this.closed)return null;
    try {
      const state=await transaction(this.database,'readonly',store=>{
        const result={record:null},request=store.get('current');
        request.onsuccess=()=>{result.record=request.result;};return result;
      });
      const record=state.record,manifest=bytes(record?.manifest),compressed=bytes(record?.compressed);
      this.observedKey=recordKey(record);
      return manifest?.length&&manifest.length<=MANIFEST_LIMIT&&compressed?.length&&compressed.length<=COMPRESSED_LIMIT?
        {...record,manifest,compressed}:null;
    }catch{this.close();return null;}
  }
  async write(record,{expectedKey}={}) {
    if(this.closed)return false;
    try {
      const state=await transaction(this.database,'readwrite',store=>{
        const result={written:false},request=store.get('current');
        request.onsuccess=()=>{
          const currentKey=recordKey(request.result);
          if(expectedKey===undefined||currentKey===expectedKey||currentKey===record.key) {
            store.put(record,'current');result.written=true;
          }
        };return result;
      });
      return state.written;
    }
    catch{this.close();return false;}
  }
  close(){if(!this.closed){this.closed=true;this.database.close();}}
}
function parseManifest(raw,identity) {
  const manifest=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(raw));
  const digest=value=>typeof value==='string'&&(/^[a-f0-9]{64}$/).test(value);
  if(manifest?.schema!==1||manifest.algorithm_identity!==identity||manifest.asset!=='rgb24-lut.bin.gz'||
    manifest.compression!=='gzip'||!Number.isInteger(manifest.compressed_bytes)||manifest.compressed_bytes<1||
    manifest.compressed_bytes>COMPRESSED_LIMIT||!RAW_SIZES.has(manifest.uncompressed_bytes)||
    !digest(manifest.compressed_sha256)||!digest(manifest.uncompressed_sha256))
    throw new Error('The color table does not match this encoder.');
  return manifest;
}
async function readStream(stream,{limit,exact=false,signal,onChunk=()=>{}}) {
  if(!stream)throw new Error('The color table could not be read.');
  const reader=stream.getReader();let received=0;
  const abort=()=>{reader.cancel(new DOMException('Color table loading was cancelled.','AbortError')).catch(()=>{});};
  signal?.addEventListener('abort',abort,{once:true});
  try {
    check(signal);
    while(true) {
      const {value,done}=await reader.read();check(signal);if(done)break;
      received+=value.byteLength;
      if(received>limit)throw new Error('The color table exceeded its expected size.');
      await onChunk(value,received);
    }
    if(exact&&received!==limit)throw new Error('The color table download is incomplete.');
    return received;
  }finally{signal?.removeEventListener('abort',abort);await reader.cancel().catch(()=>{});reader.releaseLock();}
}
async function readBytes(stream,limit,signal,exact,onProgress) {
  const value=new Uint8Array(limit);
  const received=await readStream(stream,{limit,signal,exact,onChunk:(chunk,total)=>{value.set(chunk,total-chunk.length);onProgress?.(total);}});
  return received===limit?value:value.slice(0,received);
}
async function importCompressed(compressed,manifest,importer,signal,onProgress) {
  if(compressed.length!==manifest.compressed_bytes||await sha256(compressed)!==manifest.compressed_sha256)
    throw new Error('The color table checksum is invalid.');
  check(signal);
  if(typeof DecompressionStream!=='function')throw new Error('This browser cannot decompress the color table.');
  const header=new Uint8Array(HEADER_BYTES);let headerLength=0;
  try {
    onProgress({phase:'decompressing',received:0,total:manifest.uncompressed_bytes});
    const stream=new Blob([compressed]).stream().pipeThrough(new DecompressionStream('gzip'));
    await readStream(stream,{limit:manifest.uncompressed_bytes,exact:true,signal,onChunk:async(chunk,total)=>{
      let offset=0;
      if(headerLength<HEADER_BYTES) {
        const length=Math.min(HEADER_BYTES-headerLength,chunk.length);header.set(chunk.subarray(0,length),headerLength);
        headerLength+=length;offset=length;
        if(headerLength===HEADER_BYTES){await importer.begin(header);check(signal);}
      }
      while(offset<chunk.length) {
        const end=Math.min(offset+65536,chunk.length);
        await importer.append(chunk.slice(offset,end));check(signal);offset=end;
      }
      onProgress({phase:'decompressing',received:total,total:manifest.uncompressed_bytes});
    }});
    onProgress({phase:'validating',received:manifest.uncompressed_bytes,total:manifest.uncompressed_bytes});
    await importer.finish(manifest.uncompressed_sha256);check(signal);
  }catch(error){await importer.abort().catch(()=>{});check(signal);throw error;}
}
/** Inflate directly into the Rust importer, keeping no plaintext LUT in JS. */
export async function loadLutAsset(manifestUrl,identity,importer,{signal,onProgress=()=>{},cacheOptions={},fetch:fetchAsset=globalThis.fetch}={}) {
  check(signal);
  const cache=await LutCache.open(cacheOptions);let saved;
  try {
    saved=await cache?.read();check(signal);
    let rawManifest;
    try {
      const response=await fetchAsset(manifestUrl,{signal,cache:'no-cache'});
      if(!response.ok)throw new Error(`Color table information is unavailable (HTTP ${response.status}).`);
      rawManifest=await readBytes(response.body,MANIFEST_LIMIT,signal,false);
    }catch(error){check(signal);if(!saved)throw error;rawManifest=saved.manifest;}
    const manifest=parseManifest(rawManifest,identity),key=`${await sha256(rawManifest)}:${identity}`;
    if(saved?.key===key) {
      try {
        onProgress({phase:'cached',received:0,total:manifest.compressed_bytes});
        await importCompressed(saved.compressed,manifest,importer,signal,onProgress);
        return {cached:true,saved:true};
      }catch(error){
        check(signal);
        if(importer.available?.()===false||typeof DecompressionStream!=='function')throw error;
        // Keep the previous record until a replacement has passed validation.
      }
    }
    check(signal);
    const assetUrl=new URL(manifest.asset,manifestUrl);assetUrl.searchParams.set('v',manifest.compressed_sha256);
    onProgress({phase:'downloading',received:0,total:manifest.compressed_bytes});
    const response=await fetchAsset(assetUrl,{signal});
    if(!response.ok)throw new Error(`The color table could not be downloaded (HTTP ${response.status}).`);
    const compressed=await readBytes(response.body,manifest.compressed_bytes,signal,true,
      received=>onProgress({phase:'downloading',received,total:manifest.compressed_bytes}));
    await importCompressed(compressed,manifest,importer,signal,onProgress);check(signal);
    const persisted=await cache?.write({key,manifest:rawManifest,compressed},{expectedKey:cache.observedKey??null});
    check(signal);
    return {cached:false,saved:Boolean(persisted)};
  }finally{cache?.close();}
}
