import test from 'node:test';
import assert from 'node:assert/strict';
import {videoPlan,asciiTitle,interleavePcm,automaticGainDb,audioTrackPresence,decodePcm,awaitAbort,SAMPLE_RATE,MAX_FRAMES} from '../site/media.mjs';
import {CONTENT_FPS} from '../site/mup-decoder.mjs';
test('16:9 fits the native SD active area without stretching',()=>{
  const plan=videoPlan(60,1920,1080);assert.equal(plan.width,356);assert.equal(plan.height,200);assert.equal(plan.left,2);assert.equal(plan.top,0);
});
test('portrait and4:3 sources retain aspect ratio and centered black bars',()=>{
  assert.deepEqual({...videoPlan(1,1080,1920),frameCount:undefined,duration:undefined,estimatedBytes:undefined},
    {frameCount:undefined,duration:undefined,estimatedBytes:undefined,width:113,height:200,left:123,top:0});
  const plan=videoPlan(1,640,480);assert.equal(plan.width,267);assert.equal(plan.height,200);assert.equal(plan.left,46);
});
test('player frame-count limits reject oversized or empty videos',()=>{
  assert.equal(videoPlan(MAX_FRAMES/CONTENT_FPS,356,200).frameCount,MAX_FRAMES);
  assert.throws(()=>videoPlan((MAX_FRAMES+1)/CONTENT_FPS,356,200),/21 minutes/);
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
test('container titles are ASCII and fit the64-byte field',()=>{
  assert.equal(asciiTitle('Café 🌞.mp4'),'Cafe');assert.equal(asciiTitle('😀.webm'),'MY VIDEO');assert.equal(asciiTitle('x'.repeat(100)+'.mp4').length,63);
});
