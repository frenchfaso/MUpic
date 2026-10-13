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
  const pool=new QuantizerPool(3,'quantizer-worker.mjs',{eager:true});
  const request=pool.workers[0].call('quantize',{});pool.close();await assert.rejects(request,/cancelled/);
  assert.ok(pool.workers.every(worker=>worker.worker.closed));await assert.rejects(pool.workers[1].call('init'),/cancelled/);
});
test('a failed transfer rejects the call without retaining an RPC entry',async()=>{
  const client=new WorkerClient('quantizer-worker.mjs');
  client.worker.postMessage=()=>{throw new DOMException('Invalid transfer','DataCloneError');};
  await assert.rejects(client.call('plan'),/Invalid transfer/);assert.equal(client.pending.size,0);client.close();
});
test('color work is disjoint, dynamically balanced and imported before packing',async()=>{
  const pool=new QuantizerPool(3,'quantizer-worker.mjs',{eager:true}),keys=Uint32Array.from({length:5200},(_,i)=>i);
  const searched=[],workers=new Set();
  for(const [index,client] of pool.workers.entries())client.worker.handle=message=>{
    const {id,method,args}=message;
    if(method==='prepare')queueMicrotask(()=>client.worker.onmessage({data:{id,result:keys.slice().buffer}}));
    if(method==='plan') {
      const batch=new Uint32Array(args.keys);
      workers.add(index);searched.push(...batch);
      assert.deepEqual(Object.keys(args),['keys']);
      setTimeout(()=>client.worker.onmessage({data:{id,result:batch.map(key=>key+1).buffer}}),index===0?5:0);
    }
    if(method==='pack') {
      assert.deepEqual(new Uint32Array(args.keys),keys);
      assert.deepEqual(new Uint32Array(args.plans),keys.map(key=>key+1));
      queueMicrotask(()=>client.worker.onmessage({data:{id,result:[Uint8Array.of(1).buffer,Uint8Array.of(2).buffer]}}));
    }
  };
  const result=await pool.batch([new Uint8Array(6),new Uint8Array(6)]);
  assert.deepEqual(searched.sort((a,b)=>a-b),Array.from(keys));assert.equal(workers.size,3);
  assert.deepEqual(result.map(buffer=>new Uint8Array(buffer)[0]),[1,2]);pool.close();
});
test('queued batches retain one cache owner and preserve frame order',async()=>{
  const pool=new QuantizerPool(2,'quantizer-worker.mjs',{eager:true});let active=0,peak=0,marker=0;
  pool.workers[0].worker.handle=message=>{
    const {id,method,args}=message;
    if(method==='prepare') {
      active++;peak=Math.max(peak,active);marker=new Uint8Array(args.frames[0])[0];
      queueMicrotask(()=>pool.workers[0].worker.onmessage({data:{id,result:Uint32Array.of(marker).buffer}}));
    } else if(method==='plan') {
      assert.deepEqual(Object.keys(args),['keys']);
      setTimeout(()=>pool.workers[0].worker.onmessage({data:{id,result:Uint32Array.of(0).buffer}}),1);
    } else if(method==='pack') {
      active--;queueMicrotask(()=>pool.workers[0].worker.onmessage({data:{id,result:[Uint8Array.of(marker).buffer]}}));
    }
  };
  const results=await Promise.all([pool.batch([Uint8Array.of(3)]),pool.batch([Uint8Array.of(9)])]);
  assert.equal(peak,1);
  assert.deepEqual(results.map(batch=>new Uint8Array(batch[0])[0]),[3,9]);pool.close();
});
test('shared frame work stays ordered and buffers are reused only after consumption',async()=>{
  const pool=new QuantizerPool(3,'quantizer-worker.mjs',{eager:true,shared:true});
  pool.sharedLut=new SharedArrayBuffer(4);pool.lutReady=true;
  for(const client of pool.workers)client.worker.handle=({id,method,args})=>{
    assert.equal(method,'packShared');
    const marker=new Uint8Array(args.buffer,args.rgbOffset,216000)[0];
    setTimeout(()=>{
      new Uint8Array(args.buffer,args.packedOffset,36000).fill(marker);
      client.worker.onmessage({data:{id}});
    },marker===3?5:0);
  };
  const make=marker=>new Uint8Array(216000).fill(marker),frames=[3,9,7].map(make);
  const first=await pool.batch(frames),firstBuffer=first[0].buffer;
  assert.deepEqual(first.map(frame=>frame[0]),[3,9,7]);
  assert.ok(first.every(frame=>frame.buffer===firstBuffer));
  assert.ok(frames.every(frame=>frame.byteLength===216000),'shared capture inputs are not detached');
  const second=await pool.batch([make(4),make(8),make(6)]);
  assert.notEqual(second[0].buffer,firstBuffer,'the encoder still owns the first packed batch');
  pool.releaseBatch(first);
  const third=await pool.batch([make(2),make(5),make(1)]);
  assert.equal(third[0].buffer,firstBuffer);
  assert.deepEqual(second.map(frame=>frame[0]),[4,8,6],'reuse cannot overwrite another leased batch');
  pool.releaseBatch(second);pool.releaseBatch(third);pool.close();
});
test('shared worker errors settle remaining writes before recycling a slab',async()=>{
  const pool=new QuantizerPool(2,'quantizer-worker.mjs',{eager:true,shared:true});
  pool.sharedLut=new SharedArrayBuffer(4);pool.lutReady=true;let complete;
  pool.workers[0].worker.handle=({id})=>queueMicrotask(()=>pool.workers[0].worker.onmessage({data:{id,error:'packing failed'}}));
  pool.workers[1].worker.handle=({id,args})=>{complete=()=>{
    new Uint8Array(args.buffer,args.packedOffset,36000).fill(7);
    pool.workers[1].worker.onmessage({data:{id}});
  };};
  const request=pool.batch([new Uint8Array(216000),new Uint8Array(216000)]);
  const rejected=assert.rejects(request,/packing failed/);
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(pool.freeBuffers.length,0,'a pending worker can still write to the shared slab');
  complete();await rejected;assert.equal(pool.freeBuffers.length,1);pool.close();
});
test('cancelling a shared batch cannot restore buffers to a closed pool',async()=>{
  const pool=new QuantizerPool(2,'quantizer-worker.mjs',{eager:true,shared:true});
  pool.sharedLut=new SharedArrayBuffer(4);pool.lutReady=true;
  const request=pool.batch([new Uint8Array(216000),new Uint8Array(216000)]);
  const rejected=assert.rejects(request,/cancelled/);
  await new Promise(resolve=>setImmediate(resolve));pool.close();await rejected;
  assert.equal(pool.freeBuffers.length,0);
});
