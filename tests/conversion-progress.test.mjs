import test from 'node:test';
import assert from 'node:assert/strict';
import {ConversionProgress} from '../site/conversion-progress.mjs';
import {WorkerClient} from '../site/worker-pool.mjs';
test('out-of-order quantizer completions advance progress before ordered output is ready',()=>{
  const progress=new ConversionProgress(3);assert.equal(progress.snapshot().value,null);progress.start();
  const values=[];
  for(const index of [2,1,0]) {
    const state=progress.quantizedFrame(index);values.push(state.value);assert.equal(state.encodedFrames,0);
  }
  assert.ok(values[0]>0);assert.ok(values[0]<values[1]&&values[1]<values[2]);assert.equal(values[2],0.85);
  assert.equal(progress.quantizedFrame(0).quantizedFrames,3,'duplicate messages cannot overcount completed work');
  for(let index=0;index<3;index++)progress.encodedFrame(index);
  assert.equal(progress.finishing().value,0.95);assert.equal(progress.complete().value,1);
});
test('unfinished or out-of-order output cannot claim completion',()=>{
  const progress=new ConversionProgress(3);progress.start();progress.quantizedFrame(1);
  assert.throws(()=>progress.encodedFrame(1),/source order/);
  assert.throws(()=>progress.encodedFrame(0),/source order/);
  assert.throws(()=>progress.finishing(),/incomplete/);assert.throws(()=>progress.complete(),/Validate/);
  assert.throws(()=>progress.quantizedFrame(3),/index/);
});
test('worker promises report every actual completion without changing the ordered mux inputs',async()=>{
  class WorkerFixture {
    constructor(){this.requests=[];}
    postMessage(message){this.requests.push(message);}
    terminate(){}
    reply(value){this.onmessage({data:{id:this.requests[0].id,result:value}});}
  }
  const original=globalThis.Worker;globalThis.Worker=WorkerFixture;
  const clients=Array.from({length:3},()=>new WorkerClient('quantizer-worker.mjs'));
  try {
    const progress=new ConversionProgress(3),events=[];progress.start();
    const pending=clients.map((client,index)=>client.call('quantize').then(result=>{events.push(progress.quantizedFrame(index));return result;}));
    clients[2].worker.reply('frame2');await Promise.resolve();
    assert.equal(events.length,1);assert.equal(events[0].quantizedFrames,1);assert.equal(events[0].encodedFrames,0);assert.ok(events[0].value>0&&events[0].value<1);
    clients[1].worker.reply('frame1');await Promise.resolve();assert.equal(events.length,2);
    clients[0].worker.reply('frame0');const ordered=await Promise.all(pending);
    assert.deepEqual(ordered,['frame0','frame1','frame2']);assert.equal(events.length,3);
    for(let index=0;index<ordered.length;index++)progress.encodedFrame(index);
    assert.ok(progress.snapshot().value<1);progress.finishing();assert.equal(progress.complete().value,1);
  } finally {clients.forEach(client=>client.close());globalThis.Worker=original;}
});
