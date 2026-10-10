import {CONTENT_FPS} from './mup-decoder.mjs';
export const SAMPLE_RATE = 22085;
export const MAX_FRAMES = 21845;
export function videoPlan(duration,width,height) {
  if(!Number.isFinite(duration) || duration<=0 || width<=0 || height<=0) throw new Error('This video has no readable duration or picture.');
  const frameCount=Math.ceil(duration*CONTENT_FPS);
  if(frameCount<2) throw new Error('Please choose a video at least 0.12 seconds long.');
  if(frameCount>MAX_FRAMES) throw new Error('The current C64 player supports videos up to about 21 minutes 47 seconds. Split this video with the native CLI.');
  const scale=Math.min(356/width,200/height),drawWidth=Math.max(1,Math.min(356,Math.round(width*scale))),drawHeight=Math.max(1,Math.min(200,Math.round(height*scale)));
  return {frameCount,duration:frameCount/CONTENT_FPS,width:drawWidth,height:drawHeight,
    left:2+Math.floor((356-drawWidth)/2),top:Math.floor((200-drawHeight)/2),
    estimatedBytes:65536+Math.ceil(frameCount*3/20)*262144};
}
export function asciiTitle(name) {
  return name.replace(/\.[^.]+$/,'').normalize('NFKD').replace(/[^\x20-\x7e]/g,'').trim().slice(0,63) || 'MY VIDEO';
}
export function checkAbort(signal) { if(signal?.aborted) throw new DOMException('Conversion was cancelled.','AbortError'); }
export function waitEvent(element,event,{signal,timeout=30000}={}) {
  return new Promise((resolve,reject)=>{
    let timer;
    const clean=()=>{clearTimeout(timer);element.removeEventListener(event,ok);element.removeEventListener('error',fail);signal?.removeEventListener('abort',abort);};
    const ok=()=>{clean();resolve();};
    const fail=()=>{clean();reject(new Error('The browser could not decode this video. Try MP4/H.264 or WebM, or use the native CLI.'));};
    const abort=()=>{clean();reject(new DOMException('Conversion was cancelled.','AbortError'));};
    if(signal?.aborted) return abort();
    element.addEventListener(event,ok,{once:true});element.addEventListener('error',fail,{once:true});signal?.addEventListener('abort',abort,{once:true});
    timer=setTimeout(()=>{clean();reject(new Error(`The video decoder timed out while waiting for ${event}.`));},timeout);
  });
}
export async function seekFrame(video,time,signal) {
  checkAbort(signal);
  const target=Math.max(0,Math.min(time,Math.max(0,video.duration-0.001)));
  if(Math.abs(video.currentTime-target)>0.00001 || video.seeking) {
    const done=waitEvent(video,'seeked',{signal});video.currentTime=target;await done;
  } else if(video.readyState<2) await waitEvent(video,'loadeddata',{signal});
  checkAbort(signal);
}
export function captureRgb(video,context,plan) {
  context.fillStyle='#000';context.fillRect(0,0,360,200);
  context.imageSmoothingEnabled=true;context.imageSmoothingQuality='high';
  context.drawImage(video,plan.left,plan.top,plan.width,plan.height);
  const rgba=context.getImageData(0,0,360,200).data,rgb=new Uint8Array(360*200*3);
  for(let source=0,target=0;source<rgba.length;source+=4,target+=3) {
    rgb[target]=rgba[source];rgb[target+1]=rgba[source+1];rgb[target+2]=rgba[source+2];
  }
  return rgb;
}
export function interleavePcm(left,right,gainDb=0) {
  if(left.length!==right.length) throw new Error('Audio channel lengths differ.');
  const gain=10**(gainDb/20),bytes=new Uint8Array(left.length*4),view=new DataView(bytes.buffer);
  for(let sample=0;sample<left.length;sample++) {
    const convert=value=>Math.max(-32768,Math.min(32767,Math.round(value*gain*32768)));
    view.setInt16(sample*4,convert(left[sample]),true);view.setInt16(sample*4+2,convert(right[sample]),true);
  }
  return bytes;
}
export async function decodePcm(file,duration,{silent=false,gainDb=0,signal}={}) {
  const samples=Math.round(duration*SAMPLE_RATE);
  if(silent) return new Uint8Array(samples*4);
  checkAbort(signal);
  const Context=globalThis.AudioContext || globalThis.webkitAudioContext;
  const Offline=globalThis.OfflineAudioContext || globalThis.webkitOfflineAudioContext;
  if(!Context || !Offline) throw new Error('This browser cannot decode audio. Please use the native CLI.');
  const context=new Context();let decoded;
  try {
    decoded=await context.decodeAudioData(await file.arrayBuffer());
  } catch {
    throw new Error('The browser could not decode this file\'s audio. Use the native CLI, or tick “Silent source” only if the video has no audio. Audio is never discarded automatically.');
  } finally { await context.close(); }
  checkAbort(signal);
  const offline=new Offline(2,samples,SAMPLE_RATE),source=offline.createBufferSource();
  source.buffer=decoded;source.connect(offline.destination);source.start();
  const rendered=await offline.startRendering();decoded=null;
  checkAbort(signal);
  return interleavePcm(rendered.getChannelData(0),rendered.getChannelData(1),gainDb);
}
