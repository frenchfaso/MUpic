import test from 'node:test';
import assert from 'node:assert/strict';
import {WorkerClient,QuantizerPool} from '../site/worker-pool.mjs';
class FakeWorker {
  static live=[];
  constructor(url,options){this.url=url;this.options=options;this.messages=[];this.closed=false;FakeWorker.live.push(this);}
  postMessage(message,transfer){this.messages.push({message,transfer});}
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
test('a quantizer batch sends distinct RGB and GPU buffers to parallel workers',async()=>{
  const pool=new QuantizerPool(2,'quantizer-worker.mjs'),rgb=[new Uint8Array(6),new Uint8Array(6)],hints=[new Uint32Array(2),new Uint32Array(2)];
  const pending=pool.batch(rgb,hints);
  for(let i=0;i<2;i++) {
    assert.equal(pool.workers[i].worker.messages[0].message.method,'quantize');
    assert.equal(pool.workers[i].worker.messages[0].transfer.length,2);pool.workers[i].worker.reply(0,new ArrayBuffer(1));
  }
  assert.equal((await pending).length,2);pool.close();
});
