/** Minimal adapter around the same Rust exports used by the native encoder. */
export class EncoderCore {
  static async load(url,module) {
    if(!module) {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`The Rust encoder could not be loaded (HTTP ${response.status}).`);
      try { module = await WebAssembly.compileStreaming(Promise.resolve(response.clone())); }
      catch { module = await WebAssembly.compile(await response.arrayBuffer()); }
    }
    const instance = await WebAssembly.instantiate(module, {});
    const core=new EncoderCore(instance.exports);core.module=module;return core;
  }
  constructor(exports) {
    this.exports = exports;
    for (const name of ['memory','mup_alloc','mup_free','mup_quantize','mup_begin','mup_begin_pcm8','mup_encode_packed',
      'mup_finish','mup_result_ptr','mup_result_len','mup_output_ptr','mup_output_len','mup_output_clear',
      'mup_error_ptr','mup_error_len','mup_prepare_keys','mup_plan_keys','mup_import_plans',
      'mup_lut_identity','mup_lut_info','mup_lut_patterns','mup_lut_asset_begin','mup_lut_asset_chunk',
      'mup_lut_asset_end','mup_lut_asset_abort','mup_lut_reset']) if (!(name in exports)) throw new Error(`Missing Rust encoder export: ${name}.`);
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
  quantize(rgb) {
    const pointer = this.allocate(rgb);
    try {
      this.check(this.exports.mup_quantize(pointer,rgb.byteLength));
      const e=this.exports;
      return this.bytes(e.mup_result_ptr(),e.mup_result_len()).slice();
    } finally { this.free(pointer,rgb.byteLength); }
  }
  sharedLut() {
    const e=this.exports;
    if(!e.mup_lut_plans_ptr||!e.mup_lut_plans_len||!e.mup_pack_plans)return null;
    const length=e.mup_lut_plans_len(),pointer=e.mup_lut_plans_ptr();
    if(length!==67108864||!pointer)return null;
    const buffer=new SharedArrayBuffer(length);
    new Uint8Array(buffer).set(this.bytes(pointer,length));
    return buffer;
  }
  packShared(rgb,lut,output) {
    if(rgb.length!==216000||lut.length!==16777216||output.length!==36000)
      throw new Error('Invalid shared frame or color table.');
    const e=this.exports,length=72000*4,pointer=e.mup_alloc(length);
    if(!pointer)throw new Error('The encoder ran out of memory.');
    try {
      const plans=new Uint32Array(e.memory.buffer,pointer,72000);
      for(let pixel=0,at=0;pixel<plans.length;pixel++,at+=3)
        plans[pixel]=lut[(rgb[at]<<16)|(rgb[at+1]<<8)|rgb[at+2]];
      this.check(e.mup_pack_plans(pointer,length));
      output.set(this.bytes(e.mup_result_ptr(),e.mup_result_len()));
    }finally{this.free(pointer,length);}
  }
  prepareKeys(rgb) {
    const pointer=this.allocate(rgb);
    try {
      this.check(this.exports.mup_prepare_keys(pointer,rgb.byteLength));
      const e=this.exports;
      return new Uint32Array(this.bytes(e.mup_result_ptr(),e.mup_result_len()).slice().buffer);
    } finally {this.free(pointer,rgb.byteLength);}
  }
  planKeys(keys) {
    const raw=new Uint8Array(keys.buffer,keys.byteOffset,keys.byteLength),pointer=this.allocate(raw);
    try {
      this.check(this.exports.mup_plan_keys(pointer,raw.byteLength));
      const e=this.exports;
      return new Uint32Array(this.bytes(e.mup_result_ptr(),e.mup_result_len()).slice().buffer);
    } finally {this.free(pointer,raw.byteLength);}
  }
  importPlans(keys,plans) {
    const rawKeys=new Uint8Array(keys.buffer,keys.byteOffset,keys.byteLength);
    const rawPlans=new Uint8Array(plans.buffer,plans.byteOffset,plans.byteLength);
    const keyPointer=this.allocate(rawKeys);let planPointer=0;
    try {
      planPointer=this.allocate(rawPlans);
      this.check(this.exports.mup_import_plans(keyPointer,rawKeys.byteLength,planPointer,rawPlans.byteLength));
    } finally {if(planPointer)this.free(planPointer,rawPlans.byteLength);this.free(keyPointer,rawKeys.byteLength);}
  }
  lutIdentity() {
    this.check(this.exports.mup_lut_identity());const e=this.exports;
    return new TextDecoder().decode(this.bytes(e.mup_result_ptr(),e.mup_result_len()));
  }
  lutInfo() {
    this.check(this.exports.mup_lut_info());const e=this.exports;
    const [blockCount,blockSize,loadedBlocks,totalColors]=new Uint32Array(this.bytes(e.mup_result_ptr(),e.mup_result_len()).slice().buffer);
    return {blockCount,blockSize,loadedBlocks,totalColors};
  }
  lutPatternFingerprint() {
    this.check(this.exports.mup_lut_patterns());const e=this.exports;
    return new TextDecoder().decode(this.bytes(e.mup_result_ptr(),e.mup_result_len()));
  }
  lutAssetOperation(name,bytes) {
    const pointer=this.allocate(bytes);
    try {
      this.check(this.exports[name](pointer,bytes.byteLength));
    } finally {this.free(pointer,bytes.byteLength);}
  }
  beginLutAsset(header) {this.lutAssetOperation('mup_lut_asset_begin',header);}
  appendLutAsset(chunk) {this.lutAssetOperation('mup_lut_asset_chunk',chunk);}
  finishLutAsset(expectedSha) {
    let digest=new Uint8Array();
    if(expectedSha!==undefined) {
      if(!(/^[a-f0-9]{64}$/).test(expectedSha))throw new Error('Invalid LUT asset checksum.');
      digest=Uint8Array.from(expectedSha.match(/../g),value=>parseInt(value,16));
    }
    this.lutAssetOperation('mup_lut_asset_end',digest);
  }
  abortLutAsset() {this.check(this.exports.mup_lut_asset_abort());}
  resetLut() { this.check(this.exports.mup_lut_reset()); }
  resultWords() {
    const e=this.exports,raw=this.bytes(e.mup_result_ptr(),e.mup_result_len()),view=new DataView(raw.buffer,raw.byteOffset,raw.byteLength);
    return Array.from({length:raw.byteLength/4},(_,i)=>view.getUint32(i*4,true));
  }
  setProfile(profile) { this.check(this.exports.mup_set_profile(profile)); }
  profileInfo(profile) {
    this.check(this.exports.mup_profile_info(profile));
    const [id,ticks,clockHz,lines,cyclesPerLine,pixelAspectNumerator,pixelAspectDenominator,rateBudget,maxFrames]=this.resultWords();
    return {id,ticks,clockHz,lines,cyclesPerLine,pixelAspectNumerator,pixelAspectDenominator,rateBudget,maxFrames,
      fps:clockHz/(lines*cyclesPerLine*ticks)};
  }
  fit(profile,width,height,sarNumerator=1,sarDenominator=1) {
    this.check(this.exports.mup_fit(profile,width,height,sarNumerator,sarDenominator));
    const [left,top,drawWidth,drawHeight]=this.resultWords();
    return {left,top,width:drawWidth,height:drawHeight};
  }
  audioInfo(audioFormat,sampleRate) {
    if(!['pcm16','pcm8'].includes(audioFormat))throw new Error('Unsupported audio format.');
    this.check(this.exports.mup_audio_info(audioFormat==='pcm8'?8:16,sampleRate));
    const [divider,rateNumerator,rateDenominator,audioOffset]=this.resultWords();
    return {divider,rateNumerator,rateDenominator,audioOffset};
  }
  begin(frameCount,pcm,title,{audioFormat='pcm16',sampleRate=22085,profile}={}) {
    if(!['pcm16','pcm8'].includes(audioFormat))throw new Error('Unsupported audio format.');
    const titleBytes=new TextEncoder().encode(title), pcmPointer=this.allocate(pcm), titlePointer=this.allocate(titleBytes);
    try {
      this.check(profile!==undefined
        ?this.exports.mup_begin_profile(frameCount,pcmPointer,pcm.byteLength,titlePointer,titleBytes.byteLength,sampleRate,audioFormat==='pcm8'?8:16,profile)
        :audioFormat==='pcm8'
        ?this.exports.mup_begin_pcm8(frameCount,pcmPointer,pcm.byteLength,titlePointer,titleBytes.byteLength,sampleRate)
        :this.exports.mup_begin(frameCount,pcmPointer,pcm.byteLength,titlePointer,titleBytes.byteLength));
    }
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
    const stats={peakSlots:e.mup_peak_slots?.(),adaptedFrames:e.mup_adapted_frames?.(),receivedFrames:e.mup_received_frames?.()};
    return {prefix,chunks,stats};
  }
}
