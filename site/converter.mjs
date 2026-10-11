import {MupPreview,CONTENT_FPS} from './mup-decoder.mjs';
import {videoPlan,asciiTitle,decodePcm,seekFrame,captureRgb,checkAbort,waitEvent,awaitAbort} from './media.mjs?v=81de3bb75e9c558f';
import {WorkerClient,QuantizerPool} from './worker-pool.mjs?v=f2dd967349f8b8a2';
import {ConversionProgress} from './conversion-progress.mjs?v=ff2c0e223da601a2';
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
const workerCount=Math.max(1,Math.min(8,(navigator.hardwareConcurrency||2)-1));
let poolInitialization=null,preparation=null,preparationController=null,standardEncoding=false;
const hasLut=()=>Boolean(pool?.lutReady&&!pool.closed&&!pool.workers[0]?.closed);
let audioContext=null,audioBuffer=null,audioSource=null,audioLoading=null;
const conversionDialog=$('conversion-dialog'),tableDialog=$('lut-dialog');
function status(message,{error=false}={}) {
  $('conversion-status').textContent=message;
  $('conversion-status').classList.toggle('error',error);
}
function resultStatus(message,{error=false}={}) {
  const output=$('conversion-result');
  output.hidden=!message;output.textContent=message;output.classList.toggle('error',error);
}
function showConversion(title) {
  $('conversion-title').textContent=title;
  $('conversion-progress').removeAttribute('value');
  if(!conversionDialog.open)conversionDialog.showModal();
}
function cancelConversion() {
  abortController?.abort();
  preparationController?.abort();pool?.stopLut();
  if(encoder) {pool?.close();encoder.close();}
  status('Cancelling…');
}
conversionDialog.addEventListener('cancel',event=>{event.preventDefault();cancelConversion();});
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
  busy=value;$('choose-video').disabled=value;$('video-file').disabled=value;
  $('drop-zone').classList.toggle('busy',value);
}
async function readyPool() {
  if(!pool||pool.closed||pool.workers[0]?.closed) {
    pool?.close();
    pool=new QuantizerPool(workerCount,assetUrl('./quantizer-worker.mjs'));
    poolInitialization=pool.init(wasmUrl);
  }
  await poolInitialization;return pool;
}
function tableStatus(message,{error=false}={}) {
  $('lut-status').textContent=message;$('lut-status').classList.toggle('error',error);
}
async function loadTable({modal=true}={}) {
  if(preparation)return preparation;
  preparationController=new AbortController();
  standardEncoding=false;tableStatus('Loading color table…');
  $('lut-progress').removeAttribute('value');
  if(modal&&!tableDialog.open)tableDialog.showModal();
  preparation=(async()=>{
    try {
      const active=await readyPool();
      await active.loadLut(assetUrl('./assets/rgb24-lut.json'),{signal:preparationController.signal,onProgress:snapshot=>{
        $('lut-progress').value=snapshot.received/snapshot.total;
        const percentage=(snapshot.received/snapshot.total*100).toFixed(0);
        const message=snapshot.phase==='cached'?'Loading saved color table…':
          snapshot.phase==='validating'?'Checking color table…':
          `${snapshot.phase==='downloading'?`Downloading color table (${sizeLabel(snapshot.total)})`:'Unpacking color table'} · ${percentage}%`;
        tableStatus(message);
        if(busy)status(message);
      }});
    } catch(error) {
      if(error.name!=='AbortError') {
        standardEncoding=true;
        resultStatus('The color table is unavailable. Standard encoding will be used for this visit.');
      }
    } finally {
      preparation=null;preparationController=null;
      if(tableDialog.open)tableDialog.close();
    }
  })();
  await preparation;
}
function skipTable() {
  standardEncoding=true;preparationController?.abort();pool?.stopLut();
  if(tableDialog.open)tableDialog.close();
}
$('lut-standard').addEventListener('click',skipTable);
tableDialog.addEventListener('cancel',event=>{event.preventDefault();skipTable();});
async function selectFile(file) {
  if(!file||busy)return;
  const serial=++loadSerial;selected=null;plan=null;
  original.pause();clearOutput();resultStatus('');
  if(inputUrl)URL.revokeObjectURL(inputUrl);inputUrl=URL.createObjectURL(file);
  original.src=inputUrl;decoder.src=inputUrl;original.load();decoder.load();
  $('comparison').hidden=true;$('file-details').hidden=false;$('file-details').textContent=`${file.name} · ${sizeLabel(file.size)}`;
  abortController=new AbortController();const signal=abortController.signal;
  setBusy(true);showConversion('Reading video');status('Reading the video…');
  try {
    await waitEvent(decoder,'loadedmetadata',{signal});checkAbort(signal);
    if(serial!==loadSerial)return;
    plan=videoPlan(decoder.duration,decoder.videoWidth,decoder.videoHeight);selected=file;
    $('comparison').hidden=false;
    $('file-details').textContent=`${file.name} · ${timeLabel(decoder.duration)} · ${sizeLabel(file.size)}`;
    await convertSelected(signal);
  } catch(error) {
    clearOutput();
    if(error.name==='AbortError'||signal.aborted)resultStatus('Cancelled. Choose a video to try again.');
    else resultStatus(error.message,{error:true});
  } finally {
    abortController?.abort();
    encoder?.close();encoder=null;abortController=null;setBusy(false);
    if(conversionDialog.open)conversionDialog.close();
  }
}
$('choose-video').addEventListener('click',()=>{$('video-file').value='';$('video-file').click();});
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
  if(event.dataTransfer.files.length!==1) {resultStatus('Please drop one video at a time.',{error:true});return;}
  selectFile(event.dataTransfer.files[0]);
});
$('cancel').addEventListener('click',cancelConversion);
async function convertSelected(signal) {
  const parts=[];
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
  showConversion('Converting video');
  status('Preparing audio…');
  if(!standardEncoding&&!hasLut()) {
    status('Waiting for the color table…');
    if(preparation)await awaitAbort(preparation,signal);
    checkAbort(signal);
    if(!standardEncoding&&!hasLut())await awaitAbort(loadTable({modal:false}),signal);
    checkAbort(signal);
    if(!hasLut()&&!standardEncoding)throw new DOMException('Color table loading was cancelled.','AbortError');
  }
  await awaitAbort(readyPool(),signal);checkAbort(signal);status('Preparing audio…');
  const pcm=await decodePcm(selected,plan.duration,{signal});
  checkAbort(signal);
  encoder=new WorkerClient(assetUrl('./encoder-worker.mjs'));
  status('Preparing…');
  await encoder.call('init',{url:wasmUrl});checkAbort(signal);
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
  original.currentTime=0;original.muted=true;$('preview-audio').value='mup';
  await paintFrame(0);
  checkAbort(signal);
  $('download').hidden=false;$('preview-controls').hidden=false;
  $('conversion-progress').value=progress.complete().value;
  resultStatus(`Ready · ${sizeLabel(movie.size)}`);
}
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
  } catch(error) {resultStatus(`Preview: ${error.message}`,{error:true});}
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
  } catch(error) {resultStatus(`Audio preview: ${error.message}. You can listen to the original instead.`,{error:true});}
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
void loadTable();
