import {awaitAbort} from './media.mjs?v=81de3bb75e9c558f';

const defaultContext=()=>{
  const Context=globalThis.AudioContext||globalThis.webkitAudioContext;
  return Context?new Context():null;
};
const defaultRequest=callback=>globalThis.requestAnimationFrame?globalThis.requestAnimationFrame(callback):setTimeout(callback,16);
const defaultCancel=handle=>globalThis.cancelAnimationFrame?globalThis.cancelAnimationFrame(handle):clearTimeout(handle);

export class MupPlayer {
  constructor({drawFrame,onUpdate=()=>{},onError=()=>{},createAudioContext=defaultContext,
    now=()=>performance.now()/1000,requestFrame=defaultRequest,cancelFrame=defaultCancel}={}) {
    this.drawFrame=drawFrame;this.onUpdate=onUpdate;this.onError=onError;
    this.createAudioContext=createAudioContext;this.now=now;this.requestFrame=requestFrame;this.cancelFrame=cancelFrame;
    this.preview=null;this.context=null;this.source=null;this.buffer=null;this.audioLoading=null;
    this.position=0;this.playing=false;this.starting=false;this.disposed=false;
    this.serial=0;this.action=0;this.paintEpoch=0;this.pendingPlay=null;this.animation=null;
    this.pendingFrame=null;this.activeFrame=null;this.lastFrame=-1;
  }
  get snapshot() {
    return {ready:Boolean(this.preview)&&!this.disposed,playing:this.playing,starting:this.starting,
      time:this.currentTime(),duration:this.preview?.header.duration||0};
  }
  currentTime() {
    if(!this.playing) return this.position;
    const audioTime=this.context?.currentTime;
    const elapsed=this.audioAnchor!==null && Number.isFinite(audioTime) && this.context.state!=='closed'
      ?audioTime-this.audioAnchor:this.now()-this.wallAnchor;
    return Math.min(this.preview.header.duration,this.playPosition+Math.max(0,elapsed));
  }
  update() { if(!this.disposed)this.onUpdate(this.snapshot); }
  releaseSource() {
    const source=this.source;this.source=null;
    if(source){source.onended=null;try{source.stop();}catch{}try{source.disconnect();}catch{}}
  }
  halt() {
    this.position=this.currentTime();this.playing=false;this.starting=false;this.action++;
    this.pendingPlay?.controller.abort();this.pendingPlay=null;this.releaseSource();
    if(this.animation!==null){this.cancelFrame(this.animation);this.animation=null;}
    this.paintEpoch++;
    if(this.pendingFrame){this.pendingFrame.resolve(false);this.pendingFrame=null;}
  }
  async load(preview) {
    this.clear();if(this.disposed)return false;
    this.preview=preview;this.update();
    return this.paint(0);
  }
  clear() {
    this.halt();this.serial++;this.preview=null;this.buffer=null;this.audioLoading=null;
    this.position=0;this.lastFrame=-1;this.update();
  }
  frameAt(time) {
    const {frameCount,duration}=this.preview.header;
    return Math.min(frameCount-1,Math.max(0,Math.floor(time*frameCount/duration)));
  }
  paint(index) {
    if(!this.preview||this.disposed)return Promise.resolve(false);
    if(this.lastFrame===index)return Promise.resolve(true);
    const matching=job=>job&&job.preview===this.preview&&job.epoch===this.paintEpoch&&job.index===index;
    if(matching(this.pendingFrame))return this.pendingFrame.promise;
    if(matching(this.activeFrame)&&!this.pendingFrame)return this.activeFrame.promise;
    if(this.pendingFrame)this.pendingFrame.resolve(false);
    let resolve;const promise=new Promise(done=>{resolve=done;});
    this.pendingFrame={preview:this.preview,serial:this.serial,epoch:this.paintEpoch,index,resolve,promise};
    void this.drainFrames();return promise;
  }
  async drainFrames() {
    if(this.activeFrame)return;
    while(this.pendingFrame) {
      const job=this.pendingFrame;this.pendingFrame=null;this.activeFrame=job;
      const current=()=>!this.disposed&&job.serial===this.serial&&job.epoch===this.paintEpoch;
      try {
        const rgba=await job.preview.frame(job.index);
        if(current()){this.drawFrame(rgba,job.index);this.lastFrame=job.index;job.resolve(true);}
        else job.resolve(false);
      } catch(error) {
        if(current())this.fail(error);job.resolve(false);
      } finally {this.activeFrame=null;}
    }
  }
  audio(preview,serial,context) {
    if(this.buffer)return Promise.resolve(this.buffer);
    if(this.audioLoading)return this.audioLoading;
    const loading=Promise.resolve().then(()=>!this.disposed&&this.serial===serial&&this.preview===preview?preview.audioBuffer(context):null).then(buffer=>{
      if(buffer&&!this.disposed&&this.serial===serial&&this.preview===preview)this.buffer=buffer;
      return buffer;
    }).finally(()=>{if(this.audioLoading===loading)this.audioLoading=null;});
    this.audioLoading=loading;return loading;
  }
  play() {
    if(!this.preview||this.disposed)return Promise.resolve(false);
    if(this.pendingPlay)return this.pendingPlay.promise;
    if(this.playing)return Promise.resolve(true);
    if(this.position>=this.preview.header.duration){this.position=0;this.paintEpoch++;}
    const controller=new AbortController(),token=++this.action,preview=this.preview,serial=this.serial;
    let resume;
    try {
      if(!this.context)this.context=this.createAudioContext();
      // resume() is called inside the initiating click, before awaiting audio or frames.
      resume=this.context?.resume?this.context.resume():Promise.resolve();
    } catch(error){this.fail(error);return Promise.resolve(false);}
    this.starting=true;
    const operation={controller,promise:null};this.pendingPlay=operation;
    const current=()=>!this.disposed&&this.action===token&&this.serial===serial;
    operation.promise=(async()=>{
      try {
        const audio=this.context?this.audio(preview,serial,this.context):Promise.resolve(null);
        const [,buffer]=await awaitAbort(Promise.all([resume,audio,this.paint(this.frameAt(this.position))]),controller.signal);
        if(!current())return false;
        const ratio=preview.header.audioHz/preview.header.sampleRate;
        if(buffer&&this.position*ratio<buffer.duration) {
          const source=this.context.createBufferSource();this.source=source;source.buffer=buffer;
          source.playbackRate.value=ratio;source.connect(this.context.destination);source.start(0,this.position*ratio);
        }
        this.playPosition=this.position;this.wallAnchor=this.now();
        this.audioAnchor=this.context&&Number.isFinite(this.context.currentTime)&&this.context.state!=='closed'?this.context.currentTime:null;
        this.playing=true;this.starting=false;this.update();this.schedule();return true;
      } catch(error) {
        if(current()&&error.name!=='AbortError')this.fail(error);
        return false;
      } finally {
        if(this.pendingPlay===operation){this.pendingPlay=null;this.starting=false;this.update();}
      }
    })();
    this.update();return operation.promise;
  }
  schedule() {
    if(!this.playing||this.animation!==null)return;
    const token=this.action;
    this.animation=this.requestFrame(()=>{
      if(token!==this.action||!this.playing||this.disposed)return;this.animation=null;
      this.position=this.currentTime();
      if(this.position>=this.preview.header.duration) {
        this.halt();this.position=this.preview.header.duration;
        void this.paint(this.preview.header.frameCount-1);this.update();return;
      }
      void this.paint(this.frameAt(this.position));this.update();this.schedule();
    });
  }
  pause() {
    this.halt();if(this.preview)void this.paint(this.frameAt(this.position));this.update();
  }
  async stop() {
    this.halt();this.position=0;this.update();return this.paint(0);
  }
  seek(seconds) {
    if(!this.preview||this.disposed||!Number.isFinite(seconds))return Promise.resolve(false);
    const resume=this.playing||this.starting;
    this.halt();this.position=Math.min(this.preview.header.duration,Math.max(0,seconds));this.update();
    return resume&&this.position<this.preview.header.duration?this.play():this.paint(this.frameAt(this.position));
  }
  fail(error) {this.halt();this.update();if(!this.disposed)this.onError(error);}
  dispose() {
    this.clear();this.disposed=true;
    const context=this.context;this.context=null;
    if(context?.close)void Promise.resolve().then(()=>context.close()).catch(()=>{});
  }
}
