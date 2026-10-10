import {loadLutAsset} from './lut-cache.mjs?v=25727af609d67526';
/** Worker scheduling deliberately needs neither SharedArrayBuffer nor COOP/COEP. */
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
  constructor(count,url,{eager=false}={}) {
    if(!Number.isInteger(count)||count<1)throw new Error('Worker count must be positive.');
    this.count=count;this.url=url;this.closed=false;this.lutReady=false;this.pending=Promise.resolve();
    this.workers=Array.from({length:eager?count:1},()=>new WorkerClient(url));
  }
  async init(wasmUrl) {
    this.wasmUrl=wasmUrl;
    try{await Promise.all(this.workers.map(worker=>worker.call('init',{url:wasmUrl})));}
    catch(error){this.close();throw error;}
  }
  checkOwner() {
    if(this.closed||this.workers[0]?.closed){this.close();throw new Error('The encoder worker stopped. Retry loading the color table.');}
  }
  async ensureWorkers() {
    this.checkOwner();const additions=[];
    this.workers=this.workers.filter(worker=>!worker.closed);
    while(this.workers.length<this.count){const worker=new WorkerClient(this.url);this.workers.push(worker);additions.push(worker);}
    try{await Promise.all(additions.map(worker=>worker.call('init',{url:this.wasmUrl})));}
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
      return result;
    }finally{signal?.removeEventListener('abort',abort);this.loadingController=null;}
  }
  batch(frames) {
    const request=this.pending.then(()=>this.planBatch(frames));this.pending=request.catch(()=>{});return request;
  }
  async planBatch(frames) {
    this.checkOwner();
    const owner=this.workers[0],buffers=frames.map(frame=>
      frame.byteOffset===0&&frame.byteLength===frame.buffer.byteLength?frame.buffer:frame.slice().buffer);
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
  close(){this.closed=true;this.stopLut();for(const worker of this.workers)worker.close();}
}
