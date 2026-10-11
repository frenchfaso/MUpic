import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

// Exercise the actual activation path with both allowed and suspended autoplay.
const app=await readFile(new URL('../site/app.js',import.meta.url),'utf8');
const source=app.slice(app.indexOf('async function ensureAudio()'),app.indexOf('function stopSidWorker('));
function fixture(allowed=false) {
  const listeners=new Map(),calls={resumes:0,setup:0,synced:0};let context;
  class AudioContext {
    constructor(){context=this;this.sampleRate=48000;this.state='suspended';this.pending=[];
      this.audioWorklet={addModule:async()=>{calls.setup++;}};}
    resume(){calls.resumes++;if(allowed){this.state='running';return Promise.resolve();}
      return new Promise(resolve=>this.pending.push(resolve));}
    unlock(){allowed=true;this.state='running';for(const resolve of this.pending.splice(0))resolve();}
  }
  const sandbox={window:{AudioContext,addEventListener(type,callback){listeners.set(type,callback);}},
    navigator:{userActivation:{isActive:true}},AudioWorkletNode:class{constructor(){this.port={};}connect(){}},
    console:{error(){}},assetUrl:name=>name,updateUI(){},cancelAudioActivation(){},attachAudioTransport(){},
    syncAudioTransport(){calls.synced++;},};
  vm.createContext(sandbox);
  vm.runInContext(`let audioContext=null,audioNode=null,audioSetup=null,audioRing=null;
    let audioRequest=0,audioEnabled=false,audioAutoStartPending=true;
    const AUDIO_SAMPLE_RATE=48000;${source}
    this.activate=()=>setAudio(true);
    this.snapshot=()=>({pending:audioAutoStartPending,enabled:audioEnabled,state:audioContext?.state});`,sandbox);
  return {sandbox,calls,listeners,get context(){return context;}};
}
test('permitted autoplay activates audio without a canvas gesture',async()=>{
  const f=fixture(true);await f.sandbox.activate();
  assert.equal(f.calls.resumes,1);assert.equal(f.calls.setup,1);assert.equal(f.calls.synced,1);
  assert.equal(f.sandbox.snapshot().pending,false);assert.equal(f.sandbox.snapshot().state,'running');
});
test('blocked autoplay keeps a page-wide touch retry available while resume is pending',async()=>{
  const f=fixture();const autoplay=f.sandbox.activate();
  assert.equal(f.sandbox.snapshot().pending,true);assert.equal(f.calls.synced,0);
  assert.ok(f.listeners.has('pointerup'));
  f.context.unlock();f.listeners.get('pointerup')({isTrusted:true,pointerType:'touch'});
  await autoplay;await new Promise(resolve=>setImmediate(resolve));
  assert.equal(f.calls.resumes,2);assert.equal(f.calls.setup,1);
  assert.equal(f.calls.synced,1,'the stale autoplay attempt does not enable a second clock');
  assert.equal(f.sandbox.snapshot().pending,false);
});
test('untrusted events, inactive activation and shortcut keys do not consume the retry',()=>{
  const f=fixture();const key=f.listeners.get('keydown');
  for(const event of [{isTrusted:false},{isTrusted:true,code:'Escape'},
    {isTrusted:true,metaKey:true},{isTrusted:true,repeat:true}])key(event);
  f.sandbox.navigator.userActivation.isActive=false;key({isTrusted:true,code:'ArrowDown'});
  assert.equal(f.calls.resumes,0);assert.equal(f.sandbox.snapshot().pending,true);
});
