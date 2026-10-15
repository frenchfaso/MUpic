import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {videoPlan,asciiTitle,interleavePcm,automaticGainDb,audioTrackPresence,audioSourceSampleRate,decodePcm,decodePcm8Source,decodePcmSource,awaitAbort,SAMPLE_RATE,MAX_FRAMES} from '../site/media.mjs';
import {CONTENT_FPS} from '../site/mup-decoder.mjs';
test('16:9 fits the native SD active area without stretching',()=>{
  const plan=videoPlan(60,1920,1080);assert.equal(plan.width,356);assert.equal(plan.height,200);assert.equal(plan.left,2);assert.equal(plan.top,0);
});
test('portrait and4:3 sources retain aspect ratio and centered black bars',()=>{
  assert.deepEqual({...videoPlan(1,1080,1920),frameCount:undefined,duration:undefined,estimatedBytes:undefined},
    {frameCount:undefined,duration:undefined,estimatedBytes:undefined,width:113,height:200,left:123,top:0});
  const plan=videoPlan(1,640,480);assert.equal(plan.width,267);assert.equal(plan.height,200);assert.equal(plan.left,46);
});
test('FAT32 size limit rejects oversized or empty videos',()=>{
  assert.equal(videoPlan(MAX_FRAMES/CONTENT_FPS,356,200).frameCount,MAX_FRAMES);
  assert.equal(videoPlan(MAX_FRAMES/CONTENT_FPS,356,200).estimatedBytes,4294836224);
  assert.throws(()=>videoPlan((MAX_FRAMES+1)/CONTENT_FPS,356,200),/FAT32/);
  assert.throws(()=>videoPlan(0,356,200),/duration/);assert.throws(()=>videoPlan(0.03,356,200),/0.12/);
});
test('audio gain saturates signed16-bit samples and interleaves stereo',()=>{
  const pcm=interleavePcm(Float32Array.of(-1,0.5,2),Float32Array.of(1,-0.5,-2));
  const view=new DataView(pcm.buffer);assert.deepEqual(Array.from({length:6},(_,i)=>view.getInt16(i*2,true)),[-32768,32767,16384,-16384,32767,-32768]);
});
test('automatic gain keeps stereo balance and a 1 dB headroom after resampling',()=>{
  const left=Float32Array.of(0,0.25,-0.125),right=Float32Array.of(0.5,-0.25,0.125);
  const gain=automaticGainDb(left,right),pcm=interleavePcm(left,right,gain),view=new DataView(pcm.buffer);
  assert.ok(Math.abs(gain-(20*Math.log10(2)-1))<1e-10);
  assert.equal(view.getInt16(2,true),Math.round(32768*10**(-1/20)));
  assert.equal(view.getInt16(4,true),Math.round(16384*10**(-1/20)));
  const loud=Float32Array.of(-2,2),reduced=interleavePcm(loud,loud,automaticGainDb(loud,loud));
  assert.ok(Math.abs(new DataView(reduced.buffer).getInt16(0,true))<32767);
});
test('automatic gain preserves silence and rejects invalid samples',()=>{
  const silence=new Float32Array(8);assert.equal(automaticGainDb(silence,silence),0);
  assert.deepEqual(interleavePcm(silence,silence,automaticGainDb(silence,silence)),new Uint8Array(32));
  assert.throws(()=>automaticGainDb(Float32Array.of(NaN),Float32Array.of(0)),/invalid samples/);
});
function mp4Box(type,bytes=new Uint8Array(),large=false) {
  const header=new Uint8Array(large?16:8),view=new DataView(header.buffer);
  view.setUint32(0,large?1:header.length+bytes.length);header.set(new TextEncoder().encode(type),4);
  if(large)view.setBigUint64(8,BigInt(header.length+bytes.length));
  return new Uint8Array([...header,...bytes]);
}
function mp4Track(type) {
  const handler=new Uint8Array(12);handler.set(new TextEncoder().encode(type),8);
  return mp4Box('trak',mp4Box('mdia',mp4Box('hdlr',handler)));
}
function mp4(...tracks) {
  return new Blob([mp4Box('ftyp',new TextEncoder().encode('isom')),mp4Box('mdat',Uint8Array.of(1,2,3)),mp4Box('moov',new Uint8Array(tracks.flatMap(bytes=>Array.from(bytes))),true)]);
}
function ebml(id,bytes=new Uint8Array(),unknown=false) {
  assert.ok(bytes.length<127);
  return new Uint8Array([...id,unknown?255:128|bytes.length,...bytes]);
}
function webm(types,unknownSegment=false) {
  const entries=types.flatMap(type=>Array.from(ebml([0xae],type===undefined?new Uint8Array():ebml([0x83],Uint8Array.of(type)))));
  return new Blob([ebml([0x1a,0x45,0xdf,0xa3]),ebml([0x18,0x53,0x80,0x67],ebml([0x16,0x54,0xae,0x6b],Uint8Array.from(entries)),unknownSegment)]);
}
function bytes(...parts) {return Uint8Array.from(parts.flatMap(part=>Array.from(part)));}
function aacTrack(rate,asc=Uint8Array.of(0x12,0x10),{codec='mp4a',version=0,wave=false}={}) {
  const header=new Uint8Array(28+(version===1?16:0)),view=new DataView(header.buffer);view.setUint16(8,version);view.setUint16(16,2);view.setUint16(18,16);view.setUint32(24,rate*65536);
  const descriptor=(tag,payload)=>Uint8Array.of(tag,payload.length,...payload);
  const config=new Uint8Array(13);config[0]=0x40;config[1]=0x15;
  let esds=mp4Box('esds',bytes(new Uint8Array(4),descriptor(3,bytes(Uint8Array.of(0,1,0),descriptor(4,bytes(config,descriptor(5,asc)))))));
  if(wave)esds=mp4Box('wave',esds);
  const count=Uint8Array.of(0,0,0,0,0,0,0,1),handler=new Uint8Array(12);handler.set(new TextEncoder().encode('soun'),8);
  return mp4Box('trak',mp4Box('mdia',bytes(mp4Box('hdlr',handler),mp4Box('minf',mp4Box('stbl',mp4Box('stsd',bytes(count,mp4Box(codec,bytes(header,esds)))))))));
}
function aacBits(...fields) {
  const bits=fields.map(([value,size])=>value.toString(2).padStart(size,'0')).join(''),result=new Uint8Array(Math.ceil(bits.length/8));
  for(let bit=0;bit<bits.length;bit++)if(bits[bit]==='1')result[bit>>3]|=1<<(7-(bit&7));
  return result;
}
function webmAudio(codec,rate,unknownSegment=false) {
  const frequency=new Uint8Array(8);new DataView(frequency.buffer).setFloat64(0,rate);
  const entry=ebml([0xae],bytes(ebml([0x83],Uint8Array.of(2)),ebml([0x86],new TextEncoder().encode(codec)),ebml([0xe1],ebml([0xb5],frequency))));
  return new Blob([ebml([0x1a,0x45,0xdf,0xa3]),ebml([0x18,0x53,0x80,0x67],ebml([0x16,0x54,0xae,0x6b],entry),unknownSegment)]);
}
test('PCM8 reads original AAC rates, including MOV wave entries and signalled HE-AAC',async()=>{
  assert.equal(await audioSourceSampleRate(mp4(aacTrack(44100))),44100);
  assert.equal(await audioSourceSampleRate(mp4(aacTrack(44100,Uint8Array.of(0x12,0x10),{version:1,wave:true}))),44100);
  const he=aacBits([5,5],[7,4],[2,4],[4,4],[2,5],[0,3]);
  assert.equal(await audioSourceSampleRate(mp4(aacTrack(22050,he))),44100);
  const extension=aacBits([2,5],[7,4],[2,4],[0,3],[0x2b7,11],[5,5],[1,1],[4,4]);
  assert.equal(await audioSourceSampleRate(mp4(aacTrack(22050,extension))),44100);
  const explicit=aacBits([2,5],[15,4],[32000,24],[2,4],[0,3]);
  assert.equal(await audioSourceSampleRate(mp4(aacTrack(32000,explicit))),32000);
});
test('PCM8 uses the 48 kHz Opus decoded stream rate and Vorbis SamplingFrequency',async()=>{
  assert.equal(await audioSourceSampleRate(webmAudio('A_OPUS',24000,true)),48000);
  assert.equal(await audioSourceSampleRate(webmAudio('A_VORBIS',32000)),32000);
  assert.equal(await audioSourceSampleRate(mp4(mp4Track('vide'))),null);
  assert.equal(await audioSourceSampleRate(webm([1],true)),null);
});
test('PCM8 metadata from genuine FFmpeg AAC and Opus containers preserves source rates',async()=>{
  // Generated from 0.04 s of stereo silence; these contain full codec metadata.
  for(const [filename,expected] of [['rate-aac-44100.mp4',44100],['rate-opus-48000.webm',48000]]) {
    const source=new Blob([await readFile(new URL(`./fixtures/${filename}`,import.meta.url))]);
    assert.equal(await audioSourceSampleRate(source),expected);
  }
});
test('PCM8 rejects unknown, truncated, ambiguous and unsupported stream metadata explicitly',async()=>{
  for(const source of [new Blob(['unknown source']),mp4(mp4Track('vide'),mp4Track('soun')),mp4(aacTrack(48000)),mp4(aacTrack(44100,Uint8Array.of(0x12,0x10),{codec:'ac-3'})),webmAudio('A_FLAC',44100)])
    await assert.rejects(audioSourceSampleRate(source),/native CLI/);
  const truncated=mp4(aacTrack(44100));await assert.rejects(audioSourceSampleRate(truncated.slice(0,truncated.size-1)),/native CLI/);
  await assert.rejects(audioSourceSampleRate(webmAudio('A_VORBIS',96000)),/96000 Hz.*Resample/);
  for(const rate of [NaN,Infinity,-Infinity,44100.5])await assert.rejects(audioSourceSampleRate(webmAudio('A_VORBIS',rate)),/Invalid audio frequency/);
  for(const rate of [0,-1,7999,48001])await assert.rejects(audioSourceSampleRate(webmAudio('A_VORBIS',rate)),/8 to 48 kHz/);
});
test('MP4 and MOV handler metadata distinguishes absent audio from undecodable tracks',async()=>{
  assert.equal(await audioTrackPresence(mp4(mp4Track('vide'))),false);
  assert.equal(await audioTrackPresence(mp4(mp4Track('vide'),mp4Track('soun'))),true);
  assert.equal(await audioTrackPresence(mp4(mp4Track('vide'),mp4Box('trak'))),null);
  assert.equal(await audioTrackPresence(mp4(mp4Track('vide'),mp4Track('xxxx'))),null);
  assert.equal(await audioTrackPresence(mp4(mp4Track('vide'),mp4Track('sbtl'))),false);
  assert.equal(await audioTrackPresence(mp4(mp4Track('meta'))),null);
  const truncated=mp4(mp4Track('vide'));assert.equal(await audioTrackPresence(truncated.slice(0,truncated.size-1)),null);
});
test('WebM finite and streaming segments detect audio without inspecting video clusters',async()=>{
  assert.equal(await audioTrackPresence(webm([1])),false);
  assert.equal(await audioTrackPresence(webm([1],true)),false);
  assert.equal(await audioTrackPresence(webm([1,2],true)),true);
  assert.equal(await audioTrackPresence(webm([1,undefined])),null);
  assert.equal(await audioTrackPresence(webm([1,0])),null);
  assert.equal(await audioTrackPresence(webm([1,17])),false);
  assert.equal(await audioTrackPresence(new Blob(['unrecognized source'])),null);
});
test('confirmed video-only inputs encode silent PCM without requiring browser audio decoding',async()=>{
  const duration=0.12,pcm=await decodePcm(mp4(mp4Track('vide')),duration);
  assert.equal(pcm.length,Math.round(duration*SAMPLE_RATE)*4);assert.ok(pcm.every(sample=>sample===0));
});
function mockAudio(t,{left,right,reject=false,decode,render}) {
  const originalContext=globalThis.AudioContext,originalOffline=globalThis.OfflineAudioContext;
  const calls={closed:0,channels:0,samples:0,rate:0};
  globalThis.AudioContext=class {
    async decodeAudioData(){if(decode)return decode();if(reject)throw new Error('unsupported codec');return {};}
    async close(){calls.closed++;}
  };
  globalThis.OfflineAudioContext=class {
    constructor(channels,samples,rate){Object.assign(calls,{channels,samples,rate});}
    createBufferSource(){return {connect(){},start(){}};}
    async startRendering(){if(render)return render();return {getChannelData:channel=>channel?right:left};}
  };
  t.after(()=>{if(originalContext===undefined)delete globalThis.AudioContext;else globalThis.AudioContext=originalContext;
    if(originalOffline===undefined)delete globalThis.OfflineAudioContext;else globalThis.OfflineAudioContext=originalOffline;});
  return calls;
}
function mockPcm8Audio(t,{left,right,decode,render}) {
  const originalContext=globalThis.AudioContext,originalOffline=globalThis.OfflineAudioContext,calls={closed:0};
  globalThis.AudioContext=class {
    constructor(options){calls.decodeRate=options.sampleRate;}
    async decodeAudioData(){return decode?decode():{sampleRate:calls.decodeRate};}
    async close(){calls.closed++;}
  };
  globalThis.OfflineAudioContext=class {
    constructor(channels,samples,rate){Object.assign(calls,{channels,samples,rate});}
    createBufferSource(){const source={playbackRate:{value:1},connect(){},start(){calls.playbackRate=source.playbackRate.value;}};return source;}
    async startRendering(){return render?render():{getChannelData:channel=>channel?right:left};}
  };
  t.after(()=>{if(originalContext===undefined)delete globalThis.AudioContext;else globalThis.AudioContext=originalContext;
    if(originalOffline===undefined)delete globalThis.OfflineAudioContext;else globalThis.OfflineAudioContext=originalOffline;});
  return calls;
}
test('PCM8 retains original frequency and sends unquantized stereo floats to the shared DSP',async t=>{
  const left=Float32Array.of(0,0.125,0.25,-0.75),right=Float32Array.of(0,-0.125,0.5,0.75),calls=mockPcm8Audio(t,{left,right});
  const rate=6250000/(Math.round(6250000/44100-1.5)+1.5),{pcm,sampleRate}=await decodePcm8Source(mp4(aacTrack(44100)),4/rate);
  assert.equal(sampleRate,44100);assert.equal(calls.decodeRate,44100);assert.equal(calls.samples,4);assert.equal(calls.channels,2);assert.equal(calls.rate,Math.round(rate));assert.equal(calls.closed,1);
  assert.ok(Math.abs(calls.playbackRate-calls.rate/rate)<1e-15);
  const view=new DataView(pcm.buffer);
  assert.deepEqual(Array.from({length:8},(_,sample)=>view.getFloat32(sample*4,true)),[0,0,0.125,-0.125,0.25,0.5,-0.75,0.75]);
});
test('PCM8 supports the 8 kHz sampler rate below OfflineAudioContext minimum without timing drift',async t=>{
  const calls=mockPcm8Audio(t,{left:Float32Array.of(0.25),right:Float32Array.of(-0.25)}),rate=6250000/(Math.round(6250000/8000-1.5)+1.5);
  const {pcm,sampleRate}=await decodePcm8Source(mp4(aacTrack(8000,Uint8Array.of(0x15,0x90))),1/rate);
  assert.equal(sampleRate,8000);assert.equal(pcm.length,8);assert.equal(calls.rate,8000);assert.ok(rate<8000);assert.equal(calls.playbackRate,8000/rate);
});
test('PCM8 video-only sources produce empty audio without requiring browser audio contexts',async()=>{
  assert.deepEqual(await decodePcm8Source(mp4(mp4Track('vide')),0.12),{pcm:new Uint8Array(0),sampleRate:SAMPLE_RATE});
  assert.deepEqual(await decodePcmSource(mp4(mp4Track('vide')),0.12),{pcm:new Uint8Array(0),sampleRate:44100});
});
test('PCM16 preserves source rate and the shared hardware clock without browser-side quantization',async t=>{
  const left=Float32Array.of(0.25,-0.75),right=Float32Array.of(-0.5,0.125),calls=mockPcm8Audio(t,{left,right});
  const rateNumerator=6250000,rateDenominator=142,actual=rateNumerator/rateDenominator;
  const result=await decodePcmSource(mp4(aacTrack(44100)),2/actual,{audioInfo:async rate=>{
    assert.equal(rate,44100);return {rateNumerator,rateDenominator};
  }});
  assert.equal(result.sampleRate,44100);assert.equal(calls.decodeRate,44100);
  assert.equal(calls.samples,2);assert.equal(calls.playbackRate,calls.rate/actual);
  const view=new DataView(result.pcm.buffer);
  assert.deepEqual(Array.from({length:4},(_,i)=>view.getFloat32(i*4,true)),[0.25,-0.5,-0.75,0.125]);
});
test('profile plans use shared fitting, cadence and per-profile duration limits',()=>{
  const profile={id:4,fps:1022730/(65*263*4),ticks:4,maxFrames:81915},fit={left:0,top:15,width:360,height:169};
  const plan=videoPlan(2,1920,1080,{profile,fit});
  assert.equal(plan.frameCount,30);assert.equal(plan.duration,30/profile.fps);
  for(const key of ['left','top','width','height'])assert.equal(plan[key],fit[key]);
  assert.equal(plan.estimatedBytes,65536+6*262144);
  assert.throws(()=>videoPlan((profile.maxFrames+1)/profile.fps,1920,1080,{profile,fit}),/FAT32/);
});
test('audio decoding automatically normalizes rendered PCM and retains the full planned duration',async t=>{
  const left=Float32Array.of(0,0.125,0.25,0),right=Float32Array.of(0,-0.125,-0.25,0),calls=mockAudio(t,{left,right});
  const pcm=await decodePcm(mp4(mp4Track('vide'),mp4Track('soun')),4/SAMPLE_RATE);
  assert.deepEqual(calls,{closed:1,channels:2,samples:4,rate:SAMPLE_RATE});
  assert.deepEqual(pcm,interleavePcm(left,right,automaticGainDb(left,right)));
  assert.equal(pcm.length,16);assert.equal(new DataView(pcm.buffer).getInt16(12,true),0);
});
test('explicit numeric gain retains the previous conversion semantics',async t=>{
  const left=Float32Array.of(0.25),right=Float32Array.of(-0.25);mockAudio(t,{left,right});
  assert.deepEqual(await decodePcm(mp4(mp4Track('vide'),mp4Track('soun')),1/SAMPLE_RATE,{gainDb:0}),interleavePcm(left,right,0));
});
test('unsupported audio is never silently discarded when tracks exist or metadata is unknown',async t=>{
  const calls=mockAudio(t,{reject:true});
  await assert.rejects(decodePcm(mp4(mp4Track('vide'),mp4Track('soun')),0.12),/could not decode.*audio/);
  await assert.rejects(decodePcm(new Blob(['unknown source format']),0.12),/could not decode.*audio/);
  assert.equal(calls.closed,2);
});
test('audio inspection and silent decoding respect cancellation',async()=>{
  const controller=new AbortController();controller.abort();
  await assert.rejects(audioTrackPresence(mp4(mp4Track('vide')),{signal:controller.signal}),{name:'AbortError'});
  await assert.rejects(decodePcm(mp4(mp4Track('vide')),0.12,{silent:true,signal:controller.signal}),{name:'AbortError'});
});
function deferred() {
  let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});
  return {promise,resolve,reject};
}
test('abortable waits reject promptly, remove listeners and consume later rejection',{timeout:1000},async t=>{
  const controller=new AbortController(),operation=deferred();
  const remove=t.mock.method(controller.signal,'removeEventListener');
  const waiting=awaitAbort(operation.promise,controller.signal);controller.abort();
  await assert.rejects(waiting,{name:'AbortError'});assert.equal(remove.mock.calls.length,1);
  operation.reject(new Error('decoder failed after cancellation'));
  await new Promise(resolve=>setImmediate(resolve));
  const alreadyCancelled=deferred(),cancelled=awaitAbort(alreadyCancelled.promise,controller.signal);
  await assert.rejects(cancelled,{name:'AbortError'});alreadyCancelled.reject(new Error('late read failure'));
  await new Promise(resolve=>setImmediate(resolve));
});
test('abortable waits preserve results and errors and clean listeners after normal completion',async t=>{
  const controller=new AbortController(),remove=t.mock.method(controller.signal,'removeEventListener');
  assert.equal(await awaitAbort(Promise.resolve(42),controller.signal),42);
  const error=new Error('original error');await assert.rejects(awaitAbort(Promise.reject(error),controller.signal),error);
  assert.equal(remove.mock.calls.length,2);controller.abort();
});
test('container metadata reads cancel before an outstanding file read completes',{timeout:1000},async()=>{
  const controller=new AbortController(),read=deferred(),started=deferred();
  const file={size:16,slice(){return {arrayBuffer(){started.resolve();return read.promise;}};}};
  const inspecting=audioTrackPresence(file,{signal:controller.signal});await started.promise;controller.abort();
  await assert.rejects(inspecting,{name:'AbortError'});read.reject(new Error('late metadata read failure'));
});
test('audio file reads cancel promptly and close their AudioContext',{timeout:1000},async t=>{
  const controller=new AbortController(),read=deferred(),started=deferred(),calls=mockAudio(t,{});
  const file={arrayBuffer(){started.resolve();return read.promise;}};
  const decoding=decodePcm(file,0.12,{signal:controller.signal});await started.promise;controller.abort();
  await assert.rejects(decoding,{name:'AbortError'});assert.equal(calls.closed,1);
  read.reject(new Error('late source read failure'));
});
test('audio decoding cancels before decodeAudioData settles and closes its AudioContext',{timeout:1000},async t=>{
  const controller=new AbortController(),decode=deferred(),started=deferred();
  const calls=mockAudio(t,{decode(){started.resolve();return decode.promise;}});
  const decoding=decodePcm(mp4(mp4Track('vide'),mp4Track('soun')),0.12,{signal:controller.signal});
  await started.promise;controller.abort();await assert.rejects(decoding,{name:'AbortError'});
  assert.equal(calls.closed,1);decode.reject(new Error('late decode failure'));
});
test('offline PCM rendering cancels before startRendering settles',{timeout:1000},async t=>{
  const controller=new AbortController(),render=deferred(),started=deferred();
  const calls=mockAudio(t,{render(){started.resolve();return render.promise;}});
  const decoding=decodePcm(mp4(mp4Track('vide'),mp4Track('soun')),0.12,{signal:controller.signal});
  await started.promise;controller.abort();await assert.rejects(decoding,{name:'AbortError'});
  assert.equal(calls.closed,1);render.reject(new Error('late render failure'));
});
test('PCM8 cancellation interrupts source decode and closes its AudioContext',{timeout:1000},async t=>{
  const controller=new AbortController(),decode=deferred(),started=deferred(),calls=mockPcm8Audio(t,{decode(){started.resolve();return decode.promise;}});
  const decoding=decodePcm8Source(mp4(aacTrack(44100)),0.12,{signal:controller.signal});
  await started.promise;controller.abort();await assert.rejects(decoding,{name:'AbortError'});assert.equal(calls.closed,1);decode.reject(new Error('late decode failure'));
});
test('PCM8 cancellation interrupts offline rendering promptly',{timeout:1000},async t=>{
  const controller=new AbortController(),render=deferred(),started=deferred(),calls=mockPcm8Audio(t,{render(){started.resolve();return render.promise;}});
  const decoding=decodePcm8Source(mp4(aacTrack(44100)),0.12,{signal:controller.signal});
  await started.promise;controller.abort();await assert.rejects(decoding,{name:'AbortError'});assert.equal(calls.closed,1);render.reject(new Error('late render failure'));
});
test('container titles are ASCII and fit the64-byte field',()=>{
  assert.equal(asciiTitle('Café 🌞.mp4'),'Cafe');assert.equal(asciiTitle('😀.webm'),'MY VIDEO');assert.equal(asciiTitle('x'.repeat(100)+'.mp4').length,63);
});
