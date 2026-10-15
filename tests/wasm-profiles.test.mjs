import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {EncoderCore} from '../site/wasm-core.mjs';
import {MupPreview,PAL_HZ,NTSC_HZ} from '../site/mup-decoder.mjs';

const wasmFile=new URL('../site/assets/encoder.wasm',import.meta.url);
const makeCore=async()=>new EncoderCore((await WebAssembly.instantiate(await readFile(wasmFile),{})).instance.exports);
const rowFrame=front=>Uint8Array.from({length:36000},(_,i)=>front[(i%180)*256+Math.floor(i/180)]);

test('shared WASM profile metadata and fit preserve source aspect for all four output combinations',async()=>{
  const core=await makeCore();
  for(const id of [1,2,3,4]) {
    const p=core.profileInfo(id),ntsc=id>2;
    assert.equal(p.id,id);assert.equal(p.ticks,ntsc?4:3);
    assert.equal(p.fps,(ntsc?NTSC_HZ:PAL_HZ)/p.ticks);
    assert.equal(p.maxFrames,Math.floor(65535/p.ticks));
    for(const [width,height] of [[1920,1080],[640,480],[1080,1920],[2560,1080]]) {
      const fit=core.fit(id,width,height),pixelAspect=p.pixelAspectNumerator/p.pixelAspectDenominator;
      assert.ok(fit.width===360||fit.height===200);
      assert.equal(fit.left,Math.floor((360-fit.width)/2));assert.equal(fit.top,Math.floor((200-fit.height)/2));
      const sourceAspect=width/height,resultAspect=fit.width*pixelAspect/fit.height;
      assert.ok(Math.abs(sourceAspect-resultAspect)<=pixelAspect/fit.height+sourceAspect/fit.height);
    }
  }
  assert.deepEqual(core.fit(2,1920,1080),{left:0,top:5,width:360,height:190});
  assert.deepEqual(core.fit(4,1920,1080),{left:0,top:15,width:360,height:169});
  assert.throws(()=>core.profileInfo(5),/profile/);
});

test('switching quantizer profiles enables the full 360-pixel canvas and retains legacy border behavior',async()=>{
  const core=await makeCore(),rgb=new Uint8Array(216000).fill(247);
  for(const id of [0,2,4,0,1,3]) {
    core.setProfile(id);const packed=core.quantize(rgb);
    for(let row=0;row<200;row++) {
      assert.equal(packed[row*180],id?0x11:0);assert.equal(packed[row*180+179],id?0x11:0);
      assert.equal(packed[row*180+1],0x11);
    }
  }
});

test('every video/audio combination emits independently decoded full-width frames and exact sampler timing',async()=>{
  for(const profile of [1,2,3,4])for(const audioFormat of ['pcm16','pcm8'])for(const sampleRate of [44100,48000]) {
    const core=await makeCore(),p=core.profileInfo(profile),audio=core.audioInfo(audioFormat,sampleRate),frames=8;
    const hardwareRate=audio.rateNumerator/audio.rateDenominator,samples=Math.ceil(frames/p.fps*hardwareRate);
    const raw=new Uint8Array(samples*8),view=new DataView(raw.buffer);
    for(let sample=0;sample<samples;sample++) {
      const value=0.75*Math.sin(sample*2*Math.PI*800/hardwareRate);
      view.setFloat32(sample*8,value,true);view.setFloat32(sample*8+4,-value/2,true);
    }
    core.begin(frames,raw,'PROFILE TEST',{audioFormat,sampleRate,profile});
    const parts=[],oracles=[];
    for(let frame=0;frame<frames;frame++) {
      const packed=new Uint8Array(36000).fill(0x11);
      for(let row=0;row<200;row++){packed[row*180]=(frame+1)*17;packed[row*180+179]=(frame+2)*17;}
      parts.push(...core.frame(packed));
      oracles.push(core.bytes(core.exports.mup_preview_ptr(),core.exports.mup_preview_len()).slice());
    }
    const done=core.finish(),preview=await MupPreview.open(new Blob([done.prefix,...parts,...done.chunks]));
    assert.equal(preview.header.profile,profile);assert.equal(preview.header.pcm8,audioFormat==='pcm8');
    assert.equal(preview.header.sampleRate,sampleRate);assert.equal(preview.header.audioHz,hardwareRate);
    assert.equal(preview.header.audioOffset,audio.audioOffset);
    assert.equal(preview.header.frameCount,frames);assert.equal(preview.header.ticksPerFrame,p.ticks);
    assert.ok(Math.abs(preview.header.duration-frames/p.fps)<1e-12);
    const totalSamples=preview.audioLengths.reduce((sum,n)=>sum+n,0)/preview.header.bytesPerSample;
    assert.equal(totalSamples,Math.round(preview.header.duration*hardwareRate));
    for(const frame of [0,1,7,4,2,6]) {
      await preview.frame(frame);assert.deepEqual(rowFrame(preview.state.front),oracles[frame]);
    }
    assert.ok(done.stats.peakSlots<=p.rateBudget);
    const buffer=await preview.audioBuffer({createBuffer(channels,count,rate){return {sampleRate:rate,data:[new Float32Array(count),new Float32Array(count)],getChannelData(channel){return this.data[channel];}};}});
    assert.equal(buffer.sampleRate,sampleRate);assert.ok(buffer.data[0].some(sample=>sample<0));
    assert.ok(buffer.data[0].every(sample=>Math.abs(sample)<0.95));
  }
});
