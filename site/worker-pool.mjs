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
      const id=++this.serial; this.pending.set(id,{resolve,reject}); this.worker.postMessage({id,method,args},transfer);
    });
  }
  close(error=new DOMException('Conversion was cancelled.','AbortError')) {
    if(this.closed) return; this.closed=true; this.worker.terminate();
    for(const pending of this.pending.values()) pending.reject(error); this.pending.clear();
  }
}
export class QuantizerPool {
  constructor(count,url) { this.workers=Array.from({length:count},()=>new WorkerClient(url)); }
  async init(wasmUrl) { await Promise.all(this.workers.map(worker=>worker.call('init',{url:wasmUrl}))); }
  async batch(frames,seeds=[]) {
    return Promise.all(frames.map((frame,index)=>{
      const hint=seeds[index],args={rgb:frame.buffer,seeds:hint?.buffer},transfer=[frame.buffer];
      if(hint) transfer.push(hint.buffer);
      return this.workers[index].call('quantize',args,transfer);
    }));
  }
  close() { for(const worker of this.workers) worker.close(); }
}
