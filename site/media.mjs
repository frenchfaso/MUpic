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
export function awaitAbort(promise,signal) {
  if(!signal) return Promise.resolve(promise);
  return new Promise((resolve,reject)=>{
    const finish=(settle,value)=>{signal.removeEventListener('abort',abort);settle(value);};
    const abort=()=>finish(reject,new DOMException('Conversion was cancelled.','AbortError'));
    // Keep both handlers attached even after cancellation: browser operations can settle later.
    Promise.resolve(promise).then(value=>finish(resolve,value),error=>finish(reject,error));
    if(signal.aborted) abort();else signal.addEventListener('abort',abort,{once:true});
  });
}
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
class ContainerError extends Error {}
async function readContainerBytes(file,start,length,signal) {
  checkAbort(signal);
  const bytes=new Uint8Array(await awaitAbort(file.slice(start,start+length).arrayBuffer(),signal));
  checkAbort(signal);
  if(bytes.length!==length) throw new ContainerError('Incomplete container metadata.');
  return bytes;
}
function textAt(bytes,start,length) { return String.fromCharCode(...bytes.subarray(start,start+length)); }
async function mp4Box(file,start,end,signal) {
  if(end-start<8) throw new ContainerError('Incomplete MP4 box.');
  const bytes=await readContainerBytes(file,start,Math.min(16,end-start),signal),view=new DataView(bytes.buffer);
  let size=view.getUint32(0),header=8;
  if(size===1) {
    if(bytes.length<16) throw new ContainerError('Incomplete MP4 box size.');
    const large=view.getBigUint64(8);if(large>BigInt(Number.MAX_SAFE_INTEGER)) throw new ContainerError('Invalid MP4 box size.');
    size=Number(large);header=16;
  } else if(size===0) size=end-start;
  if(size<header || size>end-start) throw new ContainerError('Invalid MP4 box size.');
  return {type:textAt(bytes,4,4),start:start+header,end:start+size};
}
async function mp4AudioTrack(file,signal) {
  for(let offset=0;offset<file.size;) {
    const box=await mp4Box(file,offset,file.size,signal);offset=box.end;
    if(box.type!=='moov') continue;
    let video=false,tracks=0,unknown=false;
    for(let at=box.start;at<box.end;) {
      const track=await mp4Box(file,at,box.end,signal);at=track.end;
      if(track.type!=='trak') continue;
      tracks++;let handler;
      for(let child=track.start;child<track.end;) {
        const media=await mp4Box(file,child,track.end,signal);child=media.end;
        if(media.type!=='mdia') continue;
        for(let next=media.start;next<media.end;) {
          const item=await mp4Box(file,next,media.end,signal);next=item.end;
          if(item.type!=='hdlr') continue;
          if(item.end-item.start<12) throw new ContainerError('Incomplete MP4 track handler.');
          handler=textAt(await readContainerBytes(file,item.start,12,signal),8,4);
        }
      }
      if(handler==='soun') return true;
      if(handler==='vide') video=true;
      if(!['vide','text','subt','sbtl','clcp','hint','meta','mdta','tmcd','pict','auxv','subp'].includes(handler)) unknown=true;
    }
    return tracks && video && !unknown ? false : null;
  }
  return null;
}
async function ebmlElement(file,start,end,signal) {
  const bytes=await readContainerBytes(file,start,Math.min(12,end-start),signal);
  const width=(byte,max)=>{let count=1,mask=128;while(count<=max && !(byte&mask)){count++;mask>>=1;}if(count>max)throw new ContainerError('Invalid EBML integer.');return count;};
  if(!bytes.length) throw new ContainerError('Incomplete EBML element.');
  const idWidth=width(bytes[0],4);
  if(bytes.length<=idWidth) throw new ContainerError('Incomplete EBML element size.');
  const sizeWidth=width(bytes[idWidth],8),header=idWidth+sizeWidth;
  if(bytes.length<header) throw new ContainerError('Incomplete EBML element size.');
  let id=0,size=bytes[idWidth]&((1<<(8-sizeWidth))-1),unknown=size===((1<<(8-sizeWidth))-1);
  for(let i=0;i<idWidth;i++) id=id*256+bytes[i];
  for(let i=1;i<sizeWidth;i++){size=size*256+bytes[idWidth+i];unknown=unknown && bytes[idWidth+i]===255;}
  if(!unknown && (!Number.isSafeInteger(size) || size>end-start-header)) throw new ContainerError('Invalid EBML element size.');
  return {id,start:start+header,end:unknown?end:start+header+size,unknown};
}
async function webmAudioTrack(file,signal) {
  const header=await ebmlElement(file,0,file.size,signal);
  if(header.unknown) return null;
  for(let offset=header.end;offset<file.size;) {
    const segment=await ebmlElement(file,offset,file.size,signal);offset=segment.end;
    if(segment.id!==0x18538067) continue;
    for(let at=segment.start;at<segment.end;) {
      const tracks=await ebmlElement(file,at,segment.end,signal);at=tracks.end;
      if(tracks.id!==0x1654ae6b) {if(tracks.unknown)return null;continue;}
      if(tracks.unknown) return null;
      let video=false,entries=0,unknown=false;
      for(let child=tracks.start;child<tracks.end;) {
        const entry=await ebmlElement(file,child,tracks.end,signal);child=entry.end;
        if(entry.unknown) return null;
        if(entry.id!==0xae) continue;
        entries++;let type;
        for(let next=entry.start;next<entry.end;) {
          const item=await ebmlElement(file,next,entry.end,signal);next=item.end;
          if(item.unknown) return null;
          if(item.id!==0x83) continue;
          if(item.end-item.start<1 || item.end-item.start>8) throw new ContainerError('Invalid WebM track type.');
          type=0;for(const byte of await readContainerBytes(file,item.start,item.end-item.start,signal)) type=type*256+byte;
        }
        if(type===2) return true;
        if(type===1) video=true;
        if(![1,3,16,17,18,32].includes(type)) unknown=true;
      }
      return entries && video && !unknown ? false : null;
    }
  }
  return null;
}
// A failed audio decoder cannot distinguish missing audio from an unsupported codec.
// Only complete, recognizable track metadata permits automatic silent conversion.
export async function audioTrackPresence(file,{signal}={}) {
  checkAbort(signal);
  if(!file.slice || !Number.isSafeInteger(file.size) || file.size<8) return null;
  try {
    const header=await readContainerBytes(file,0,Math.min(16,file.size),signal);
    if(['ftyp','moov','mdat','wide','free','skip'].includes(textAt(header,4,4))) return await mp4AudioTrack(file,signal);
    if(new DataView(header.buffer).getUint32(0)===0x1a45dfa3) return await webmAudioTrack(file,signal);
    return null;
  } catch(error) { if(error instanceof ContainerError)return null;throw error; }
}
export function automaticGainDb(left,right) {
  if(left.length!==right.length) throw new Error('Audio channel lengths differ.');
  let peak=0;
  for(let sample=0;sample<left.length;sample++) {
    if(!Number.isFinite(left[sample]) || !Number.isFinite(right[sample])) throw new Error('The decoded audio contains invalid samples.');
    peak=Math.max(peak,Math.abs(left[sample]),Math.abs(right[sample]));
  }
  return peak===0 ? 0 : -1-20*Math.log10(peak);
}
export async function decodePcm(file,duration,{silent=false,gainDb='auto',signal}={}) {
  const samples=Math.round(duration*SAMPLE_RATE);
  checkAbort(signal);
  if(silent || await audioTrackPresence(file,{signal})===false) return new Uint8Array(samples*4);
  const Context=globalThis.AudioContext || globalThis.webkitAudioContext;
  const Offline=globalThis.OfflineAudioContext || globalThis.webkitOfflineAudioContext;
  if(!Context || !Offline) throw new Error('This browser cannot decode audio. Please use the native CLI.');
  const context=new Context();let decoded;
  try {
    const bytes=await awaitAbort(file.arrayBuffer(),signal);
    checkAbort(signal);
    decoded=await awaitAbort(context.decodeAudioData(bytes),signal);
  } catch {
    checkAbort(signal);
    throw new Error('The browser could not decode this file\'s audio. Try MP4/AAC or WebM/Opus, or use the native CLI.');
  } finally { await context.close(); }
  checkAbort(signal);
  const offline=new Offline(2,samples,SAMPLE_RATE),source=offline.createBufferSource();
  source.buffer=decoded;source.connect(offline.destination);source.start();
  const rendered=await awaitAbort(offline.startRendering(),signal);decoded=null;
  checkAbort(signal);
  const left=rendered.getChannelData(0),right=rendered.getChannelData(1);
  return interleavePcm(left,right,gainDb==='auto'?automaticGainDb(left,right):gainDb);
}
