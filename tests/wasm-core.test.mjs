import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import {createGunzip} from 'node:zlib';
import {EncoderCore} from '../site/wasm-core.mjs';
import {MupPreview,PALETTE,CONTENT_FPS} from '../site/mup-decoder.mjs';
const wasmFile=new URL('../site/assets/encoder.wasm',import.meta.url);
const makeCore=async()=>new EncoderCore((await WebAssembly.instantiate(await readFile(wasmFile),{})).instance.exports);
const rowFrame=front=>Uint8Array.from({length:36000},(_,i)=>front[(i%180)*256+Math.floor(i/180)]);
test('production WASM omits offline LUT generation and validation APIs',async()=>{
  const core=await makeCore();
  for(const name of ['mup_lut_generate','mup_lut_block_identity','mup_lut_candidate_count','mup_lut_validate'])
    assert.equal(core.exports[name],undefined,`${name} must remain offline only`);
});
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
const lutFile=new URL('../site/assets/rgb24-lut.bin.gz',import.meta.url);
async function* lutChunks() {
  const source=createReadStream(lutFile),decoded=source.pipe(createGunzip());
  try {for await(const chunk of decoded)yield new Uint8Array(chunk.buffer,chunk.byteOffset,chunk.byteLength);}
  finally {decoded.destroy();source.destroy();}
}
async function loadLut(core,{expectedSha}={}) {
  const header=new Uint8Array(128),hash=createHash('sha256');let filled=0;
  for await(const chunk of lutChunks()) {
    hash.update(chunk);let offset=0;
    if(filled<128) {
      const count=Math.min(128-filled,chunk.length);header.set(chunk.subarray(0,count),filled);filled+=count;offset=count;
      if(filled===128)core.beginLutAsset(header);
    }
    if(offset<chunk.length)core.appendLutAsset(chunk.subarray(offset));
  }
  core.finishLutAsset(expectedSha??hash.digest('hex'));return header;
}
test('the portable LUT asset preserves exact Rust packing and complete MUP output',async()=>{
  const owner=await makeCore(),reference=await makeCore();
  assert.deepEqual(owner.lutInfo(),{blockCount:4096,blockSize:4096,loadedBlocks:0,totalColors:1<<24});
  assert.match(owner.lutIdentity(),/^[0-9a-f]{64}$/);
  const header=await loadLut(owner);
  assert.equal(Buffer.from(header.subarray(32,64)).toString('hex'),owner.lutIdentity());
  assert.equal(Buffer.from(header.subarray(64,96)).toString('hex'),owner.lutPatternFingerprint(),'native/WASM plan patterns must agree');
  assert.equal(owner.lutInfo().loadedBlocks,4096);
  assert.equal(owner.exports.mup_lut_generate,undefined,'the browser must not generate LUT colors');
  const rgb=new Uint8Array(216000);
  for(const color of [[0,0,101],[0,8,8],[131,79,36],PALETTE[12],PALETTE[7],[123,122,125],[255,255,254]]) {
    for(let pixel=0;pixel<72000;pixel++)rgb.set(color,pixel*3);
    assert.equal(owner.prepareKeys(rgb).length,0);
    assert.deepEqual(owner.quantize(rgb),reference.quantize(rgb),'all 64 Bayer positions and black side borders');
  }
  const encoded=[await makeCore(),await makeCore()],parts=[[],[]],pcm=new Uint8Array(11000);
  encoded.forEach(core=>core.begin(2,pcm,'EXACT LUT PARITY'));
  for(const color of [[0,0,101],[0,0,127]]) {
    for(let pixel=0;pixel<72000;pixel++)rgb.set(color,pixel*3);
    parts[0].push(...encoded[0].frame(owner.quantize(rgb)));
    parts[1].push(...encoded[1].frame(reference.quantize(rgb)));
  }
  const outputs=await Promise.all(encoded.map(async(core,index)=>{
    const {prefix,chunks}=core.finish();return new Uint8Array(await new Blob([prefix,...parts[index],...chunks]).arrayBuffer());
  }));
  assert.deepEqual(outputs[0],outputs[1],'the complete MUP, including audio and rate control, must remain byte identical');
  owner.resetLut();assert.equal(owner.lutInfo().loadedBlocks,0);
});
test('LUT asset ABI rejects corrupt headers, incomplete payloads, invalid IDs and manifest mismatches',async()=>{
  const core=await makeCore();let header;
  for await(const chunk of lutChunks()){header=chunk.slice(0,128);break;}
  for(const offset of [0,8,12,13,16,20,24,32,64]) {
    const invalid=header.slice();invalid[offset]^=128;
    assert.throws(()=>core.beginLutAsset(invalid),/LUT/);
    assert.equal(core.lutInfo().loadedBlocks,0);
  }
  assert.throws(()=>core.beginLutAsset(header.subarray(0,-1)),/LUT asset header size/);
  core.beginLutAsset(header);
  assert.throws(()=>core.finishLutAsset(),/incomplete/);
  core.beginLutAsset(header);
  assert.throws(()=>core.appendLutAsset(new Uint8Array(header[12]===0?4:3).fill(255)),/candidate id/);
  core.beginLutAsset(header);core.abortLutAsset();
  assert.throws(()=>core.appendLutAsset(new Uint8Array(3)),/Begin LUT asset/);
  await assert.rejects(loadLut(core,{expectedSha:'0'.repeat(64)}),/manifest checksum/);
  assert.equal(core.lutInfo().loadedBlocks,0);
  assert.throws(()=>core.check(core.exports.mup_lut_asset_begin(0,128)),/live mup_alloc/);
  assert.throws(()=>core.check(core.exports.mup_lut_asset_chunk(0,1048577)),/1 MiB/);
});
