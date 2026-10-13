import {loadLutAsset} from './lut-cache.mjs?v=25727af609d67526';
/** Exact searches work without isolation; a verified LUT can share frame work. */
export class WorkerClient {
  constructor(url) {
    this.worker=new Worker(url,{type:'module'});this.pending=new Map();this.serial=0;this.closed=false;
    this.worker.onmessage=({data})=>{
      const pending=this.pending.get(data.id);if(!pending)return;this.pending.delete(data.id);
      if(data.error)pending.reject(new Error(data.error));else pending.resolve(data.result);
    };
    this.worker.onerror=event=>this.close(new Error(event.message||'The encoder worker stopped unexpectedly.'));
  }
  call(method,args={},transfer=[]) {
    if(this.closed)return Promise.reject(new Error('Conversion was cancelled.'));
    return new Promise((resolve,reject)=>{
      const id=++this.serial;this.pending.set(id,{resolve,reject});
      try{this.worker.postMessage({id,method,args},transfer);}
      catch(error){this.pending.delete(id);reject(error);}
    });
  }
  close(error=new DOMException('Conversion was cancelled.','AbortError')) {
    if(this.closed)return;this.closed=true;this.worker.terminate();
    for(const pending of this.pending.values())pending.reject(error);this.pending.clear();
  }
}
export class QuantizerPool {
  constructor(count,url,{eager=false,shared=globalThis.crossOriginIsolated===true&&typeof SharedArrayBuffer==='function'}={}) {
    if(!Number.isInteger(count)||count<1)throw new Error('Worker count must be positive.');
    this.count=count;this.url=url;this.closed=false;this.lutReady=false;this.pending=Promise.resolve();
    this.shared=shared&&count>1;this.sharedLut=null;this.freeBuffers=[];
    this.workers=Array.from({length:eager?count:1},()=>new WorkerClient(url));
  }
  async init(wasmUrl) {
    this.wasmUrl=wasmUrl;
    try {
      const results=await Promise.all(this.workers.map(worker=>worker.call('init',{url:wasmUrl})));
      this.module=results[0]?.module;
    }
    catch(error){this.close();throw error;}
  }
  checkOwner() {
    if(this.closed||this.workers[0]?.closed){this.close();throw new Error('The encoder worker stopped. Retry loading the color table.');}
  }
  async ensureWorkers(count=this.count) {
    this.checkOwner();const additions=[];
    this.workers=this.workers.filter(worker=>!worker.closed);
    while(this.workers.length<count){const worker=new WorkerClient(this.url);this.workers.push(worker);additions.push(worker);}
    try{await Promise.all(additions.map(worker=>worker.call('init',{url:this.wasmUrl,module:this.module,sharedLut:this.sharedLut})));}
    catch(error){for(const worker of additions)worker.close(error);throw error;}
  }
  stopLut(){this.loadingController?.abort();}
  async loadLut(manifestUrl,{signal,onProgress,cacheOptions,fetch}={}) {
    this.checkOwner();if(this.loadingController)throw new Error('The color table is already loading.');
    const controller=new AbortController();this.loadingController=controller;
    const abort=()=>controller.abort();signal?.addEventListener('abort',abort,{once:true});if(signal?.aborted)abort();
    const owner=this.workers[0];
    const send=(method,name,value)=>owner.call(method,{[name]:value},[value.buffer]);
    try {
      const identity=await owner.call('lutIdentity');
      const result=await loadLutAsset(manifestUrl,identity,{
        begin:header=>send('beginLutAsset','header',header),
        append:chunk=>send('appendLutAsset','chunk',chunk),
        finish:expectedSha=>owner.call('finishLutAsset',{expectedSha}),
        abort:()=>owner.closed?Promise.resolve():owner.call('abortLutAsset'),
        available:()=>!owner.closed&&!this.closed,
      },{signal:controller.signal,onProgress,cacheOptions,fetch});
      const info=await owner.call('lutInfo');
      if(controller.signal.aborted)throw new DOMException('Color table loading was cancelled.','AbortError');
      if(info.loadedBlocks!==info.blockCount)throw new Error('The color table is incomplete.');
      this.lutReady=true;
      for(const worker of this.workers.slice(1))worker.close();this.workers=[owner];
      if(this.shared) {
        try {this.sharedLut=await owner.call('shareLut');}
        catch(error) {
          this.checkOwner();
          if(controller.signal.aborted)throw error;
          // A device may have room for the LUT but not its shared copy.
          this.shared=false;
        }
      }
      return result;
    }finally{signal?.removeEventListener('abort',abort);this.loadingController=null;}
  }
  batch(frames) {
    const request=this.pending.then(()=>this.planBatch(frames));this.pending=request.catch(()=>{});return request;
  }
  async planBatch(frames) {
    this.checkOwner();
    if(this.sharedLut) {
      if(frames.length<1||frames.length>3||frames.some(frame=>frame.length!==216000))
        throw new Error('A planning batch must contain one to three 360x200 RGB frames.');
      return this.packSharedBatch(frames);
    }
    const owner=this.workers[0],buffers=frames.map(frame=>
      frame.byteOffset===0&&frame.byteLength===frame.buffer.byteLength?frame.buffer:frame.slice().buffer);
    if(this.lutReady)return owner.call('quantizeBatch',{frames:buffers},buffers);
    const keys=new Uint32Array(await owner.call('prepare',{frames:buffers},buffers));
    if(keys.length)await this.ensureWorkers();
    const plans=new Uint32Array(keys.length);let next=0;
    await Promise.all(this.workers.map(async worker=>{
      while(next<keys.length) {
        const start=next,end=Math.min(next+256,keys.length);next=end;
        const batch=keys.slice(start,end),result=await worker.call('plan',{keys:batch.buffer},[batch.buffer]);
        plans.set(new Uint32Array(result),start);
      }
    }));
    return owner.call('pack',{keys:keys.buffer,plans:plans.buffer},[keys.buffer,plans.buffer]);
  }
  async packSharedBatch(frames) {
    await this.ensureWorkers(Math.min(this.count,frames.length));
    const bytes=frames.length*(216000+36000);
    const index=this.freeBuffers.findIndex(buffer=>buffer.byteLength>=bytes);
    const buffer=index<0?new SharedArrayBuffer(bytes):this.freeBuffers.splice(index,1)[0];
    const rgbBytes=frames.length*216000,result=[];
    frames.forEach((frame,i)=>{
      new Uint8Array(buffer,i*216000,216000).set(frame);
      result.push(new Uint8Array(buffer,rgbBytes+i*36000,36000));
    });
    let next=0;
    try {
      const settled=await Promise.allSettled(this.workers.slice(0,frames.length).map(async worker=>{
        while(next<frames.length) {
          const i=next++;
          await worker.call('packShared',{buffer,rgbOffset:i*216000,packedOffset:rgbBytes+i*36000});
        }
      }));
      const failed=settled.find(result=>result.status==='rejected');
      if(failed)throw failed.reason;
      return result;
    }catch(error){if(!this.closed&&this.freeBuffers.length<2)this.freeBuffers.push(buffer);throw error;}
  }
  releaseBatch(result) {
    const buffer=result[0]?.buffer;
    if(this.sharedLut&&buffer instanceof SharedArrayBuffer&&!this.closed&&this.freeBuffers.length<2)
      this.freeBuffers.push(buffer);
  }
  close(){this.closed=true;this.stopLut();this.freeBuffers=[];this.sharedLut=null;for(const worker of this.workers)worker.close();}
}
