import test from 'node:test';
import assert from 'node:assert/strict';
import {WorkerClient,QuantizerPool} from '../site/worker-pool.mjs';
class FakeWorker {
  static live=[];
  constructor(url,options){this.url=url;this.options=options;this.messages=[];this.closed=false;FakeWorker.live.push(this);}
  postMessage(message,transfer){this.messages.push({message,transfer});this.handle?.(message);}
  terminate(){this.closed=true;}
  reply(index,result,error){this.onmessage({data:{id:this.messages[index].message.id,result,error}});}
}
globalThis.Worker=FakeWorker;
test('worker RPC matches out-of-order responses and surfaces errors',async()=>{
  const client=new WorkerClient('quantizer-worker.mjs'),a=client.call('a'),b=client.call('b');
  client.worker.reply(1,'second');client.worker.reply(0,'first');assert.equal(await a,'first');assert.equal(await b,'second');
  const bad=client.call('bad');client.worker.reply(2,null,'Rust says no');await assert.rejects(bad,/Rust says no/);client.close();
});
test('cancel rejects pending requests and terminates all isolated WASM workers',async()=>{
  const pool=new QuantizerPool(3,'quantizer-worker.mjs');
  const request=pool.workers[0].call('quantize',{});pool.close();await assert.rejects(request,/cancelled/);
  assert.ok(pool.workers.every(worker=>worker.worker.closed));await assert.rejects(pool.workers[1].call('init'),/cancelled/);
});
test('a failed transfer rejects the call without retaining an RPC entry',async()=>{
  const client=new WorkerClient('quantizer-worker.mjs');
  client.worker.postMessage=()=>{throw new DOMException('Invalid transfer','DataCloneError');};
  await assert.rejects(client.call('plan'),/Invalid transfer/);assert.equal(client.pending.size,0);client.close();
});
test('color work is disjoint, dynamically balanced and imported before packing',async()=>{
  const pool=new QuantizerPool(3,'quantizer-worker.mjs'),keys=Uint32Array.from({length:5200},(_,i)=>i);
  const searched=[],workers=new Set(),gpuCalls=[];
  for(const [index,client] of pool.workers.entries())client.worker.handle=message=>{
    const {id,method,args}=message;
    if(method==='prepare')queueMicrotask(()=>client.worker.onmessage({data:{id,result:keys.slice().buffer}}));
    if(method==='plan') {
      const batch=new Uint32Array(args.keys),seeds=new Uint32Array(args.seeds);
      workers.add(index);searched.push(...batch);assert.deepEqual(seeds,batch.map(key=>key+7));
      setTimeout(()=>client.worker.onmessage({data:{id,result:batch.map(key=>key+1).buffer}}),index===0?5:0);
    }
    if(method==='pack') {
      assert.deepEqual(new Uint32Array(args.keys),keys);
      assert.deepEqual(new Uint32Array(args.plans),keys.map(key=>key+1));
      queueMicrotask(()=>client.worker.onmessage({data:{id,result:[Uint8Array.of(1).buffer,Uint8Array.of(2).buffer]}}));
    }
  };
  const gpu={async seeds(input){gpuCalls.push(input.slice());return input.map(key=>key+7);},destroy(){}};
  const result=await pool.batch([new Uint8Array(6),new Uint8Array(6)],gpu);
  assert.deepEqual(searched.sort((a,b)=>a-b),Array.from(keys));assert.equal(workers.size,3);
  assert.equal(gpuCalls.length,1);assert.deepEqual(gpuCalls[0],keys);
  assert.deepEqual(result.map(buffer=>new Uint8Array(buffer)[0]),[1,2]);pool.close();
});
test('queued batches retain one cache owner and a failed GPU falls back once',async()=>{
  const pool=new QuantizerPool(2,'quantizer-worker.mjs');let active=0,peak=0,gpuCalls=0,destroyed=0,marker=0;
  pool.workers[0].worker.handle=message=>{
    const {id,method,args}=message;
    if(method==='prepare') {
      active++;peak=Math.max(peak,active);marker=new Uint8Array(args.frames[0])[0];
      queueMicrotask(()=>pool.workers[0].worker.onmessage({data:{id,result:Uint32Array.of(marker).buffer}}));
    } else if(method==='plan') {
      assert.equal(args.seeds,undefined);
      setTimeout(()=>pool.workers[0].worker.onmessage({data:{id,result:Uint32Array.of(0).buffer}}),1);
    } else if(method==='pack') {
      active--;queueMicrotask(()=>pool.workers[0].worker.onmessage({data:{id,result:[Uint8Array.of(marker).buffer]}}));
    }
  };
  const gpu={async seeds(){gpuCalls++;throw Error('Device lost');},destroy(){destroyed++;}};
  const results=await Promise.all([pool.batch([Uint8Array.of(3)],gpu),pool.batch([Uint8Array.of(9)],gpu)]);
  assert.equal(peak,1);assert.equal(gpuCalls,1);assert.equal(destroyed,1);
  assert.deepEqual(results.map(batch=>new Uint8Array(batch[0])[0]),[3,9]);pool.close();
});
