import {MupPreview,CONTENT_FPS} from './mup-decoder.mjs';
import {videoPlan,asciiTitle,decodePcm,seekFrame,captureRgb,checkAbort,waitEvent} from './media.mjs?v=4b5fa7b3ec416362';
import {WorkerClient,QuantizerPool} from './worker-pool.mjs?v=d0f95ba62665f6ce';
import {ConversionProgress} from './conversion-progress.mjs';
const $=id=>document.getElementById(id);
const original=$('original-video'),decoder=$('decode-video'),canvas=$('mup-preview');
const context=canvas.getContext('2d',{alpha:false});
const capture=document.createElement('canvas');capture.width=360;capture.height=200;
const captureContext=capture.getContext('2d',{alpha:false,willReadFrequently:true});
function assetUrl(name) {
  const url=new URL(name,import.meta.url);
  url.search=new URL(import.meta.url).search;
  return url;
}
const wasmUrl=assetUrl('./assets/encoder.wasm').href;
let selected=null,plan=null,inputUrl=null,outputUrl=null,busy=false,loadSerial=0,abortController=null;
let pool=null,encoder=null,preview=null,previewSerial=0,paintRequested=-1,painting=false;
let audioContext=null,audioBuffer=null,audioSource=null,audioLoading=null;
function status(message,{error=false}={}) {
  $('conversion-status').hidden=!message;$('conversion-status').textContent=message;$('conversion-status').classList.toggle('error',error);
}
const sizeLabel=bytes=>`${(bytes/1048576).toFixed(1)} MiB`;
const timeLabel=seconds=>`${Math.floor(seconds/60)}:${String(Math.floor(seconds%60)).padStart(2,'0')}`;
function stopAudio() { if(audioSource) {try{audioSource.stop();}catch{}audioSource.disconnect();audioSource=null;} }
function clearOutput() {
  previewSerial++;preview=null;paintRequested=-1;audioBuffer=null;audioLoading=null;stopAudio();original.muted=false;
  if(outputUrl) URL.revokeObjectURL(outputUrl);outputUrl=null;
  $('download').hidden=true;$('preview-controls').hidden=true;
  context.fillStyle='#000';context.fillRect(0,0,360,200);
}
function setBusy(value) {
  busy=value;$('convert').disabled=value||!plan;$('choose-video').disabled=value;$('video-file').disabled=value;
  $('cancel').hidden=!value;$('drop-zone').classList.toggle('busy',value);
  for(const id of ['silent-source','audio-gain']) $(id).disabled=value;
}
async function selectFile(file) {
  if(!file || busy) return;
  const serial=++loadSerial;selected=null;plan=null;setBusy(false);original.pause();clearOutput();
  if(inputUrl) URL.revokeObjectURL(inputUrl);inputUrl=URL.createObjectURL(file);
  original.src=inputUrl;decoder.src=inputUrl;original.load();decoder.load();
  $('comparison').hidden=true;$('file-details').hidden=false;$('file-details').textContent=`${file.name} · ${sizeLabel(file.size)}`;
  status('Reading the video…');
  try {
    await waitEvent(decoder,'loadedmetadata');if(serial!==loadSerial)return;
    plan=videoPlan(decoder.duration,decoder.videoWidth,decoder.videoHeight);selected=file;
    $('comparison').hidden=false;
    $('file-details').textContent=`${file.name} · ${timeLabel(decoder.duration)} · ${sizeLabel(file.size)}`;
    status('');setBusy(false);
  } catch(error) {
    if(serial!==loadSerial)return;
    status(error.message,{error:true});setBusy(false);
  }
}
$('choose-video').addEventListener('click',()=>$('video-file').click());
$('video-file').addEventListener('change',event=>selectFile(event.target.files[0]));
const drop=$('drop-zone');
for(const type of ['dragenter','dragover']) drop.addEventListener(type,event=>{event.preventDefault();if(!busy)drop.classList.add('dragging');});
for(const type of ['dragleave','drop']) drop.addEventListener(type,event=>{event.preventDefault();drop.classList.remove('dragging');});
document.addEventListener('dragover',event=>event.preventDefault());
document.addEventListener('drop',event=>{
  event.preventDefault();if(drop.contains(event.target)||busy)return;
  if(event.dataTransfer.files.length===1)selectFile(event.dataTransfer.files[0]);
});
drop.addEventListener('drop',event=>{
  if(busy)return;
  if(event.dataTransfer.files.length!==1) {status('Please drop one video at a time.',{error:true});return;}
  selectFile(event.dataTransfer.files[0]);
});
$('cancel').addEventListener('click',()=>{
  abortController?.abort();pool?.close();encoder?.close();
  status('Cancelling…');
});
$('convert').addEventListener('click',async()=>{
  if(busy||!selected||!plan)return;
  clearOutput();original.pause();setBusy(true);abortController=new AbortController();
  const signal=abortController.signal,parts=[];
  const progress=new ConversionProgress(plan.frameCount);
  function updateProgress(snapshot=progress.snapshot()) {
    if(signal.aborted)return;
    const bar=$('conversion-progress');
    if(snapshot.value===null)bar.removeAttribute('value');else bar.value=snapshot.value;
    const message=snapshot.phase==='finishing'?'Finishing…':
      snapshot.quantizedFrames===snapshot.totalFrames?`Writing ${snapshot.encodedFrames} / ${snapshot.totalFrames} frames`:
      `Processing ${snapshot.quantizedFrames} / ${snapshot.totalFrames} frames`;
    status(message);
  }
  const workers=Math.max(1,Math.min(8,(navigator.hardwareConcurrency||2)-1));
  $('conversion-progress').hidden=false;$('conversion-progress').removeAttribute('value');
  status('Preparing audio…');
  try {
    const pcm=await decodePcm(selected,plan.duration,{silent:$('silent-source').checked,gainDb:Number($('audio-gain').value),signal});
    checkAbort(signal);
    pool=new QuantizerPool(workers,assetUrl('./quantizer-worker.mjs'));
    encoder=new WorkerClient(assetUrl('./encoder-worker.mjs'));
    status('Preparing…');
    await Promise.all([pool.init(wasmUrl),encoder.call('init',{url:wasmUrl})]);checkAbort(signal);
    await encoder.call('begin',{frameCount:plan.frameCount,pcm:pcm.buffer,title:asciiTitle(selected.name)},[pcm.buffer]);
    updateProgress(progress.start());
    const pending=[];
    const addChunks=chunks=>{for(const chunk of chunks)parts.push(new Blob([chunk]));};
    async function publishOne() {
      const {result,error}=await pending.shift();if(error)throw error;checkAbort(signal);
      for(const packed of result) {
        const encoded=await encoder.call('frame',{packed},[packed]);addChunks(encoded.chunks);
        updateProgress(progress.encodedFrame(progress.encodedFrames));
      }
    }
    const batchFrames=3;
    for(let first=0;first<plan.frameCount;first+=batchFrames) {
      const frames=[],last=Math.min(first+batchFrames,plan.frameCount);
      for(let index=first;index<last;index++) {
        checkAbort(signal);await seekFrame(decoder,index/CONTENT_FPS,signal);
        frames.push(captureRgb(decoder,captureContext,plan));
      }
      // Capture overlaps color planning; final codec writes retain frame order.
      pending.push((async()=>{
        try {
          const result=await pool.batch(frames);
          checkAbort(signal);
          for(let index=first;index<last;index++)updateProgress(progress.quantizedFrame(index));
          return {result};
        } catch(error){return {error};}
      })());
      if(pending.length>=2)await publishOne();
    }
    while(pending.length)await publishOne();
    updateProgress(progress.finishing());
    const finished=await encoder.call('finish');addChunks(finished.chunks);checkAbort(signal);
    const movie=new Blob([finished.prefix,...parts],{type:'application/octet-stream'});
    preview=await MupPreview.open(movie);checkAbort(signal);
    outputUrl=URL.createObjectURL(movie);$('download').href=outputUrl;
    $('download').download=`${asciiTitle(selected.name).replace(/[^A-Za-z0-9_-]/g,'_').slice(0,48)}.MUP`;
    $('download').hidden=false;$('preview-controls').hidden=false;
    original.currentTime=0;original.muted=true;$('preview-audio').value='mup';
    await paintFrame(0);
    $('conversion-progress').value=progress.complete().value;
    status(`Done · ${sizeLabel(movie.size)}`);
  } catch(error) {
    if(error.name==='AbortError'||signal.aborted)status('Cancelled.');
    else status(error.message,{error:true});
  } finally {
    pool?.close();encoder?.close();pool=null;encoder=null;abortController=null;setBusy(false);
  }
});
async function paintFrame(index) {
  if(!preview)return;paintRequested=index;
  if(painting)return;painting=true;
  const serial=previewSerial;
  try {
    while(paintRequested>=0&&preview&&serial===previewSerial) {
      const next=paintRequested;paintRequested=-1;
      const rgba=await preview.frame(next);
      if(serial===previewSerial)context.putImageData(new ImageData(rgba,360,200),0,0);
    }
  } catch(error) {status(`Preview: ${error.message}`,{error:true});}
  finally {painting=false;}
}
async function startAudio() {
  stopAudio();if(!preview||original.paused||$('preview-audio').value!=='mup')return;
  const serial=previewSerial,Context=globalThis.AudioContext||globalThis.webkitAudioContext;
  if(!Context)return;
  try {
    if(!audioContext)audioContext=new Context();await audioContext.resume();
    if(!audioBuffer) {
      if(!audioLoading)audioLoading=preview.audioBuffer(audioContext);
      audioBuffer=await audioLoading;audioLoading=null;
    }
    if(serial!==previewSerial||original.paused||$('preview-audio').value!=='mup')return;
    stopAudio();audioSource=audioContext.createBufferSource();audioSource.buffer=audioBuffer;
    const ratio=preview.header.audioHz/preview.header.sampleRate;
    audioSource.playbackRate.value=ratio*original.playbackRate;audioSource.connect(audioContext.destination);
    const offset=original.currentTime*ratio;
    if(offset<audioBuffer.duration)audioSource.start(0,offset);
  } catch(error) {status(`Audio preview: ${error.message}. You can listen to the original instead.`,{error:true});}
}
$('preview-audio').addEventListener('change',()=>{original.muted=$('preview-audio').value!=='original';startAudio();});
original.addEventListener('play',()=>{startAudio();requestAnimationFrame(playbackFrame);});
original.addEventListener('pause',stopAudio);original.addEventListener('ended',stopAudio);
original.addEventListener('seeking',stopAudio);
original.addEventListener('seeked',()=>{paintFrame(Math.floor(original.currentTime*CONTENT_FPS));startAudio();});
original.addEventListener('ratechange',startAudio);
function playbackFrame() {
  if(preview)paintFrame(Math.floor(original.currentTime*CONTENT_FPS));
  if(!original.paused&&!original.ended)requestAnimationFrame(playbackFrame);
}
window.addEventListener('pagehide',()=>{abortController?.abort();pool?.close();encoder?.close();stopAudio();audioContext?.close();});
status($('conversion-status').textContent);
