/* Actual raw-WASM ABI and independently checked container hashes/cadence. */
const fs = require('node:fs');
const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const wasmPath = process.argv[2] || 'site/assets/encoder.wasm';
const digest = b => createHash('sha256').update(b).digest();
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const value of bytes) {
    crc ^= value;
    for (let bit = 0; bit < 8; bit++) crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
  }
  return (crc ^ 0xffffffff) >>> 0;
}
(async () => {
  const module = new WebAssembly.Module(fs.readFileSync(wasmPath));
  assert.deepEqual(WebAssembly.Module.imports(module), []);
  const e = new WebAssembly.Instance(module).exports;
  const bytes = (p, n) => Buffer.from(new Uint8Array(e.memory.buffer, p, n));
  const check = status => { assert.equal(status, 0, bytes(e.mup_error_ptr(), e.mup_error_len()).toString()); };
  const alloc = data => { const p = e.mup_alloc(data.length); new Uint8Array(e.memory.buffer, p, data.length).set(data); return p; };
  assert.equal(e.mup_encode_packed(0, 0), 1);
  const title = Buffer.from('WASM ABI test'); const tp = alloc(title);
  const pcm = Buffer.alloc(50000); for (let i = 0; i < pcm.length; i++) pcm[i] = i & 255;
  const ap = alloc(pcm); check(e.mup_begin(8, ap, pcm.length, tp, title.length));
  e.mup_free(ap, pcm.length); e.mup_free(tp, title.length);
  const rgb = Buffer.alloc(216000);
  for (let y = 0; y < 200; y++) for (let x = 2; x < 358; x++) {
    const at = (y * 360 + x) * 3; rgb[at] = 76; rgb[at + 1] = 180; rgb[at + 2] = 66;
  }
  const rp = alloc(rgb); check(e.mup_quantize(rp, rgb.length));
  const packed = bytes(e.mup_result_ptr(), e.mup_result_len()); e.mup_free(rp, rgb.length);
  assert.equal(packed.length, 36000);
  for (let y = 0; y < 200; y++) { assert.equal(packed[y * 180], 0); assert.equal(packed[y * 180 + 179], 0); assert.equal(packed[y * 180 + 1], 0x55); }
  const chunks = [];
  const drain = () => { if (e.mup_output_len()) { chunks.push(bytes(e.mup_output_ptr(), e.mup_output_len())); e.mup_output_clear(); } };
  for (let frame = 0; frame < 8; frame++) {
    const p = alloc(packed); check(e.mup_encode_packed(p, packed.length)); e.mup_free(p, packed.length);
    assert.deepEqual(bytes(e.mup_preview_ptr(), e.mup_preview_len()), packed); drain();
  }
  check(e.mup_finish()); drain();
  const prefix = bytes(e.mup_result_ptr(), e.mup_result_len());
  const stream = Buffer.concat(chunks); const file = Buffer.concat([prefix, stream]);
  assert.equal(prefix.subarray(0, 4).toString(), 'MUP9'); assert.equal(prefix[5], 9); assert.equal(prefix[22], 13);
  assert.equal(prefix.readUInt32LE(24), 24); assert.equal(prefix.readUInt32LE(28), 2);
  assert.equal(prefix.readUInt16LE(14), 356); assert.equal(prefix.readUInt16LE(16), 200);
  assert.equal(prefix.readUInt32LE(64), file.length); assert.equal(file.length, 589824);
  assert.deepEqual(prefix.subarray(128, 160), digest(stream));
  const initialOffset = prefix.readUInt32LE(48);
  assert.deepEqual(prefix.subarray(96, 128), digest(prefix.subarray(initialOffset, initialOffset + 46080)));
  assert.equal(prefix.readUInt32LE(224), crc32(Buffer.concat([prefix.subarray(0, 224), prefix.subarray(228, 256)])));
  let record = 0, pcmCursor = 0;
  for (let block = 0; block < 2; block++) {
    let cursor = block * 262144;
    for (let tick = 0; tick < Math.min(20, 24 - block * 20); tick++, record++) {
      const n = stream.readUInt16LE(cursor); assert.ok(n >= 2); const flags = stream[cursor + 2];
      assert.equal(flags, record < 21 ? (record % 3 === 0 ? 2 : record % 3 === 2 ? 1 : 0) : 0);
      cursor += n + 2;
    }
    const n = prefix.readUIntLE(256 + block * 3, 3);
    assert.deepEqual(stream.subarray(block * 262144 + 225280, block * 262144 + 225280 + n), pcm.subarray(pcmCursor, pcmCursor + n));
    pcmCursor += n;
  }
  assert.equal(record, 24); assert.equal(e.mup_received_frames(), 8); assert.ok(e.mup_peak_slots() <= 425000);
  assert.equal(e.mup_finish(), 1);
  console.log(JSON.stringify({ passed: true, wasmPath, imports: 0, frames: 8, records: record, blocks: 2, bytes: file.length, peakSlots: e.mup_peak_slots() }));
})().catch(error => { console.error(error); process.exit(1); });
