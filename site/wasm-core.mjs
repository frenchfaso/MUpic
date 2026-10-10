/** Minimal adapter around the same Rust exports used by the native encoder. */
export class EncoderCore {
  static async load(url) {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`The Rust encoder could not be loaded (HTTP ${response.status}).`);
    let module;
    try { module = await WebAssembly.compileStreaming(Promise.resolve(response.clone())); }
    catch { module = await WebAssembly.compile(await response.arrayBuffer()); }
    const instance = await WebAssembly.instantiate(module, {});
    return new EncoderCore(instance.exports);
  }
  constructor(exports) {
    this.exports = exports;
    for (const name of ['memory','mup_alloc','mup_free','mup_quantize','mup_begin','mup_encode_packed',
      'mup_finish','mup_result_ptr','mup_result_len','mup_output_ptr','mup_output_len','mup_output_clear',
      'mup_error_ptr','mup_error_len']) if (!(name in exports)) throw new Error(`Missing Rust encoder export: ${name}.`);
  }
  bytes(pointer,length) { return new Uint8Array(this.exports.memory.buffer,pointer,length); }
  allocate(bytes) {
    const pointer = this.exports.mup_alloc(bytes.byteLength);
    if (!pointer && bytes.byteLength) throw new Error('The encoder ran out of memory. Try the native CLI for this video.');
    this.bytes(pointer,bytes.byteLength).set(bytes); return pointer;
  }
  free(pointer,length) { this.exports.mup_free(pointer,length); }
  check(status) {
    if (status !== 0) {
      const e = this.exports;
      const message = new TextDecoder().decode(this.bytes(e.mup_error_ptr(),e.mup_error_len()));
      throw new Error(message || 'The Rust encoder failed.');
    }
  }
  quantize(rgb,seeds) {
    const pointer = this.allocate(rgb); let seedPointer = 0;
    try {
      if (seeds && this.exports.mup_quantize_seeded) {
        const raw = new Uint8Array(seeds.buffer,seeds.byteOffset,seeds.byteLength);
        seedPointer = this.allocate(raw);
        this.check(this.exports.mup_quantize_seeded(pointer,rgb.byteLength,seedPointer,seeds.byteLength));
      } else this.check(this.exports.mup_quantize(pointer,rgb.byteLength));
      const e=this.exports;
      return this.bytes(e.mup_result_ptr(),e.mup_result_len()).slice();
    } finally { if(seedPointer) this.free(seedPointer,seeds.byteLength); this.free(pointer,rgb.byteLength); }
  }
  searchData() {
    const e=this.exports;
    if (!e.mup_gpu_search_ptr || !e.mup_gpu_search_len) return null;
    const pointer=e.mup_gpu_search_ptr(), length=e.mup_gpu_search_len();
    return this.bytes(pointer,length).slice().buffer;
  }
  begin(frameCount,pcm,title) {
    const titleBytes=new TextEncoder().encode(title), pcmPointer=this.allocate(pcm), titlePointer=this.allocate(titleBytes);
    try { this.check(this.exports.mup_begin(frameCount,pcmPointer,pcm.byteLength,titlePointer,titleBytes.byteLength)); }
    finally { this.free(pcmPointer,pcm.byteLength); this.free(titlePointer,titleBytes.byteLength); }
  }
  frame(packed) {
    const pointer=this.allocate(packed);
    try { this.check(this.exports.mup_encode_packed(pointer,packed.byteLength)); }
    finally { this.free(pointer,packed.byteLength); }
    return this.drain();
  }
  drain() {
    const e=this.exports,length=e.mup_output_len();
    const chunks=length ? [this.bytes(e.mup_output_ptr(),length).slice().buffer] : [];
    e.mup_output_clear(); return chunks;
  }
  finish() {
    this.check(this.exports.mup_finish()); const chunks=this.drain(),e=this.exports;
    const prefix=this.bytes(e.mup_result_ptr(),e.mup_result_len()).slice().buffer;
    let stats={peakSlots:e.mup_peak_slots?.(),adaptedFrames:e.mup_adapted_frames?.(),receivedFrames:e.mup_received_frames?.()};
    if(e.mup_stats_ptr && e.mup_stats_len) {
      try { stats=JSON.parse(new TextDecoder().decode(this.bytes(e.mup_stats_ptr(),e.mup_stats_len()))); } catch {}
    }
    return {prefix,chunks,stats};
  }
}
