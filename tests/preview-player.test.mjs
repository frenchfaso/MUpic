import test from 'node:test';
import assert from 'node:assert/strict';
import {MupPlayer} from '../site/preview-player.mjs';
import {CONTENT_FPS,NTSC_HZ} from '../site/mup-decoder.mjs';

const flush=()=>new Promise(resolve=>setImmediate(resolve));
function deferred() {let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
function environment({audio=true,resume,decode,frame}={}) {
  let wall=0,nextHandle=0;const frames=new Map(),cancelled=[],drawn=[],updates=[],errors=[],sources=[];
  const calls={contexts:0,resumes:0,audio:0,frames:[],active:0,maxActive:0,closed:0};
  const context={currentTime:0,state:'suspended',destination:{},
    resume(){calls.resumes++;if(resume)return resume.promise.then(()=>{this.state='running';});this.state='running';return Promise.resolve();},
    createBufferSource(){const source={playbackRate:{value:1},stopped:0,disconnected:0,
      connect(destination){this.destination=destination;},start(when,offset){this.startAt={when,offset};},
      stop(){this.stopped++;},disconnect(){this.disconnected++;}};sources.push(source);return source;},
    async close(){calls.closed++;this.state='closed';}};
  const makePreview=(id='A',duration=3)=>({id,header:{duration,frameCount:Math.ceil(duration*CONTENT_FPS),sampleRate:22085,audioHz:22000},
    async frame(index){calls.frames.push({id,index});calls.active++;calls.maxActive=Math.max(calls.maxActive,calls.active);
      try {if(frame)await frame(id,index);return {id,index};}finally{calls.active--;}}
    ,async audioBuffer(){calls.audio++;return decode?decode.promise:{duration:10,id};}});
  const player=new MupPlayer({drawFrame:rgba=>drawn.push(rgba),onUpdate:snapshot=>updates.push(snapshot),onError:error=>errors.push(error),
    createAudioContext(){calls.contexts++;return audio?context:null;},now:()=>wall,
    requestFrame(callback){const handle=++nextHandle;frames.set(handle,callback);return handle;},
    cancelFrame(handle){cancelled.push(frames.get(handle));frames.delete(handle);}});
  const advance=async seconds=>{wall+=seconds;if(context.state==='running')context.currentTime+=seconds;
    const pending=[...frames.values()];frames.clear();for(const callback of pending)callback();await flush();};
  return {player,context,makePreview,advance,calls,drawn,updates,errors,sources,frames,cancelled,setWall:value=>{wall=value;}};
}
test('MUP playback has its own timeline and uses its header duration and frame rate',async()=>{
  const env=environment({audio:false}),preview=env.makePreview('MUP',2);await env.player.load(preview);
  const original={currentTime:99,duration:999,paused:false};await env.player.play();await env.advance(0.5);
  assert.deepEqual(env.player.snapshot,{ready:true,playing:true,starting:false,time:0.5,duration:2});
  assert.equal(env.drawn.at(-1).index,Math.floor(0.5*preview.header.frameCount/preview.header.duration));assert.equal(original.currentTime,99);
  original.currentTime=0;original.paused=true;await env.advance(0.5);assert.equal(env.player.snapshot.time,1);
  assert.equal(env.calls.maxActive,1);env.player.dispose();
});
test('pause retains position, resume continues it, and stop resets and paints frame zero',async()=>{
  const env=environment();await env.player.load(env.makePreview());await env.player.play();await env.advance(0.6);
  env.player.pause();await flush();assert.equal(env.player.snapshot.time,0.6);assert.equal(env.player.snapshot.playing,false);
  assert.equal(env.sources[0].stopped,1);assert.equal(env.sources[0].disconnected,1);
  await env.advance(1);assert.equal(env.player.snapshot.time,0.6);await env.player.play();await env.advance(0.4);
  assert.ok(Math.abs(env.player.snapshot.time-1)<1e-12);await env.player.stop();assert.equal(env.player.snapshot.time,0);
  assert.equal(env.player.snapshot.playing,false);assert.equal(env.drawn.at(-1).index,0);env.player.dispose();
});
test('paused and playing seeks clamp correctly and restart sound at the matching sample offset',async()=>{
  const env=environment();await env.player.load(env.makePreview());await env.player.seek(1.25);
  assert.equal(env.player.snapshot.time,1.25);assert.equal(env.player.snapshot.playing,false);
  await env.player.play();const ratio=22000/22085;
  assert.equal(env.sources[0].playbackRate.value,ratio);assert.deepEqual(env.sources[0].startAt,{when:0,offset:1.25*ratio});
  await env.player.seek(2);assert.equal(env.player.snapshot.playing,true);assert.equal(env.player.snapshot.time,2);
  assert.equal(env.sources[1].startAt.offset,2*ratio);assert.equal(env.sources[0].stopped,1);
  await env.player.seek(100);assert.equal(env.player.snapshot.time,3);assert.equal(env.player.snapshot.playing,false);
  assert.equal(env.drawn.at(-1).index,env.player.preview.header.frameCount-1);
  await env.player.seek(-2);assert.equal(env.player.snapshot.time,0);assert.equal(env.player.snapshot.playing,false);env.player.dispose();
});
test('EOF displays the final frame and replay starts from zero',async()=>{
  const env=environment({audio:false});await env.player.load(env.makePreview('A',1));await env.player.play();await env.advance(5);
  assert.equal(env.player.snapshot.time,1);assert.equal(env.player.snapshot.playing,false);assert.equal(env.frames.size,0);
  assert.equal(env.drawn.at(-1).index,env.player.preview.header.frameCount-1);
  await env.player.play();assert.equal(env.player.snapshot.time,0);assert.equal(env.drawn.at(-1).index,0);
  await env.advance(0.25);assert.equal(env.player.snapshot.time,0.25);env.player.dispose();
});
test('resume happens synchronously, repeated play deduplicates, and pending audio never advances time',async()=>{
  const resume=deferred(),decode=deferred(),env=environment({resume,decode});await env.player.load(env.makePreview());
  const first=env.player.play();assert.equal(env.calls.resumes,1);assert.equal(env.player.snapshot.starting,true);
  const second=env.player.play();assert.equal(first,second);assert.equal(env.calls.contexts,1);
  await flush();assert.equal(env.calls.audio,1);await env.advance(10);assert.equal(env.player.snapshot.time,0);assert.equal(env.frames.size,0);
  resume.resolve();await flush();assert.equal(env.player.snapshot.playing,false);
  decode.resolve({duration:10});assert.equal(await first,true);assert.equal(env.player.snapshot.time,0);
  await env.advance(0.5);assert.equal(env.player.snapshot.time,0.5);env.player.dispose();
});
test('the AudioContext clock controls elapsed time rather than unrelated wall time',async()=>{
  const env=environment();await env.player.load(env.makePreview());await env.player.play();env.setWall(1000);
  assert.equal(env.player.snapshot.time,0);env.context.currentTime=0.75;assert.equal(env.player.snapshot.time,0.75);
  env.player.pause();env.context.currentTime=10;assert.equal(env.player.snapshot.time,0.75);env.player.dispose();
});
test('pause promptly cancels pending start and a late audio result cannot start sound',async()=>{
  const decode=deferred(),env=environment({decode});await env.player.load(env.makePreview());
  const playing=env.player.play();await flush();env.player.pause();assert.equal(await playing,false);
  assert.equal(env.player.snapshot.starting,false);decode.resolve({duration:10});await flush();
  assert.equal(env.sources.length,0);assert.equal(env.player.snapshot.time,0);assert.equal(env.frames.size,0);env.player.dispose();
});
test('seeking during pending playback cancels the old start and resumes only at the new position',async()=>{
  const decode=deferred(),env=environment({decode});await env.player.load(env.makePreview());
  const oldPlay=env.player.play();await flush();const sought=env.player.seek(1.5);
  assert.equal(await oldPlay,false);assert.equal(env.player.snapshot.time,1.5);assert.equal(env.player.snapshot.starting,true);
  await env.advance(5);assert.equal(env.player.snapshot.time,1.5);assert.equal(env.sources.length,0);
  decode.resolve({duration:10});assert.equal(await sought,true);assert.equal(env.sources.length,1);
  assert.equal(env.sources[0].startAt.offset,1.5*(22000/22085));assert.equal(env.calls.audio,1);env.player.dispose();
});
test('clearing and replacing a source guards late audio and cancelled animation callbacks',async()=>{
  const decode=deferred(),env=environment({decode});await env.player.load(env.makePreview('old'));
  const playing=env.player.play();await flush();env.player.clear();assert.equal(await playing,false);
  await env.player.load(env.makePreview('new'));decode.resolve({duration:10,id:'old'});await flush();
  assert.equal(env.player.buffer,null);assert.equal(env.sources.length,0);assert.equal(env.drawn.at(-1).id,'new');
  await env.player.play();await env.advance(0.2);env.player.pause();await env.player.play();
  const before={...env.player.snapshot},scheduled=env.frames.size;env.cancelled.at(-1)?.();
  assert.deepEqual(env.player.snapshot,before);assert.equal(env.frames.size,scheduled);env.player.dispose();
});
test('frame decoding is serialized and coalesced; stale frames cannot paint a replacement',async()=>{
  const delayed=deferred();let block=false;
  const env=environment({audio:false,frame:async(id,index)=>{if(id==='old'&&index>0&&block)await delayed.promise;}});
  await env.player.load(env.makePreview('old'));await env.player.play();block=true;
  await env.advance(0.2);await env.advance(0.2);await env.advance(0.2);assert.equal(env.calls.active,1);
  const loading=env.player.load(env.makePreview('new'));assert.equal(env.calls.active,1);
  delayed.resolve();await loading;assert.equal(env.calls.maxActive,1);assert.equal(env.drawn.at(-1).id,'new');
  assert.equal(env.drawn.filter(value=>value.id==='old').length,1);env.player.dispose();
});
test('a newer paused seek replaces a pending seek without drawing the stale frame',async()=>{
  const delayed=deferred();const env=environment({audio:false,frame:async(id,index)=>{if(index===17)await delayed.promise;}});
  await env.player.load(env.makePreview());const first=env.player.seek(1);await flush();const latest=env.player.seek(2);
  delayed.resolve();assert.equal(await first,false);assert.equal(await latest,true);
  assert.equal(env.drawn.at(-1).index,34);assert.equal(env.calls.maxActive,1);env.player.dispose();
});
test('native PAL timing draws changed MUP frames only, independently of display refresh and skipped RAFs',async()=>{
  const env=environment({audio:false}),preview=env.makePreview();
  preview.header.frameCount=60;preview.header.duration=60/CONTENT_FPS;
  await env.player.load(preview);await env.player.play();
  for(let refresh=0;refresh<6;refresh++)await env.advance(1/120);
  assert.deepEqual(env.calls.frames.map(value=>value.index),[0]);
  await env.advance(1/120);assert.deepEqual(env.calls.frames.map(value=>value.index),[0]);
  await env.advance(1/120);assert.deepEqual(env.calls.frames.map(value=>value.index),[0,1]);
  await env.advance(0.5);assert.equal(env.drawn.at(-1).index,Math.floor(env.player.snapshot.time*CONTENT_FPS));
  assert.equal(env.calls.frames.length,3);
  await env.advance(10);assert.equal(env.drawn.at(-1).index,59);assert.equal(env.player.snapshot.time,preview.header.duration);
  env.player.dispose();
});
test('NTSC four-refresh playback follows its own frame cadence and PCM16 source clock',async()=>{
  const env=environment(),preview=env.makePreview('NTSC'),fps=NTSC_HZ/4;
  preview.header={duration:30/fps,frameCount:30,sampleRate:48000,audioHz:6250000/130};
  await env.player.load(preview);await env.player.play();
  await env.advance(1/60);await env.advance(1/60);await env.advance(1/60);
  assert.deepEqual(env.calls.frames.map(frame=>frame.index),[0]);
  await env.advance(1/50);assert.equal(env.drawn.at(-1).index,1);
  await env.player.seek(1);assert.equal(env.drawn.at(-1).index,14);
  assert.equal(env.sources.at(-1).startAt.offset,preview.header.audioHz/48000);
  assert.equal(env.sources.at(-1).playbackRate.value,preview.header.audioHz/48000);env.player.dispose();
});
test('playback errors are reported once and leave the player stopped',async()=>{
  const decode=deferred(),env=environment({decode});await env.player.load(env.makePreview());const playing=env.player.play();
  decode.reject(new Error('invalid audio'));assert.equal(await playing,false);assert.equal(env.errors.length,1);
  assert.match(env.errors[0].message,/invalid audio/);assert.equal(env.player.snapshot.playing,false);
  assert.equal(env.player.snapshot.starting,false);assert.equal(env.frames.size,0);env.player.dispose();
});
test('a frame decoder error stops active playback and is reported once',async()=>{
  const env=environment({audio:false,frame:async(id,index)=>{if(index>0)throw new Error('invalid MUP record');}});
  await env.player.load(env.makePreview());await env.player.play();await env.advance(0.25);
  assert.equal(env.errors.length,1);
  assert.equal(env.player.snapshot.playing,false);assert.equal(env.player.snapshot.starting,false);
  assert.equal(env.frames.size,0);env.player.dispose();
});
test('disposing cancels pending work, ignores late results and closes the AudioContext',async()=>{
  const decode=deferred(),env=environment({decode});await env.player.load(env.makePreview());const playing=env.player.play();await flush();
  env.player.dispose();assert.equal(await playing,false);decode.reject(new Error('late audio error'));await flush();
  assert.equal(env.calls.closed,1);assert.deepEqual(env.player.snapshot,{ready:false,playing:false,starting:false,time:0,duration:0});
  assert.equal(env.errors.length,0);assert.equal(await env.player.play(),false);
});
