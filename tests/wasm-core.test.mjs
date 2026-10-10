import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {EncoderCore} from '../site/wasm-core.mjs';
import {MupPreview,PALETTE,CONTENT_FPS} from '../site/mup-decoder.mjs';
const wasmFile=new URL('../site/assets/encoder.wasm',import.meta.url);
const makeCore=async()=>new EncoderCore((await WebAssembly.instantiate(await readFile(wasmFile),{})).instance.exports);
const rowFrame=front=>Uint8Array.from({length:36000},(_,i)=>front[(i%180)*256+Math.floor(i/180)]);
test('the real Rust WASM quantizes hardware colors and emits independently decodable MUP9',async()=>{
  const quantizer=await makeCore(),encoder=await makeCore(),frames=8,parts=[],previews=[];
  const pcm=new Uint8Array(Math.ceil(frames/CONTENT_FPS*22085)*4);
  const pcmView=new DataView(pcm.buffer);for(let i=0;i<pcm.length;i+=4){pcmView.setInt16(i,1234,true);pcmView.setInt16(i+2,-5678,true);}
  encoder.begin(frames,pcm,'BROWSER ABI TEST');
  for(let frame=0;frame<frames;frame++) {
    const rgb=new Uint8Array(216000);
    for(let y=0;y<200;y++)for(let x=2;x<358;x++)rgb.set(PALETTE[(Math.floor(x/23)+Math.floor(y/31)+frame)%16],(y*360+x)*3);
    const packed=quantizer.quantize(rgb);
    assert.equal(packed.length,36000);
    for(let y=0;y<200;y++)for(let x=2;x<358;x++)assert.equal((packed[y*180+Math.floor(x/2)]>>((x%2)*4))&15,(Math.floor(x/23)+Math.floor(y/31)+frame)%16);
    parts.push(...encoder.frame(packed));
    const e=encoder.exports;previews.push(encoder.bytes(e.mup_preview_ptr(),e.mup_preview_len()).slice());
  }
  const finished=encoder.finish();parts.push(...finished.chunks);
  assert.equal(finished.stats.receivedFrames,frames);assert.ok(finished.stats.peakSlots<=425000);
  const blob=new Blob([finished.prefix,...parts]),preview=await MupPreview.open(blob);
  assert.equal(preview.header.frameCount,frames);assert.equal(preview.header.divider,281);
  for(const frame of [0,1,7,4,2,6]){await preview.frame(frame);assert.deepEqual(rowFrame(preview.state.front),previews[frame],`frame${frame} post-rate-control`);}
  const fakeContext={createBuffer(channels,samples,rate){return {sampleRate:rate,data:[new Float32Array(samples),new Float32Array(samples)],getChannelData(index){return this.data[index];}};}};
  const audio=await preview.audioBuffer(fakeContext);
  assert.equal(audio.sampleRate,22085);assert.equal(audio.data[0][0],1234/32768);assert.equal(audio.data[1][0],-5678/32768);
  assert.equal(audio.data[0].at(-1),1234/32768);
  if(process.env.MUP_TEST_OUTPUT)await writeFile(process.env.MUP_TEST_OUTPUT,new Uint8Array(await blob.arrayBuffer()));
});
test('raw ABI validation errors propagate from Rust with readable messages',async()=>{
  const core=await makeCore();assert.throws(()=>core.quantize(Uint8Array.of(0,0,0)),/360x200/);
  assert.throws(()=>core.begin(1,new Uint8Array(),'INVALID'),/Frame count/);
});
test('split color searches reproduce sequential quantization and retain shared cached colors',async()=>{
  const owner=await makeCore(),reference=await makeCore(),workers=await Promise.all([makeCore(),makeCore(),makeCore()]);
  const frames=Array.from({length:4},(_,frame)=>{
    const rgb=new Uint8Array(216000);
    for(let pixel=0;pixel<72000;pixel++) {
      const index=(pixel+frame*13)%80;
      const color=index<16?PALETTE[index]:[(index*61)&255,(index*97)&255,(index*31)&255];
      rgb.set(color,pixel*3);
    }
    return rgb;
  });
  const batch=new Uint8Array(frames.length*216000);frames.forEach((rgb,index)=>batch.set(rgb,index*216000));
  const keys=owner.prepareKeys(batch),plans=new Uint32Array(keys.length);
  assert.equal(keys.length,80);
  for(let index=0;index<keys.length;index+=27) {
    const part=keys.slice(index,index+27);
    plans.set(workers[Math.floor(index/27)].planKeys(part),index);
  }
  owner.importPlans(keys,plans);
  assert.equal(owner.prepareKeys(batch).length,0);
  for(const rgb of frames)assert.deepEqual(owner.quantize(rgb),reference.quantize(rgb));
  // Bad imports cannot partially insert any of the accompanying valid keys.
  const missing=owner.prepareKeys(new Uint8Array(216000).fill(33));
  assert.throws(()=>owner.importPlans(missing,Uint32Array.of(0xffffffff)),/Invalid/);
  assert.deepEqual(owner.prepareKeys(new Uint8Array(216000).fill(33)),missing);
  assert.throws(()=>owner.planKeys(Uint32Array.of(0x1000000)),/Invalid/);
  assert.deepEqual(owner.planKeys(new Uint32Array()),new Uint32Array());
  assert.throws(()=>owner.check(owner.exports.mup_plan_keys(0,4)),/live mup_alloc/);
  const pointer=owner.allocate(Uint8Array.of(0,0,0));
  try {assert.throws(()=>owner.check(owner.exports.mup_plan_keys(pointer,3)),/whole u32/);}
  finally {owner.free(pointer,3);}
});
