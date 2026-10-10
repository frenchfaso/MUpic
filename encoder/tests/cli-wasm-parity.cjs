/* Reproducible CLI/WASM full-file parity on a real decoded MP4+AAC fixture. */
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const repo = path.resolve(__dirname, '../..');
const binary = process.argv[2] || path.join(repo, 'encoder/target/release/mupic-encoder');
const wasmPath = process.argv[3] || path.join(repo, 'site/assets/encoder.wasm');
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'mupic-parity-'));
try {
  const video = path.join(temporary, 'fixture.mp4');
  const output = path.join(temporary, 'fixture.mup');
  const run = (args) => execFileSync('ffmpeg', ['-v', 'error', '-nostdin', ...args], { maxBuffer: 8 * 1024 * 1024 });
  run(['-f','lavfi','-i','testsrc2=size=640x360:rate=30','-f','lavfi','-i','sine=frequency=440:sample_rate=48000','-t','0.48','-c:v','libx264','-pix_fmt','yuv420p','-c:a','aac','-shortest','-y',video]);
  execFileSync(binary, [video, '-o', output, '--threads', '4', '--title', 'CLI WASM parity'], { stdio: ['ignore', 'pipe', 'inherit'] });
  const native = fs.readFileSync(output);
  const count = native.readUInt32LE(68);
  const fps = 985248 / (63 * 312 * 3);
  const seconds = count / fps;
  const rgb = run(['-ss','0','-i',video,'-an','-vf',`tpad=stop_mode=clone:stop_duration=${seconds},fps=${fps},scale=356:200:flags=lanczos,setsar=1,pad=360:200:2:0:black,format=rgb24`,'-frames:v',String(count),'-f','rawvideo','-pix_fmt','rgb24','pipe:1']);
  const pcm = run(['-ss','0','-i',video,'-map','0:a:0','-vn','-af','volume=0dB,apad','-t',String(seconds),'-ac','2','-ar','22085','-f','s16le','pipe:1']);
  const e = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(wasmPath))).exports;
  const bytes = (p,n) => Buffer.from(new Uint8Array(e.memory.buffer,p,n));
  const check = s => assert.equal(s,0,bytes(e.mup_error_ptr(),e.mup_error_len()).toString());
  const put = b => { const p=e.mup_alloc(b.length);new Uint8Array(e.memory.buffer,p,b.length).set(b);return p; };
  const title=Buffer.from('CLI WASM parity'),tp=put(title),ap=put(pcm);
  check(e.mup_begin(count,ap,pcm.length,tp,title.length));e.mup_free(tp,title.length);e.mup_free(ap,pcm.length);
  const blocks=[];
  const drain=()=>{if(e.mup_output_len()){blocks.push(bytes(e.mup_output_ptr(),e.mup_output_len()));e.mup_output_clear();}};
  for(let i=0;i<count;i++){
    const frame=rgb.subarray(i*216000,(i+1)*216000),rp=put(frame);check(e.mup_quantize(rp,frame.length));e.mup_free(rp,frame.length);
    const packed=bytes(e.mup_result_ptr(),e.mup_result_len()),pp=put(packed);check(e.mup_encode_packed(pp,packed.length));e.mup_free(pp,packed.length);drain();
  }
  check(e.mup_finish());drain();
  const encoded=Buffer.concat([bytes(e.mup_result_ptr(),e.mup_result_len()),...blocks]);
  assert.deepEqual(encoded,native);
  console.log(JSON.stringify({passed:true,frames:count,bytes:encoded.length,sha256:createHash('sha256').update(encoded).digest('hex'),peakSlots:e.mup_peak_slots()}));
} finally { fs.rmSync(temporary,{recursive:true,force:true}); }
