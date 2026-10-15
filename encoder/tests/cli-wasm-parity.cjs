/* Reproducible CLI/WASM full-file parity on a real decoded MP4+AAC fixture. */
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { gunzipSync } = require('node:zlib');
const repo = path.resolve(__dirname, '../..');
const binary = process.argv[2] || path.join(repo, 'encoder/target/release/mupic-encoder');
const wasmPath = process.argv[3] || path.join(repo, 'site/assets/encoder.wasm');
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'mupic-parity-'));
try {
  const video = path.join(temporary, 'fixture.mp4');
  const output = path.join(temporary, 'fixture.mup');
  const run = (args) => execFileSync('ffmpeg', ['-v', 'error', '-nostdin', ...args], { maxBuffer: 8 * 1024 * 1024 });
  run(['-f','lavfi','-i','testsrc2=size=640x360:rate=30','-f','lavfi','-i','sine=frequency=440:sample_rate=48000','-t','0.48','-c:v','libx264','-pix_fmt','yuv420p','-c:a','aac','-shortest','-y',video]);
  execFileSync(binary, [video, '-o', output, '--threads', '4', '--no-cache', '--title', 'CLI WASM parity'], { stdio: ['ignore', 'pipe', 'inherit'] });
  const native = fs.readFileSync(output);
  const cachedOutput = path.join(temporary, 'cached.mup');
  execFileSync(binary, [video, '-o', cachedOutput, '--threads', '4', '--cache-dir',
    path.join(temporary, 'cache'), '--title', 'CLI WASM parity'], { stdio: ['ignore', 'pipe', 'inherit'] });
  assert.deepEqual(fs.readFileSync(cachedOutput), native, 'bundled LUT must preserve native output');
  let count = native.readUInt32LE(68);
  let profileId = 2;
  let fps = 985248 / (63 * 312 * 3);
  let seconds = count / fps;
  let rgb = run(['-ss','0','-i',video,'-an','-vf',`tpad=stop_mode=clone:stop_duration=${seconds},fps=${fps},scale=360:190:flags=lanczos,setsar=1,pad=360:200:0:5:black,format=rgb24`,'-frames:v',String(count),'-f','rawvideo','-pix_fmt','rgb24','pipe:1']);
  const divider=native.readUInt16LE(32), actualRate=6250000/(divider+2);
  let pcm = run(['-ss','0','-i',video,'-map','0:a:0','-vn','-af',`atrim=duration=${seconds},apad=whole_dur=${seconds},asetrate=${48000*(divider+2)},aresample=6250000,asetrate=${Math.round(actualRate)}`,'-ac','2','-f','f32le','pipe:1']);
  const e = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(wasmPath))).exports;
  const bytes = (p,n) => Buffer.from(new Uint8Array(e.memory.buffer,p,n));
  const check = s => assert.equal(s,0,bytes(e.mup_error_ptr(),e.mup_error_len()).toString());
  const put = b => { const p=e.mup_alloc(b.length);new Uint8Array(e.memory.buffer,p,b.length).set(b);return p; };
  function encode() {
    const title=Buffer.from('CLI WASM parity'),tp=put(title),ap=put(pcm);
    check(e.mup_begin_profile(count,ap,pcm.length,tp,title.length,48000,16,profileId));e.mup_free(tp,title.length);e.mup_free(ap,pcm.length);
    const blocks=[];
    const drain=()=>{if(e.mup_output_len()){blocks.push(bytes(e.mup_output_ptr(),e.mup_output_len()));e.mup_output_clear();}};
    for(let i=0;i<count;i++){
      const frame=rgb.subarray(i*216000,(i+1)*216000),rp=put(frame);check(e.mup_quantize(rp,frame.length));e.mup_free(rp,frame.length);
      const packed=bytes(e.mup_result_ptr(),e.mup_result_len()),pp=put(packed);check(e.mup_encode_packed(pp,packed.length));e.mup_free(pp,packed.length);drain();
    }
    check(e.mup_finish());drain();
    return Buffer.concat([bytes(e.mup_result_ptr(),e.mup_result_len()),...blocks]);
  }
  const encoded=encode();
  assert.deepEqual(encoded,native);
  const asset=gunzipSync(fs.readFileSync(path.join(repo,'site/assets/rgb24-lut.bin.gz')));
  const callBytes=(name,data)=>{const p=put(data);try{check(e[name](p,data.length));}finally{e.mup_free(p,data.length);}};
  callBytes('mup_lut_asset_begin',asset.subarray(0,128));
  for(let offset=128;offset<asset.length;offset+=65536)
    callBytes('mup_lut_asset_chunk',asset.subarray(offset,offset+65536));
  const manifest=JSON.parse(fs.readFileSync(path.join(repo,'site/assets/rgb24-lut.json')));
  callBytes('mup_lut_asset_end',Buffer.from(manifest.uncompressed_sha256,'hex'));
  assert.deepEqual(encode(),native,'bundled LUT must preserve full CLI/WASM file parity');
  // Exercise the distinct four-record planner, full-width NTSC geometry and
  // cumulative NTSC sampler timing with an independently decoded MP4 fixture.
  const ntscOutput=path.join(temporary,'ntsc.mup');
  execFileSync(binary,[video,'-o',ntscOutput,'--system','ntsc','--video-output','sd',
    '--threads','4','--cache-dir',path.join(temporary,'cache'),'--title','CLI WASM parity'],
    {stdio:['ignore','pipe','inherit']});
  const ntsc=fs.readFileSync(ntscOutput);
  assert.equal(ntsc[80],3); assert.equal(ntsc[20],4);
  profileId=3; count=ntsc.readUInt32LE(68); fps=1022730/(65*263*4); seconds=count/fps;
  rgb=run(['-ss','0','-i',video,'-an','-vf',`tpad=stop_mode=clone:stop_duration=${seconds},fps=${fps},scale=360:169:flags=lanczos,setsar=1,pad=360:200:0:15:black,format=rgb24`,'-frames:v',String(count),'-f','rawvideo','-pix_fmt','rgb24','pipe:1']);
  pcm=run(['-ss','0','-i',video,'-map','0:a:0','-vn','-af',`atrim=duration=${seconds},apad=whole_dur=${seconds},asetrate=${48000*(divider+2)},aresample=6250000,asetrate=${Math.round(actualRate)}`,'-ac','2','-f','f32le','pipe:1']);
  assert.deepEqual(encode(),ntsc,'NTSC four-record CLI/WASM whole-file parity');
  console.log(JSON.stringify({passed:true,profiles:[2,3],palBytes:encoded.length,ntscBytes:ntsc.length,sha256:createHash('sha256').update(encoded).digest('hex'),peakSlots:e.mup_peak_slots()}));
} finally { fs.rmSync(temporary,{recursive:true,force:true}); }
