/** Independent MUP9 decoder for the converter preview. No emulator is involved. */
export const PAL_HZ = 985248 / (63 * 312);
export const CONTENT_FPS = PAL_HZ / 3;
export const PALETTE = [
  [0,0,0], [247,247,247], [141,47,52], [106,212,205],
  [152,53,164], [76,180,66], [44,41,177], [239,239,93],
  [152,78,32], [91,56,0], [209,103,109], [74,74,74],
  [123,123,123], [159,239,147], [109,106,239], [178,178,178],
];
const FRAME_BYTES = 180 * 256;
const BLOCK_BYTES = 262144;
const AUDIO_OFFSET = 225280;
const require = (condition, message) => { if (!condition) throw new Error(message); };
export function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}
export function readMupHeader(raw, fileBytes) {
  require(raw.byteLength === 256, 'The MUP header is incomplete.');
  const bytes = new Uint8Array(raw.buffer ?? raw, raw.byteOffset ?? 0, 256);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const u16 = offset => view.getUint16(offset, true);
  const u32 = offset => view.getUint32(offset, true);
  require(String.fromCharCode(...bytes.subarray(0, 4)) === 'MUP9', 'This is not a MUP9 file.');
  const crcBytes = new Uint8Array(252);
  crcBytes.set(bytes.subarray(0, 224)); crcBytes.set(bytes.subarray(228), 224);
  require(crc32(crcBytes) === u32(224), 'The MUP header checksum failed.');
  require(bytes[4] === 9 && bytes[5] === 9 && bytes[6] === 13 && bytes[7] <= 1,
    'The preview supports the native SD MUP9 profile.');
  require(u16(8) === 256 && u16(10) === 360 && u16(12) === 200 && u16(14) === 356 &&
    u16(16) === 200 && u16(18) === 0 && bytes[20] === 3 && bytes[21] === 0 && bytes[22] === 13,
    'Unsupported MUP picture geometry or timing.');
  const header = {
    recordCount: u32(24), blockCount: u32(28), divider: u16(32), ticksPerBlock: u16(34),
    audioOffset: u32(36), blockBytes: u32(40), audioTableOffset: u32(44),
    initialOffset: u32(48), initialBytes: u32(52), streamOffset: u32(56),
    streamBytes: u32(60), fileBytes: u32(64), frameCount: u32(68), lastTicks: u16(72), sampleRate: u32(76),
  };
  require(header.recordCount >= 6 && header.recordCount <= 65535 && header.recordCount % 3 === 0 &&
    header.frameCount === header.recordCount / 3 && header.blockCount === Math.ceil(header.recordCount / 20),
    'Invalid MUP frame count.');
  require(header.ticksPerBlock === 20 && header.audioOffset === AUDIO_OFFSET && header.blockBytes === BLOCK_BYTES &&
    header.audioTableOffset === 256 && header.initialBytes === FRAME_BYTES &&
    header.initialOffset === Math.ceil((256 + header.blockCount * 3) / 256) * 256 &&
    header.streamOffset === Math.ceil((header.initialOffset + FRAME_BYTES) / 65536) * 65536 &&
    header.streamBytes === header.blockCount * BLOCK_BYTES &&
    header.fileBytes === fileBytes && fileBytes === header.streamOffset + header.streamBytes &&
    header.lastTicks === header.recordCount - (header.blockCount - 1) * 20 && header.divider > 0 && header.sampleRate > 0,
    'Invalid MUP container bounds.');
  require(![23,74,75,...Array.from({length:16}, (_,i)=>80+i),...Array.from({length:28},(_,i)=>228+i)]
    .some(index => bytes[index]), 'Reserved MUP header bytes are nonzero.');
  return {...header, duration: header.recordCount / PAL_HZ, audioHz: 6250000 / (header.divider + 2)};
}
export function applyRecord(payload, state) {
  require(payload.length >= 2, 'Truncated MUP video record.');
  const flags = payload[0], count = payload[1];
  require(flags <= 3 && count <= 180, 'Invalid MUP record flags.');
  let at = 2;
  if (flags & 2) {
    require(payload.length >= 1026, 'Truncated MUP dictionary.');
    state.dictionary = payload.slice(2, 1026);
    require([254,255].every(t => [0,1,2,3].every(p => !state.dictionary[p * 256 + t])), 'Invalid reserved dictionary entry.');
    state.previousColumn = -1; at = 1026;
  }
  for (let index = 0; index < count; index++) {
    require(at + 2 <= payload.length, 'Truncated MUP column.');
    const column = payload[at++], tag = payload[at++];
    require(column > state.previousColumn && column < 180 && column !== 0 && column !== 179, 'Invalid MUP column order or border.');
    state.previousColumn = column;
    const base = column * 256;
    if (tag === 255) {
      require(at + 200 <= payload.length, 'Truncated MUP raw column.');
      state.front.set(payload.subarray(at, at + 200), base); at += 200;
    } else {
      require(state.dictionary && tag % 5 === 0 && tag <= 250 && at + 50 + tag <= payload.length,
        'Invalid MUP dictionary column.');
      const escapes = [];
      for (let word = 0; word < 50; word++) {
        const token = payload[at++], row = word * 4;
        if (token < 254) {
          for (let p = 0; p < 4; p++) state.front[base + row + p] = state.dictionary[p * 256 + token];
        } else if (token === 255) escapes.push(row);
      }
      require(escapes.length * 5 === tag, 'MUP escape count differs.');
      for (const row of escapes) {
        require(payload[at++] === row, 'Invalid MUP escape row.');
        state.front.set(payload.subarray(at, at + 4), base + row); at += 4;
      }
    }
  }
  require(at === payload.length, 'Trailing bytes in MUP video record.');
}
export function framebufferToRgba(front, rgba = new Uint8ClampedArray(360 * 200 * 4)) {
  require(front.length === FRAME_BYTES, 'Invalid preview framebuffer.');
  for (let y = 0; y < 200; y++) for (let column = 0; column < 180; column++) {
    const pair = front[column * 256 + y];
    for (let side = 0; side < 2; side++) {
      const rgb = PALETTE[side ? pair >>> 4 : pair & 15], offset = (y * 360 + column * 2 + side) * 4;
      rgba[offset] = rgb[0]; rgba[offset+1] = rgb[1]; rgba[offset+2] = rgb[2]; rgba[offset+3] = 255;
    }
  }
  return rgba;
}
export class MupPreview {
  constructor(blob, header, initial, audioLengths) {
    this.blob = blob; this.header = header; this.initial = initial; this.audioLengths = audioLengths;
    this.state = {front: initial.slice(), dictionary: null, previousColumn: -1};
    this.record = 0; this.blockIndex = -1; this.block = null; this.cursor = 0;
    this.checkpoints = new Map([[0, initial.slice()]]);
    this.rgba = new Uint8ClampedArray(360 * 200 * 4);
  }
  static async open(blob) {
    const header = readMupHeader(new Uint8Array(await blob.slice(0,256).arrayBuffer()), blob.size);
    const initial = new Uint8Array(await blob.slice(header.initialOffset, header.initialOffset+FRAME_BYTES).arrayBuffer());
    require(!initial.subarray(0,256).some(Boolean) && !initial.subarray(FRAME_BYTES-256).some(Boolean), 'MUP side borders are nonzero.');
    for (let column = 0; column < 180; column++) require(!initial.subarray(column*256+200,(column+1)*256).some(Boolean), 'MUP row padding is nonzero.');
    const table = new Uint8Array(await blob.slice(256,256+header.blockCount*3).arrayBuffer());
    const audioLengths = Array.from({length:header.blockCount}, (_,i) => table[i*3] | table[i*3+1]<<8 | table[i*3+2]<<16);
    require(audioLengths.every(n => n % 4 === 0 && n <= BLOCK_BYTES-AUDIO_OFFSET), 'Invalid MUP audio table.');
    return new MupPreview(blob,header,initial,audioLengths);
  }
  async loadBlock(index) {
    if (this.blockIndex === index) return;
    const offset = this.header.streamOffset + index * BLOCK_BYTES;
    this.block = new Uint8Array(await this.blob.slice(offset,offset+AUDIO_OFFSET).arrayBuffer());
    this.blockIndex = index; this.cursor = 0;
    // A checkpoint can begin inside a block; walk lengths without decoding preceding records.
    const skip = this.record - index * 20;
    for (let tick = 0; tick < skip; tick++) this.cursor += 2 + this.readLength(this.cursor);
  }
  readLength(at) {
    require(at+2 <= this.block.length, 'Truncated MUP block.');
    const length = this.block[at] | this.block[at+1]<<8;
    require(length >= 2 && at+length+4 <= AUDIO_OFFSET, 'Invalid MUP record length.');
    return length;
  }
  async frame(index) {
    index = Math.min(this.header.frameCount-1,Math.max(0,Math.floor(index)));
    const desired = index * 3;
    if (desired < this.record || desired - this.record > 180) {
      let nearest = 0;
      for (const key of this.checkpoints.keys()) if (key <= desired && key > nearest) nearest = key;
      this.state.front.set(this.checkpoints.get(nearest)); this.state.dictionary = null; this.state.previousColumn = -1;
      this.record = nearest; this.blockIndex = -1;
    }
    while (this.record < desired) {
      await this.loadBlock(Math.floor(this.record/20));
      const length = this.readLength(this.cursor);
      const payload = this.block.subarray(this.cursor+2,this.cursor+2+length);
      const expected = (this.record%3===0 ? 2:0) | (this.record%3===2 ? 1:0);
      require(payload[0] === expected, 'Invalid MUP frame publication sequence.');
      applyRecord(payload,this.state); this.cursor += 2+length; this.record++;
      if (this.record % 180 === 0) {
        this.checkpoints.set(this.record,this.state.front.slice());
        if (this.checkpoints.size > 17) this.checkpoints.delete([...this.checkpoints.keys()].find(k => k !== 0));
      }
    }
    return framebufferToRgba(this.state.front,this.rgba);
  }
  async audioBuffer(context) {
    const samples = this.audioLengths.reduce((a,b)=>a+b,0)/4;
    const result = context.createBuffer(2,samples,this.header.sampleRate);
    const left = result.getChannelData(0), right = result.getChannelData(1);
    let sample = 0;
    for (let block = 0; block < this.header.blockCount; block++) {
      const offset = this.header.streamOffset + block*BLOCK_BYTES+AUDIO_OFFSET;
      const raw = await this.blob.slice(offset,offset+this.audioLengths[block]).arrayBuffer();
      const data = new DataView(raw);
      for (let at = 0; at < raw.byteLength; at += 4, sample++) {
        left[sample] = data.getInt16(at,true)/32768; right[sample] = data.getInt16(at+2,true)/32768;
      }
    }
    return result;
  }
}
