import {MupPreview} from './mup-decoder.mjs?v=bf988c9a98d52290';
import {MupPlayer} from './preview-player.mjs?v=ba45601c5bfd1eb3';
import {videoPlan,asciiTitle,decodePcmSource,seekFrame,captureRgb,checkAbort,waitEvent,awaitAbort} from './media.mjs?v=99d5e4f3a5b246a4';
import {WorkerClient,QuantizerPool} from './worker-pool.mjs?v=e358799a317c3c0d';
import {ConversionProgress} from './conversion-progress.mjs?v=ff2c0e223da601a2';
import { ensureIsolation } from './isolation.mjs?v=06da8c01bd989f1c';
await ensureIsolation();
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
let pool=null,encoder=null;
const workerCount=Math.max(1,Math.min(8,(navigator.hardwareConcurrency||2)-1));
let poolInitialization=null,preparation=null,preparationController=null,standardEncoding=false;
const hasLut=()=>Boolean(pool?.lutReady&&!pool.closed&&!pool.workers[0]?.closed);
const originalControls=$('original-controls'),mupControls=$('mup-controls');
let originalStarting=false,originalPlaySerial=0,originalFrame=null;
const conversionDialog=$('conversion-dialog'),tableDialog=$('lut-dialog');
function settings() {
  const value=name=>document.querySelector(`input[name="${name}"]:checked`).value;
  const ntsc=value('system-mode')==='ntsc',hd=value('video-profile')==='hd';
  return {profile:(ntsc?3:1)+(hd?1:0),ntsc,hd,audioFormat:value('audio-format')};
}
function updateOptionHints() {
  const {ntsc,hd,audioFormat}=settings();
  $('profile-hint').textContent=hd?'720p / 1080p':ntsc?'480p':'576p';
  $('audio-hint').textContent=`Source rate · Auto level${audioFormat==='pcm8'?'\nHQ noise shaping':''}`;
}
async function selectedPlan(signal) {
  const active=await awaitAbort(readyPool(),signal);checkAbort(signal);
  const info=await awaitAbort(active.videoPlan(settings().profile,decoder.videoWidth,decoder.videoHeight),signal);
  const next=videoPlan(decoder.duration,decoder.videoWidth,decoder.videoHeight,info);
  $('comparison').style.setProperty('--preview-aspect',`${360*info.profile.pixelAspectNumerator} / ${200*info.profile.pixelAspectDenominator}`);
  return next;
}
updateOptionHints();
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
function renderControls(controls,snapshot) {
  const {ready,playing,starting,time,duration}=snapshot;
  controls.dataset.state=!ready?'empty':starting?'starting':playing?'playing':'paused';
  controls.querySelector('[data-action="play"]').disabled=!ready||playing||starting;
  controls.querySelector('[data-action="pause"]').disabled=!ready||(!playing&&!starting);
  controls.querySelector('[data-action="stop"]').disabled=!ready;
  const seek=controls.querySelector('[data-action="seek"]');
  seek.disabled=!ready;seek.max=ready?duration:0;seek.value=ready?time:0;
  seek.setAttribute('aria-valuetext',`${timeLabel(time)} of ${timeLabel(duration)}`);
  controls.querySelector('[data-role="time"]').textContent=`${timeLabel(time)} / ${timeLabel(duration)}`;
}
const mupPlayer=new MupPlayer({
  drawFrame:rgba=>context.putImageData(new ImageData(rgba,360,200),0,0),
  onUpdate:snapshot=>renderControls(mupControls,snapshot),
  onError:error=>resultStatus(`MUpic playback: ${error.message}`,{error:true}),
});
function updateOriginalControls() {
  const ready=Number.isFinite(original.duration)&&original.duration>0&&original.readyState>=1;
  const playing=ready&&!original.paused&&!original.ended;
  renderControls(originalControls,{ready,playing,starting:originalStarting,
    time:ready?original.currentTime:0,duration:ready?original.duration:0});
  if(playing&&originalFrame===null)originalFrame=requestAnimationFrame(()=>{originalFrame=null;updateOriginalControls();});
  else if(!playing&&originalFrame!==null){cancelAnimationFrame(originalFrame);originalFrame=null;}
}
function pauseOriginal() {
  originalPlaySerial++;originalStarting=false;original.pause();updateOriginalControls();
}
async function playOriginal() {
  const serial=++originalPlaySerial;originalStarting=true;updateOriginalControls();
  if(original.ended||original.currentTime>=original.duration)original.currentTime=0;
  try {await original.play();}
  catch(error) {if(serial===originalPlaySerial)resultStatus(`Original playback: ${error.message}`,{error:true});}
  finally {if(serial===originalPlaySerial){originalStarting=false;updateOriginalControls();}}
}
function bindControls(controls,{play,pause,stop,seek}) {
  for(const [action,callback] of Object.entries({play,pause,stop})) {
    controls.querySelector(`[data-action="${action}"]`).addEventListener('click',async()=>{
      try {await callback();}catch(error){resultStatus(error.message,{error:true});}
    });
  }
  controls.querySelector('[data-action="seek"]').addEventListener('input',async event=>{
    const value=Number(event.target.value),end=Number(event.target.max);
    // Range values may round the exact endpoint by a few floating-point units.
    const seconds=value>=end-Number.EPSILON*Math.max(1,end)*8?end:value;
    try {await seek(seconds);}catch(error){resultStatus(error.message,{error:true});}
  });
}
bindControls(originalControls,{
  play:playOriginal,pause:pauseOriginal,
  stop:()=>{pauseOriginal();original.currentTime=0;updateOriginalControls();},
  seek:seconds=>{original.currentTime=seconds;updateOriginalControls();},
});
bindControls(mupControls,{
  play:()=>mupPlayer.play(),pause:()=>mupPlayer.pause(),
  stop:()=>mupPlayer.stop(),seek:seconds=>mupPlayer.seek(seconds),
});
for(const event of ['loadedmetadata','durationchange','emptied','play','pause','ended','timeupdate','seeking','seeked'])
  original.addEventListener(event,updateOriginalControls);
