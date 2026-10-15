/* CLI and WASM must quantize identical float inputs into identical PCM8 files. */
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const repo = path.resolve(__dirname, '../..');
const binary = process.argv[2] || path.join(repo, 'encoder/target/release/mupic-encoder');
const wasm = process.argv[3] || path.join(repo, 'site/assets/encoder.wasm');
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'mupic-pcm8-parity-'));
const run = args => execFileSync('ffmpeg', ['-v', 'error', '-nostdin', ...args], { maxBuffer: 8 * 1024 * 1024 });
try {
  const e = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(wasm))).exports;
  const bytes = (p, n) => Buffer.from(new Uint8Array(e.memory.buffer, p, n));
  const check = result => assert.equal(result, 0, bytes(e.mup_error_ptr(), e.mup_error_len()).toString());
  const put = data => { const p = e.mup_alloc(data.length); new Uint8Array(e.memory.buffer, p, data.length).set(data); return p; };
  const rates = [8000, 11025, 16000, 22050, 32000, 44100, 48000];
  for (const rate of rates) {
    const source = path.join(directory, `source-${rate}.mkv`);
    const target = path.join(directory, `source-${rate}.mup`);
    run(['-f', 'lavfi', '-i', 'color=c=blue:size=640x360:rate=30',
      '-f', 'lavfi', '-i', `aevalsrc=0.2*sin(2*PI*440*t)|0.12*sin(2*PI*880*t):s=${rate}:d=0.3`,
      '-t', '0.3', '-c:v', 'ffv1', '-c:a', 'pcm_f32le', '-shortest', '-y', source]);
    execFileSync(binary, [source, '-o', target, '--duration', '0.14', '--audio-format', 'pcm8',
      '--threads', '2', '--no-cache', '--title', 'PCM8 parity'], { stdio: ['ignore', 'pipe', 'inherit'] });
    const native = fs.readFileSync(target);
    assert.equal(native[7], 3);
    assert.equal(native[80], 2);
    assert.equal(native.readUInt32LE(76), rate);
    assert.equal(native.readUInt32LE(36), 221184);
    const count = native.readUInt32LE(68), divider = native.readUInt16LE(32);
    const fps = 985248 / (63 * 312 * 3), seconds = count / fps;
    const actualRate = 12500000 / (divider * 2 + 3);
    check(e.mup_pcm8_info(rate));
    const info = bytes(e.mup_result_ptr(), e.mup_result_len());
    assert.deepEqual([0, 4, 8, 12].map(p => info.readUInt32LE(p)), [divider, 12500000, divider * 2 + 3, 221184]);
    const pcm = run(['-ss', '0', '-i', source, '-map', '0:a:0', '-vn', '-af',
      `atrim=duration=${seconds},apad=whole_dur=${seconds},asetrate=${rate * (divider * 2 + 3)},aresample=12500000,asetrate=${Math.round(actualRate)}`,
      '-ac', '2', '-f', 'f32le', 'pipe:1']);
    const rgb = run(['-ss', '0', '-i', source, '-an', '-vf',
      `tpad=stop_mode=clone:stop_duration=${seconds},fps=${fps},scale=360:190:flags=lanczos,setsar=1,pad=360:200:0:5:black,format=rgb24`,
      '-frames:v', String(count), '-f', 'rawvideo', '-pix_fmt', 'rgb24', 'pipe:1']);
    const title = Buffer.from('PCM8 parity'), tp = put(title), ap = put(pcm);
    check(e.mup_begin_profile(count, ap, pcm.length, tp, title.length, rate, 8, 2));
    e.mup_free(ap, pcm.length); e.mup_free(tp, title.length);
    const blocks = [];
    const drain = () => { if (e.mup_output_len()) { blocks.push(bytes(e.mup_output_ptr(), e.mup_output_len())); e.mup_output_clear(); } };
    for (let frame = 0; frame < count; frame++) {
      const input = rgb.subarray(frame * 216000, (frame + 1) * 216000), rp = put(input);
      check(e.mup_quantize(rp, input.length)); e.mup_free(rp, input.length);
      const packed = bytes(e.mup_result_ptr(), e.mup_result_len()), pp = put(packed);
      check(e.mup_encode_packed(pp, packed.length)); e.mup_free(pp, packed.length); drain();
    }
    check(e.mup_finish()); drain();
    const encoded = Buffer.concat([bytes(e.mup_result_ptr(), e.mup_result_len()), ...blocks]);
    assert.deepEqual(encoded, native, `${rate} Hz CLI/WASM PCM8 quantization and container parity`);
    const n = BigInt(count * 3) * 12500000n * 19656n;
    const d = BigInt(divider * 2 + 3) * 985248n;
    let samples = n / d;
    if (n % d * 2n > d || (n % d * 2n === d && samples % 2n)) samples++;
    assert.equal(native.readUIntLE(256, 3), Number(samples * 2n));
    // Resampling must preserve the fractional-clock duration, including 8 kHz.
    assert.ok(Math.abs(pcm.length / 8 - Number(samples)) <= 2);
  }
  assert.equal(e.mup_pcm8_info(96000), 1);
  console.log(JSON.stringify({ passed: true, rates, cliWasmByteIdentical: true, fractionalClock: true }));
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
