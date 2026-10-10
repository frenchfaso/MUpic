import "./wasm.js";
import { RemoteDisk } from "./remote-disk.js";
import { fetchBytes } from "./fetch-bytes.mjs";
const { instantiateUE2 } = globalThis.UE2Wasm;
const wasmUrl = new URL("./ue2_web.wasm", import.meta.url);
wasmUrl.search = new URL(import.meta.url).search;
// CPU, VIC and CIA share bus timing. A separate SID worker replays timestamped
// Rust events; no mutable chip state is shared concurrently across threads.
let module, sidPort;
let running = false, initialized = false, audioEnabled = false;
let remoteDisk = null, ioWaiting = false;
let timer = null, emuMs = 0, hostEpoch = 0, emuEpoch = 0;
let lastTick = 0, lastVideoEmu = -Infinity, lastVideoFrame = -1;
let tickQueued = false, sidPending = 0, framePending = 0;
let lastEmuProgressHost = 0, lastSidEmu = 0;
let audioControl = null, audioClockPort = null, audioRate = 48000;
let audioConsumed = 0, audioConsumedEpoch = 0, audioEpochEmu = 0;
let pendingAudioFrames = 0;
let fallbackQueuedFrames = 0;
let fallbackReceivedFrames = 0, fallbackReceivedBase = null, sentAudioFrames = 0;
let consumedCounterLast = 0, consumedCounterTotal = 0;
let publishedCounterLast = 0, publishedCounterTotal = 0;
const framePool = [], eventPool = [], samplerPool = [];
const MAX_SID_PENDING = 4;
// A short FIFO absorbs the audio prefill and browser/GPU scheduling jitter.
// Steady PAL playback drains it at 50 Hz; emulation never waits for video.
const MAX_VIDEO_PENDING = 8;
const AUDIO_LEAD_MS = 60;
const AUDIO_LIMIT_MS = 80;
const AUDIO_STEP_INSTRUCTIONS = 5000;
// Keep less of the 60 ms reserve trapped in an unpublished CPU-side tail.
const SID_BATCH_MS = 10;
const ticks = new MessageChannel();
ticks.port1.onmessage = () => { tickQueued = false; tick(); };
const send = (type, values = {}, transfers = []) => postMessage({ type, ...values }, transfers);
const errorText = () => {
  const ptr = module?._ue2_error();
  return ptr ? module.UTF8ToString(ptr) : "Emulator error.";
};

