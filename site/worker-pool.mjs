/** Worker scheduling deliberately needs neither SharedArrayBuffer nor COOP/COEP. */
export class WorkerClient {
  constructor(url) {
    this.worker=new Worker(url,{type:'module'}); this.pending=new Map(); this.serial=0; this.closed=false;
    this.worker.onmessage=({data})=>{
      const pending=this.pending.get(data.id); if(!pending) return; this.pending.delete(data.id);
      if(data.error) pending.reject(new Error(data.error)); else pending.resolve(data.result);
    };
    this.worker.onerror=event=>this.close(new Error(event.message || 'The encoder worker stopped unexpectedly.'));
  }
  call(method,args={},transfer=[]) {
    if(this.closed) return Promise.reject(new Error('Conversion was cancelled.'));
    return new Promise((resolve,reject)=>{
      const id=++this.serial;this.pending.set(id,{resolve,reject});
      try {this.worker.postMessage({id,method,args},transfer);}
      catch(error){this.pending.delete(id);reject(error);}
    });
  }
  close(error=new DOMException('Conversion was cancelled.','AbortError')) {
    if(this.closed) return; this.closed=true; this.worker.terminate();
    for(const pending of this.pending.values()) pending.reject(error); this.pending.clear();
  }
}
export class QuantizerPool {
  constructor(count,url) {
    if(!Number.isInteger(count)||count<1)throw new Error('Worker count must be positive.');
    this.workers=Array.from({length:count},()=>new WorkerClient(url));this.pending=Promise.resolve();
  }
  async init(wasmUrl) { await Promise.all(this.workers.map(worker=>worker.call('init',{url:wasmUrl}))); }
  batch(frames,gpu=null) {
    // One owner retains the cache; workers only search disjoint missing colors.
    const request=this.pending.then(()=>this.planBatch(frames,gpu));
    this.pending=request.catch(()=>{});return request;
  }
  async planBatch(frames,gpu) {
    const coordinator=this.workers[0],buffers=frames.map(frame=>
      frame.byteOffset===0&&frame.byteLength===frame.buffer.byteLength?frame.buffer:frame.slice().buffer);
    const keys=new Uint32Array(await coordinator.call('prepare',{frames:buffers},buffers));
    let seeds=null;
    if(gpu&&gpu!==this.failedGpu&&keys.length) {
      try {seeds=await gpu.seeds(keys);}catch{gpu.destroy();this.failedGpu=gpu;}
    }
    const plans=new Uint32Array(keys.length);let next=0;
    // Small work units balance cheap palette colors and expensive exact searches.
    await Promise.all(this.workers.map(async worker=>{
      while(next<keys.length) {
        const start=next,end=Math.min(next+256,keys.length);next=end;
        const batch=keys.slice(start,end),hint=seeds?.slice(start,end),transfer=[batch.buffer];
        if(hint)transfer.push(hint.buffer);
        const result=await worker.call('plan',{keys:batch.buffer,seeds:hint?.buffer},transfer);
        plans.set(new Uint32Array(result),start);
      }
    }));
    return coordinator.call('pack',{keys:keys.buffer,plans:plans.buffer},[keys.buffer,plans.buffer]);
  }
  close() { for(const worker of this.workers) worker.close(); }
}
