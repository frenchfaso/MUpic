import "./wasm.js";
const { instantiateUE2 } = globalThis.UE2Wasm;
const wasmUrl = new URL("./ue2_web.wasm", import.meta.url);
wasmUrl.search = new URL(import.meta.url).search;
// Rust owns oscillator/envelope/filter/resampling and PCM mixing. JS transports
// opaque timestamped records and delivers stereo samples to AudioWorklet.
let module, cpuPort, audioPort, control, samples;
let capacity = 0, enabled = true;
const pcmPool = [];
const report = (message) => postMessage({ type: "error", message: String(message?.message || message) });
async function start(options) {
  if (module) throw new Error("The SID worker is already initialized.");
  cpuPort = options.cpuPort; audioPort = options.audioPort; enabled = Boolean(options.audioEnabled);
  if (options.ring) {
    control = new Int32Array(options.ring, 0, 8); samples = new Float32Array(options.ring, 32);
    capacity = samples.length / 2;
  }
  if (audioPort) {
    audioPort.onmessage = ({ data }) => { if (data.type === "pcm-return" && data.buffer?.byteLength && pcmPool.length < 4) pcmPool.push(data.buffer); };
    audioPort.start();
  }
  module = await instantiateUE2({
    wasmUrl,
    print: () => {}, printErr: (message) => postMessage({ type: "log", message: String(message) }),
  });
  if (module._ue2_sid_init(options.sampleRate || 48000) !== 0) throw new Error("Unable to initialize reSID Rust.");
  cpuPort.onmessage = ({ data }) => {
    if (data.type !== "events") return;
    try {
      if (!data.byteLength || data.byteLength % 16 || data.byteLength > data.buffer.byteLength) throw new Error("Invalid SID batch.");
      const ptr = module._ue2_sid_alloc(data.byteLength);
      if (!ptr) throw new Error("Unable to allocate SID event memory.");
      module.HEAPU8.set(new Uint8Array(data.buffer, 0, data.byteLength), ptr);
      if (module._ue2_sid_process(data.byteLength) !== 0) throw new Error(module.UTF8ToString(module._ue2_error()));
      if (data.samplerSamples) {
        const samplerPtr = module._ue2_sid_sampler_alloc(data.samplerSamples);
        if (!samplerPtr) throw new Error("Unable to allocate sampler memory.");
        module.HEAP16.set(new Int16Array(data.sampler, 0, data.samplerSamples), samplerPtr >>> 1);
        if (module._ue2_sid_mix(data.samplerSamples) !== 0) throw new Error(module.UTF8ToString(module._ue2_error()));
      }
      const pcmPtr = module._ue2_sid_audio(), count = module._ue2_sid_audio_samples();
      const audioFramesSent = enabled && pcmPtr && count
        ? publishPcm(module.HEAP16.subarray(pcmPtr >>> 1, (pcmPtr >>> 1) + count)) : 0;
      const transfers = [data.buffer];
      if (data.sampler) transfers.push(data.sampler);
      cpuPort.postMessage({ type: "batch-return", buffer: data.buffer, sampler: data.sampler,
        samplerSamples: data.samplerSamples, audioFramesSent }, transfers);
    } catch (error) {
      cpuPort.postMessage({ type: "error", message: String(error?.message || error) }); report(error);
    }
  };
  cpuPort.start(); postMessage({ type: "ready", transport: control ? "shared-ring" : "transfer-port", sampling: "interpolate", wasmModule: module.wasmModule });
}
function publishPcm(pcm) {
  if (control) {
    if (!Atomics.load(control, 5)) return 0;
    const read = Atomics.load(control, 0);
    let write = Atomics.load(control, 1);
    const frames = Math.floor(pcm.length / 2), free = capacity - 1 - ((write - read + capacity) % capacity);
    // Only the consumer changes its reader index; producer never races a drop.
    if (frames > free) return 0;
    for (let frame = 0; frame < frames; frame++) {
      samples[write * 2] = pcm[frame * 2] / 32768; samples[write * 2 + 1] = pcm[frame * 2 + 1] / 32768;
      write = (write + 1) % capacity;
    }
    // Publish the ring cursor and its produced-frame counter coherently. The
    // consumer only needs the cursor; CPU pacing reads both using this seqlock.
    Atomics.add(control, 7, 1);
    Atomics.store(control, 1, write);
    Atomics.add(control, 4, frames);
    Atomics.add(control, 7, 1);
    return frames;
  } else if (audioPort) {
    const byteLength = pcm.length * 2;
    let buffer = pcmPool.pop();
    if (!buffer || buffer.byteLength < byteLength) buffer = new ArrayBuffer(Math.max(4096, byteLength));
    new Int16Array(buffer, 0, pcm.length).set(pcm);
    audioPort.postMessage({ type: "samples", buffer, samples: pcm.length }, [buffer]);
    return Math.floor(pcm.length / 2);
  }
  return 0;
}
self.onmessage = ({ data }) => {
  if (data.type === "start") start(data).catch(report);
  else if (data.type === "audio") {
    enabled = Boolean(data.enabled);
    if (Number.isSafeInteger(data.request)) postMessage({ type: "audio-ready", request: data.request });
  }
};