function scheduleTick(delay = 0) {
  if (ioWaiting) return;
  if (timer !== null) clearTimeout(timer);
  timer = null;
  if (tickQueued) return;
  if (delay > 0) timer = setTimeout(tick, delay);
  else if (!tickQueued) { tickQueued = true; ticks.port2.postMessage(0); }
}
function fault(error) {
  running = false;
  remoteDisk?.close();
  releaseJoysticks();
  if (timer !== null) clearTimeout(timer);
  timer = null;
  send("error", { message: String(error?.message || error) });
}
function releaseJoysticks() {
  if (!initialized) return;
  module._ue2_joystick(1, 0xff);
  module._ue2_joystick(2, 0xff);
}
async function start(options) {
  if (module || initialized) throw new Error("Restart with a new worker.");
  audioEnabled = Boolean(options.audioEnabled);
  audioRate = options.sampleRate || 48000;
  if (options.ring) {
    audioControl = new Int32Array(options.ring, 0, 8);
    consumedCounterLast = Atomics.load(audioControl, 6) >>> 0;
    publishedCounterLast = Atomics.load(audioControl, 4) >>> 0;
  }
  audioClockPort = options.audioClockPort;
  if (audioClockPort) {
    audioClockPort.onmessage = ({ data }) => {
      if (data.type !== "audio-clock") return;
      audioConsumed = data.consumedFrames;
      fallbackQueuedFrames = data.queuedFrames;
      if (fallbackReceivedBase === null) fallbackReceivedBase = data.receivedFrames;
      fallbackReceivedFrames = data.receivedFrames - fallbackReceivedBase;
      if (running) scheduleTick();
    };
    audioClockPort.start();
  }
  sidPort = options.sidPort;
  if (!sidPort) throw new Error("The SID worker is not connected.");
  sidPort.onmessage = ({ data }) => {
    if (data.type === "batch-return") {
      sidPending = Math.max(0, sidPending - 1);
      pendingAudioFrames = Math.max(0, pendingAudioFrames - (data.samplerSamples || 0) / 2);
      sentAudioFrames += data.audioFramesSent || 0;
      if (data.buffer?.byteLength) eventPool.push(data.buffer);
      if (data.sampler?.byteLength) samplerPool.push(data.sampler);
      if (running && sidPending < MAX_SID_PENDING) scheduleTick();
    } else if (data.type === "error") fault(data.message);
  };
  sidPort.start();
  send("state", { state: "loading", message: "Loading Rust WebAssembly…" });
  const response = await fetch("manifest.json", { cache: "no-store" });
  if (!response.ok) throw new Error(`manifest.json: HTTP ${response.status}`);
  const manifest = await response.json();
  if (!Array.isArray(manifest.core) || !Array.isArray(manifest.mupic?.assets)) throw new Error("Invalid file manifest.");
  if (!manifest.mupic.disk) throw new Error("MUpic disk manifest is missing.");
  remoteDisk = new RemoteDisk(manifest.mupic.disk);
  module = await instantiateUE2({
    wasmModule: options.wasmModule,
    wasmUrl,
    remoteRead: (...args) => remoteDisk.read(...args),
    printErr: (message) => send("log", { message: String(message), level: "error" }),
  });
  const shaderPtr = module._ue2_crt_shader(), paramsPtr = module._ue2_crt_params();
  send("video-config", {
    shader: new TextDecoder().decode(module.HEAPU8.subarray(shaderPtr, shaderPtr + module._ue2_crt_shader_len())),
    params: Array.from(module.HEAPF32.subarray(paramsPtr >>> 2, (paramsPtr >>> 2) + module._ue2_crt_params_len())),
  });
  const entries = [...manifest.core, ...manifest.mupic.assets];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (typeof entry.url !== "string" || (entry.aliases !== undefined && !Array.isArray(entry.aliases))) throw new Error("Invalid file manifest entry.");
    const paths = [entry.path, ...(entry.aliases || [])];
    if (paths.some(path => typeof path !== "string" || !path.startsWith("/") || path.split("/").includes(".."))) throw new Error("Invalid file path in the manifest.");
    const bytes = await fetchBytes(entry.url, (loaded, total) => send("download", {
      file: entry.path, index: i + 1, count: entries.length, loaded, total,
    }));
    if (bytes.length !== entry.bytes) throw new Error(`${entry.path}: size mismatch.`);
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
    if (Array.from(digest, b => b.toString(16).padStart(2, "0")).join("") !== entry.sha256)
      throw new Error(`${entry.path}: SHA-256 mismatch.`);
    for (const path of paths) module.registerFile(path, bytes);
  }
  send("state", { state: "booting", message: "Booting firmware…" });
  if (!module._ue2_remote_disk || module._ue2_remote_disk(remoteDisk.bytes) !== 0)
    throw new Error(errorText());
  if (module._ue2_init(options.sampleRate || 48000) !== 0) throw new Error(errorText());
  initialized = true;
  if (!module._ue2_render()) throw new Error(errorText());
  const width = module._ue2_width(), height = module._ue2_height();
  if (width <= 0 || height <= 0 || width > 4096 || height > 4096) throw new Error("Invalid screen dimensions.");
  send("ready");
  publishVideo();
  resume();
}
function resume() {
  if (!initialized || running) return;
  running = true;
  hostEpoch = lastTick = lastEmuProgressHost = performance.now();
  emuEpoch = emuMs;
  anchorAudioClock();
  send("state", { state: "running" }); scheduleTick();
}
function pause() {
  releaseJoysticks();
  if (!running) return;
  running = false;
  publishSid(true);
  if (timer !== null) clearTimeout(timer);
  timer = null; send("state", { state: "paused" });
}
function consumedAudioFrames() {
  if (!audioControl) return audioConsumed;
  const raw = Atomics.load(audioControl, 6) >>> 0;
  consumedCounterTotal += (raw - consumedCounterLast) >>> 0;
  consumedCounterLast = raw;
  return consumedCounterTotal;
}
function anchorAudioClock() {
  audioConsumedEpoch = consumedAudioFrames();
  // Include the not-yet-submitted tail in the reserve. No PCM is discarded or
  // stretched; a pause flushes that tail before the next anchor is created.
  audioEpochEmu = lastSidEmu;
}
function audioClockActive() {
  return audioEnabled && (audioControl !== null || audioClockPort !== null);
}
function sharedAudioSnapshot() {
  const capacity = (audioControl.buffer.byteLength - 32) / 8;
  for (let attempt = 0; attempt < 3; attempt++) {
    const before = Atomics.load(audioControl, 7);
    if (before & 1) continue;
    const read = Atomics.load(audioControl, 0);
    const write = Atomics.load(audioControl, 1);
    const published = Atomics.load(audioControl, 4) >>> 0;
    if (before !== Atomics.load(audioControl, 7)) continue;
    publishedCounterTotal += (published - publishedCounterLast) >>> 0;
    publishedCounterLast = published;
    return { queued: (write - read + capacity) % capacity, published: publishedCounterTotal };
  }
  // The producer is publishing a batch. Wait one task rather than guessing its
  // partially observed queue/counter, without ever blocking either thread.
  return null;
}
function audioReservedMs() {
  if (!audioControl && audioClockPort) {
    // The receiver reports how much PCM arrived as well as how much it played.
    // Include packets still in either port/SID worker, even after a SID ack.
    // Consumption since the latest 16 ms report only makes this conservative.
    const outstanding = Math.max(0, sentAudioFrames + pendingAudioFrames - fallbackReceivedFrames + fallbackQueuedFrames);
    return outstanding / audioRate * 1000 + Math.max(0, emuMs - lastSidEmu);
  }
  if (audioControl) {
    const snapshot = sharedAudioSnapshot();
    if (!snapshot) return AUDIO_LIMIT_MS;
    const frames = Math.max(0, snapshot.queued + sentAudioFrames + pendingAudioFrames - snapshot.published);
    return frames / audioRate * 1000 + Math.max(0, emuMs - lastSidEmu);
  }
  return 0;
}
function pacingTarget(now) {
  if (!audioClockActive()) return emuEpoch + now - hostEpoch;
  const current = consumedAudioFrames();
  const consumed = Math.max(0, current - audioConsumedEpoch);
  // The clock only advances for real PCM. Re-priming after an underrun does
  // not turn silent output time into extra queued audio.
  return audioEpochEmu + consumed / audioRate * 1000 + AUDIO_LEAD_MS;
}
function publishSid(force = false) {
  if (!force && emuMs - lastSidEmu < SID_BATCH_MS) return;
  const ptr = module._ue2_sid_events();
  const byteLength = module._ue2_sid_events_bytes();
  if (!byteLength) return;
  if (!ptr || byteLength % 16 !== 0) throw new Error("Invalid Rust SID batch.");
  let buffer = eventPool.pop();
  if (!buffer || buffer.byteLength < byteLength) buffer = new ArrayBuffer(Math.max(4096, byteLength));
  new Uint8Array(buffer, 0, byteLength).set(module.HEAPU8.subarray(ptr, ptr + byteLength));
  const samplerPtr = module._ue2_audio(), samplerSamples = module._ue2_audio_samples();
  let sampler;
  const transfers = [buffer];
  if (samplerPtr && samplerSamples) {
    sampler = samplerPool.pop();
    const bytes = samplerSamples * 2;
    if (!sampler || sampler.byteLength < bytes) sampler = new ArrayBuffer(Math.max(4096, bytes));
    new Uint8Array(sampler, 0, bytes).set(module.HEAPU8.subarray(samplerPtr, samplerPtr + bytes));
    transfers.push(sampler);
  }
  sidPending++; lastSidEmu = emuMs;
  pendingAudioFrames += samplerSamples / 2;
  sidPort.postMessage({ type: "events", buffer, byteLength, sampler, samplerSamples }, transfers);
}
function publishVideo() {
  const frameId = module._ue2_frame_id();
  if (!Number.isSafeInteger(frameId) || frameId < 0) throw new Error("Invalid VIC frame counter.");
  const changed = frameId !== lastVideoFrame;
  // Native firmware refreshes at 50 Hz before the VIC starts counting.
  // A live counter retains the 40 ms safeguard so PAL phases are not repeated.
  if (!changed && emuMs - lastVideoEmu < (frameId === 0 ? 20 : 40)) return;
  lastVideoFrame = frameId; lastVideoEmu = emuMs;
  if (framePending >= MAX_VIDEO_PENDING) return;
  const ptr = module._ue2_render();
  if (!ptr) throw new Error(errorText());
  const width = module._ue2_width(), height = module._ue2_height(), byteLength = width * height * 4;
  let buffer = framePool.pop();
  if (!buffer || buffer.byteLength !== byteLength) buffer = new ArrayBuffer(byteLength);
  new Uint8Array(buffer).set(module.HEAPU8.subarray(ptr, ptr + byteLength));
  const frameEmuMs = module._ue2_frame_time();
  if (!Number.isFinite(frameEmuMs) || frameEmuMs < 0) throw new Error("Invalid video timestamp.");
  framePending++; send("frame", { width, height, buffer, frameId, emuMs: frameEmuMs }, [buffer]);
}
async function tick() {
  timer = null;
  if (!running || ioWaiting) return;
  try {
    const begin = performance.now(), gap = begin - lastTick;
    lastTick = begin;
    if (gap > 100) {
      // Muted emulation can recenter after a suspended tab. Audio follows its
      // hardware consumer, so a stall never grants a second prefill reserve.
      if (!audioClockActive()) { hostEpoch = begin; emuEpoch = emuMs; }
      lastEmuProgressHost = begin;
    }
    const target = pacingTarget(begin);
    while (emuMs < target && sidPending < MAX_SID_PENDING && performance.now() - begin < 8) {
      const reserved = audioClockActive() ? audioReservedMs() : 0;
      if (audioClockActive() && reserved >= AUDIO_LIMIT_MS - 5) break;
      const next = module._ue2_step(audioClockActive() ? AUDIO_STEP_INSTRUCTIONS : 25000);
      if (!Number.isFinite(next) || next < 0) throw new Error(errorText());
      if (next < emuMs) throw new Error("Emulator time moved backwards.");
      if (next > emuMs) lastEmuProgressHost = performance.now();
      emuMs = next;
      // Capture the stable VIC `displayed` buffer before stepping far enough
      // for the next phase. Rust caps each step at four emulated milliseconds.
      publishVideo(); publishSid();
      if (remoteDisk && module._ue2_remote_waiting()) {
        ioWaiting = true;
        await remoteDisk.settle();
        module._ue2_remote_resume();
        if (module._ue2_remote_waiting()) throw new Error("Remote disk did not resume.");
        ioWaiting = false;
        // Network time grants no catch-up budget to the CPU. Audio keeps its
        // existing FIFO and is re-anchored without dropping or stretching PCM.
        hostEpoch = lastTick = lastEmuProgressHost = performance.now();
        emuEpoch = emuMs;
        anchorAudioClock();
        if (running) scheduleTick();
        return;
      }
    }
    const now = performance.now();
    if (now - lastEmuProgressHost > 5000 && (!audioClockActive() || audioReservedMs() < 20)) throw new Error("Emulator time has not advanced for 5 seconds.");
    const ahead = emuMs - pacingTarget(now);
    const reserveBlocked = audioClockActive() && audioReservedMs() >= AUDIO_LIMIT_MS - 5;
    const waitingForConsumer = audioClockActive() && ahead === 0;
    scheduleTick(sidPending >= MAX_SID_PENDING || reserveBlocked ? 4
      : waitingForConsumer ? Math.min(4, 128 / audioRate * 1000)
      : Math.min(8, Math.max(0, ahead)));
  } catch (error) { ioWaiting = false; fault(error); }
}
self.onmessage = ({ data }) => {
  if (data.type === "start") { start(data).catch(fault); return; }
  if (data.type === "frame-return") {
    framePending = Math.max(0, framePending - 1);
    if (data.buffer?.byteLength && framePool.length < MAX_VIDEO_PENDING) framePool.push(data.buffer);
    return;
  }
  if (!initialized) return;
  try {
    switch (data.type) {
      case "pause": pause(); break;
      case "resume": resume(); break;
      case "audio":
        if (audioEnabled !== Boolean(data.enabled)) {
          audioEnabled = Boolean(data.enabled);
          hostEpoch = performance.now(); emuEpoch = emuMs;
          anchorAudioClock();
        }
        break;
      case "key": if (Number.isInteger(data.index) && data.index >= 0 && data.index < 64) module._ue2_key(data.index, data.down ? 1 : 0); break;
      case "joystick":
        if (Number.isInteger(data.port) && data.port >= 1 && data.port <= 2 &&
            Number.isInteger(data.lines) && data.lines >= 0 && data.lines <= 0xff) {
          // A queued polling message must never reassert a paused joystick.
          module._ue2_joystick(data.port, running ? data.lines : 0xff);
        }
        break;
      case "menu": module._ue2_menu(data.down ? 1 : 0); break;
      case "restore": module._ue2_restore(data.down ? 1 : 0); break;

    }
  } catch (error) {
    fault(error);
  }
};