updateOriginalControls();renderControls(mupControls,mupPlayer.snapshot);
function clearOutput() {
  mupPlayer.clear();original.muted=false;
  if(outputUrl) URL.revokeObjectURL(outputUrl);outputUrl=null;
  $('download').hidden=true;
  context.fillStyle='#000';context.fillRect(0,0,360,200);
}
function setBusy(value) {
  busy=value;$('choose-video').disabled=value;$('video-file').disabled=value;
  $('encode').disabled=value||!selected||!plan;
  for(const fieldset of $('encoding-options').querySelectorAll('fieldset'))fieldset.disabled=value;
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
  pauseOriginal();clearOutput();resultStatus('');
  if(inputUrl)URL.revokeObjectURL(inputUrl);inputUrl=URL.createObjectURL(file);
  original.src=inputUrl;decoder.src=inputUrl;original.load();decoder.load();
  $('comparison').hidden=true;$('file-details').hidden=false;$('file-details').textContent=`${file.name} · ${sizeLabel(file.size)}`;
  abortController=new AbortController();const signal=abortController.signal;
  setBusy(true);showConversion('Reading video');status('Reading the video…');
  try {
    await waitEvent(decoder,'loadedmetadata',{signal});checkAbort(signal);
    if(serial!==loadSerial)return;
    plan=await selectedPlan(signal);checkAbort(signal);selected=file;
    $('comparison').hidden=false;
    $('file-details').textContent=`${file.name} · ${timeLabel(decoder.duration)} · ${sizeLabel(file.size)}`;
    resultStatus('Video ready. Choose your options, then press Encode.');
  } catch(error) {
    selected=null;plan=null;
    clearOutput();
    if(error.name==='AbortError'||signal.aborted)resultStatus('Cancelled. Choose a video to try again.');
    else resultStatus(error.message,{error:true});
  } finally {
    abortController?.abort();
    abortController=null;setBusy(false);
    if(conversionDialog.open)conversionDialog.close();
  }
}
async function encodeSelected() {
  if(busy||!selected||!plan)return;
  pauseOriginal();clearOutput();resultStatus('');
  abortController=new AbortController();const signal=abortController.signal;
  setBusy(true);
  try {
    plan=await selectedPlan(signal);checkAbort(signal);
    await convertSelected(signal);
  } catch(error) {
    clearOutput();
    if(error.name==='AbortError'||signal.aborted)resultStatus('Cancelled. Press Encode to try again.');
    else resultStatus(error.message,{error:true});
  } finally {
    abortController?.abort();
    encoder?.close();encoder=null;abortController=null;setBusy(false);
    if(conversionDialog.open)conversionDialog.close();
  }
}
$('encode').addEventListener('click',encodeSelected);
$('encoding-options').addEventListener('change',()=>{
  updateOptionHints();
  if(selected&&!busy) {
    clearOutput();
    resultStatus('Options updated. Press Encode when ready.');
  }
});
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
  await awaitAbort(pool.setProfile(plan.profile.id),signal);checkAbort(signal);
  const {audioFormat}=settings();
  const audio=await decodePcmSource(selected,plan.duration,{signal,audioInfo:sampleRate=>pool.audioInfo(audioFormat,sampleRate)});
  const {pcm,sampleRate}=audio;
  checkAbort(signal);
  encoder=new WorkerClient(assetUrl('./encoder-worker.mjs'));
  status('Preparing…');
  await encoder.call('init',{url:wasmUrl,module:pool.module});checkAbort(signal);
  await encoder.call('begin',{frameCount:plan.frameCount,pcm:pcm.buffer,title:asciiTitle(selected.name),audioFormat,sampleRate,profile:plan.profile.id},[pcm.buffer]);
  updateProgress(progress.start());
  const pending=[];
  const addChunks=chunks=>{for(const chunk of chunks)parts.push(new Blob([chunk]));};
  async function publishOne() {
    const {result,error}=await pending.shift();if(error)throw error;
    try {
      checkAbort(signal);
      for(const packed of result) {
        const transfer=packed instanceof ArrayBuffer?[packed]:[];
        const encoded=await encoder.call('frame',{packed},transfer);addChunks(encoded.chunks);
        updateProgress(progress.encodedFrame(progress.encodedFrames));
      }
    }finally{pool.releaseBatch(result);}
  }
  const batchFrames=3;
  for(let first=0;first<plan.frameCount;first+=batchFrames) {
    const frames=[],last=Math.min(first+batchFrames,plan.frameCount);
    for(let index=first;index<last;index++) {
      checkAbort(signal);await seekFrame(decoder,index/plan.fps,signal);
      frames.push(captureRgb(decoder,captureContext,plan));
    }
    // Capture overlaps color planning; final codec writes retain frame order.
    pending.push((async()=>{
      try {
        const result=await pool.batch(frames);
        if(signal.aborted){pool.releaseBatch(result);checkAbort(signal);}
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
  const preview=await MupPreview.open(movie);checkAbort(signal);
  outputUrl=URL.createObjectURL(movie);$('download').href=outputUrl;
  $('download').download=`${asciiTitle(selected.name).replace(/[^A-Za-z0-9_-]/g,'_').slice(0,48)}.MUP`;
  original.currentTime=0;
  const painted=await mupPlayer.load(preview);
  checkAbort(signal);
  if(!painted)throw new Error('The MUpic preview could not be displayed.');
  $('download').hidden=false;
  $('conversion-progress').value=progress.complete().value;
  resultStatus(`Ready · ${sizeLabel(movie.size)} · ${plan.fps.toFixed(1)} fps · ${preview.header.pcm8?'PCM8 HQ':'PCM16'} · ${(preview.header.sampleRate/1000).toLocaleString(undefined,{maximumFractionDigits:3})} kHz`);
}
window.addEventListener('pagehide',event=>{
  abortController?.abort();preparationController?.abort();pool?.close();encoder?.close();pauseOriginal();
  if(event.persisted)mupPlayer.pause();else mupPlayer.dispose();
});
void loadTable();
