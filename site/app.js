import { CrtPresenter } from "./gpu.js";
import { GamepadInput } from "./gamepad.js";
const $ = (id) => document.getElementById(id);
const canvas = $("screen");
const mupicPage = document.body.dataset.preset === "mupic";
const controls = {
  c64: $("start-c64"), audio: $("audio"), fullscreen: $("fullscreen"),
};

let worker = null;
let sidWorker = null;
let sidShutdown = Promise.resolve();
let presenter = null;
const pendingFrames = [];
let paintQueued = false;
let videoWasHidden = document.hidden;
let audioRing = null;
let audioContext = null;
let audioNode = null;
let audioSetup = null;
let audioEnabled = false;
let audioAutoStartPending = true;
const AUDIO_SAMPLE_RATE = 48000;
let audioRequest = 0;
let audioHandshake = 0;
const audioReadyWaiters = new Map();
// Prepare worker endpoints before boot; output starts in a real user gesture.
let workerAudioTransport = null;
let generation = 0;
const logLines = [];
const gamepadInput = new GamepadInput();
const state = {
  state: "idle", preset: null, workerReady: false, emuMs: 0, wallMs: 0,
  speedPercent: null, frames: 0, width: 384, height: 272, lastFrameAt: null,
  audioState: "off", audioQueuedMs: 0, audioUnderflows: 0, audioDroppedFrames: 0,
  download: null, error: null, bootPercent: null,
  stepCpuMs: 0, videoCpuMs: 0, audioCpuMs: 0,
  sidCpuMs: 0, sidPending: 0, skippedFrames: 0, presentedFrames: 0,
  missedVicFrames: 0, videoBackpressureDrops: 0, videoPending: 0,
  presentedVicFrameId: null, presentedFrameEmuMs: null,
  lastCapturedFrameId: null, presentationSkippedFrames: 0, maxVideoQueue: 0,
  capturedFrameEmuMs: null, capturedFrames: 0,
  renderer: "WebGPU", gpuReady: false, crtEnabled: true, gpuSubmitCpuMs: 0,
  gpuCompletedFrames: 0, gpuInFlight: 0,
  threading: null, crossOriginIsolated: window.crossOriginIsolated, audioTransport: null,
  speedWindowMs: 0, speedHistory: [], benchmark: null, build: null,
  audioReservedMs: 0, maxAudioReservedMs: 0,
  wolfRelease: null, storage: null, mupic: null,
  gamepads: 0, sidSampling: "interpolate", sidParallel: false, sidWorkers: 0, activeSids: 0,
};
Object.defineProperty(window, "__ue2Status", {
  configurable: false,
  get: () => Object.freeze({
    ...state,
    download: state.download ? Object.freeze({ ...state.download }) : null,
    storage: state.storage ? Object.freeze({ ...state.storage }) : null,
    logs: Object.freeze(logLines.slice()),
    speedHistory: Object.freeze(state.speedHistory.slice()),
  }),
});
Object.defineProperty(window, "__ue2Debug", {
  value: Object.freeze({ measureRealtime: startBenchmark, audioTiming: () => ({
    state: audioContext?.state || "off", sampleRate: audioContext?.sampleRate || null,
    // These are browser estimates of the output path, distinct from our PCM FIFO.
    baseLatencyMs: typeof audioContext?.baseLatency === "number" ? audioContext.baseLatency * 1000 : null,
    outputLatencyMs: typeof audioContext?.outputLatency === "number" ? audioContext.outputLatency * 1000 : null,
    queuedMs: state.audioQueuedMs, reservedMs: state.audioReservedMs,
    sidPending: state.sidPending, underflows: state.audioUnderflows,
  }) }), configurable: false,
});

function log(message, level = "info") {
  const text = `${new Date().toLocaleTimeString("en-GB")} ${level === "error" ? "! " : ""}${message}`;
  logLines.push(text);
  if (logLines.length > 160) logLines.splice(0, logLines.length - 160);
  if (level === "error") console.error(text);
}

function updateUI(message) {
  const loading = ["loading", "booting"].includes(state.state);
  controls.c64.disabled = loading;
  controls.c64.textContent = mupicPage ? "PLAYER" : "BASIC";
  const wolfRelease = state.wolfRelease;
  controls.audio.textContent = "AUDIO";
  controls.audio.setAttribute("aria-pressed", String(audioEnabled));
  controls.audio.setAttribute("aria-label", `Audio ${audioEnabled ? "on" : "off"}`);
  controls.audio.title = audioEnabled ? "Turn audio off" : "Turn audio on";
  const status = {
    idle: "Ready.", loading: "Loading files…", booting: "Booting firmware…",
    running: state.preset === 2
      ? state.bootPercent !== null && state.bootPercent < 100
        ? `Starting MUpic Player… ${Math.floor(state.bootPercent)}%` : "MUpic Player running."
      : state.preset === 1 && state.bootPercent !== null && state.bootPercent < 100
        ? `Starting WOLF64U… ${Math.floor(state.bootPercent)}%` : state.preset === 1 ? "WOLF64U running." : "C64 BASIC running.",
    paused: "Emulation paused.", error: state.error || "Emulator error.",
  };
  $("runtime-status").textContent = message || status[state.state] || state.state;
  canvas.dataset.state = state.state;
  canvas.dataset.wolfRevision = wolfRelease?.revision || "snapshot";
  canvas.dataset.sidSampling = state.sidSampling;
  canvas.dataset.speed = state.speedPercent?.toFixed(3) || "";
  if (mupicPage) canvas.dataset.mupicProof = JSON.stringify({
    state: state.state, error: state.error, emuMs: state.emuMs, wallMs: state.wallMs,
    speedPercent: state.speedPercent, speedWindowMs: state.speedWindowMs,
    recentSpeedWindows: state.speedHistory.slice(-65),
    storage: state.storage, player: state.mupic || null,
    audioEnabled, audioState: state.audioState, audioUnderflows: state.audioUnderflows,
    audioQueuedMs: state.audioQueuedMs, audioReservedMs: state.audioReservedMs,
    capturedFrames: state.capturedFrames, presentedFrames: state.presentedFrames,
    gpuCompletedFrames: state.gpuCompletedFrames, missedVicFrames: state.missedVicFrames,
    videoBackpressureDrops: state.videoBackpressureDrops,
    presentationSkippedFrames: state.presentationSkippedFrames,
    audioDroppedFrames: state.audioDroppedFrames, gpuReady: state.gpuReady,
    crtEnabled: state.crtEnabled, pageHidden: document.hidden,
    sourceRaster: [state.width, state.height],
  });
  $("download-progress").hidden = !loading || !state.download;
}

function cancelAudioActivation() {
  for (const waiter of audioReadyWaiters.values()) { clearTimeout(waiter.timeout); waiter.resolve(); }
  audioReadyWaiters.clear();
}
function receiveAudioReady(data) {
  if (data.type !== "audio-ready") return false;
  const waiter = audioReadyWaiters.get(data.request);
  if (waiter) { audioReadyWaiters.delete(data.request); clearTimeout(waiter.timeout); waiter.resolve(); }
  return true;
}
function acknowledgedAudio(target, type) {
  const request = ++audioHandshake;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => { audioReadyWaiters.delete(request); reject(new Error("Audio output activation timed out.")); }, 5000);
    audioReadyWaiters.set(request, { resolve, timeout });
    target.postMessage({ type, enabled: true, request });
  });
}
function prepareAudioTransport(token) {
  if (!audioRing && window.crossOriginIsolated && typeof SharedArrayBuffer !== "undefined") {
    const ringFrames = Math.ceil(AUDIO_SAMPLE_RATE * 0.20) + 1;
    audioRing = new SharedArrayBuffer(32 + ringFrames * 2 * 4);
  }
  return { generation: token, node: null,
    audioLink: audioRing ? null : new MessageChannel(),
    clockLink: audioRing ? null : new MessageChannel() };
}
function releaseAudioTransport() {
  for (const link of [workerAudioTransport?.audioLink, workerAudioTransport?.clockLink]) {
    link?.port1.close(); link?.port2.close();
  }
  workerAudioTransport = null;
}
function attachAudioTransport() {
  const transport = workerAudioTransport;
  if (!audioNode || !transport || transport.generation !== generation || transport.node === audioNode) return;
  if (transport.clockLink) audioNode.port.postMessage({ type: "attach-clock", port: transport.clockLink.port2 }, [transport.clockLink.port2]);
  if (transport.audioLink) audioNode.port.postMessage({ type: "attach-port", port: transport.audioLink.port2 }, [transport.audioLink.port2]);
  // Messages on this port are ordered: both attachments precede enabled/ACK.
  transport.node = audioNode;
}
async function syncAudioTransport() {
  if (!audioEnabled) {
    worker?.postMessage({ type: "audio", enabled: false });
    sidWorker?.postMessage({ type: "audio", enabled: false });
    audioNode?.port.postMessage({ type: "enabled", enabled: false });
    return;
  }
  if (!state.workerReady || state.state === "paused" || state.state === "error" || !audioNode || audioContext?.state !== "running" || !sidWorker || !worker) return;
  attachAudioTransport();
  const token = generation, choice = audioRequest, cpu = worker, sid = sidWorker;
  const current = () => token === generation && choice === audioRequest && audioEnabled && worker === cpu && sidWorker === sid && state.state !== "paused" && state.state !== "error";
  try {
    await acknowledgedAudio(audioNode.port, "enabled");
    if (!current()) return;
    await acknowledgedAudio(sid, "audio");
    if (!current()) return;
    // Both downstream consumers now accept PCM. Starting the clock before
    // either acknowledgement can discard one batch and deadlock cold priming.
    cpu.postMessage({ type: "audio", enabled: true });
  } catch (error) { if (current()) showError(error.message || error); }
}

function clearAudio() {
  audioNode?.port.postMessage({ type: "clear" });
  state.audioQueuedMs = 0;
}

async function ensureAudio() {
  if (!audioContext) {
    const AudioContextClass = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextClass) throw new Error("Web Audio is not available in this browser.");
    audioContext = new AudioContextClass({ sampleRate: AUDIO_SAMPLE_RATE, latencyHint: "interactive" });
    audioContext.onstatechange = () => {
      state.audioState = audioContext.state;
      if (audioContext.state !== "running") invalidateBenchmark(`audio-${audioContext.state}`);
    };
  }
  if (audioContext.sampleRate !== AUDIO_SAMPLE_RATE) throw new Error("This browser cannot create the required 48 kHz audio output.");
  // Create and resume within the user gesture, before the first await.
  const resumed = audioContext.resume();
  if (!audioSetup) {
    const setup = (async () => {
      if (!audioContext.audioWorklet) throw new Error("AudioWorklet is not available; use localhost or HTTPS.");
      await audioContext.audioWorklet.addModule("audio-worklet.js");
      const node = new AudioWorkletNode(audioContext, "ue2-audio", {
        outputChannelCount: [2], numberOfInputs: 0, numberOfOutputs: 1, processorOptions: { ring: audioRing },
      });
      node.port.onmessage = ({ data }) => {
        if (audioNode !== node) return;
        if (receiveAudioReady(data)) return;
        state.audioQueuedMs = data.queuedMs;
        state.audioUnderflows = data.underflows;
        state.audioDroppedFrames = data.droppedFrames;
      };
      try { node.connect(audioContext.destination); }
      catch (error) { node.disconnect(); node.port.close(); throw error; }
      audioNode = node;
    })();
    audioSetup = setup;
    setup.catch(() => { if (audioSetup === setup) audioSetup = null; });
  }
  await Promise.all([resumed, audioSetup]);
  attachAudioTransport();
  state.audioState = audioContext.state;
}

async function setAudio(enabled) {
  // A manual choice also consumes auto-start, so playing cannot undo a mute.
  audioAutoStartPending = false;
  const request = ++audioRequest;
  cancelAudioActivation();
  audioEnabled = Boolean(enabled);
  updateUI();
  if (audioEnabled) {
    try {
      await ensureAudio();
      if (request !== audioRequest) return;
    } catch (error) {
      if (request !== audioRequest) return;
      audioEnabled = false;
      state.audioState = "unavailable";
      log(error.message || error, "error");
    }
  } else {
    invalidateBenchmark("audio-muted");
  }
  await syncAudioTransport();
  updateUI();
}

function activateAudioFromInput(event) {
  // Escape and browser shortcuts need not grant activation. Keep auto-start
  // available for the next usable gesture instead of creating suspended audio.
  if (!audioAutoStartPending || !event.isTrusted || event.code === "Escape"
      || navigator.userActivation?.isActive === false) return;
  setAudio(true);
}

function stopSidWorker(instance) {
  // A reset clears sidWorker immediately. Later resets and error recovery must
  // still await its pending close before another producer can use audioRing.
  if (instance) sidShutdown = sidShutdown.then(() => {
    if (!instance.sidPool) { instance.terminate(); return; }
    return new Promise((resolve) => {
      let timer;
      const finish = () => {
        clearTimeout(timer); instance.removeEventListener("message", closed);
        instance.terminate(); resolve();
      };
      const closed = ({ data }) => { if (data.type === "closed") finish(); };
      instance.addEventListener("message", closed);
      timer = setTimeout(finish, 1000);
      try { instance.postMessage({ type: "close" }); } catch { finish(); }
    });
  });
  return sidShutdown;
}

async function start(preset) {
  const token = ++generation;
  cancelAudioActivation();
  releaseAllKeys();
  releaseGamepad();
  worker?.terminate();
  const previousSidWorker = sidWorker;
  worker = null;
  sidWorker = null;
  await stopSidWorker(previousSidWorker);
  if (token !== generation) return;
  releaseAudioTransport();
  workerAudioTransport = prepareAudioTransport(token);
  audioNode?.port.postMessage({ type: "enabled", enabled: false });
  pendingFrames.length = 0;
  clearAudio();
  Object.assign(state, {
    state: "loading", preset, workerReady: false, emuMs: 0, wallMs: 0,
    speedPercent: null, frames: 0, lastFrameAt: null, error: null, download: null,
    bootPercent: preset === 1 || preset === 2 ? 0 : null,
    stepCpuMs: 0, videoCpuMs: 0, audioCpuMs: 0, sidCpuMs: 0, sidPending: 0,
    skippedFrames: 0, presentedFrames: 0, missedVicFrames: 0,
    videoBackpressureDrops: 0, videoPending: 0,
    presentedVicFrameId: null, presentedFrameEmuMs: null,
    lastCapturedFrameId: null, presentationSkippedFrames: 0, maxVideoQueue: 0,
    capturedFrameEmuMs: null, capturedFrames: 0,
    speedHistory: [], benchmark: null, build: null,
    wolfRelease: null, storage: null, mupic: null, sidParallel: false, sidWorkers: 0, activeSids: 0,
  });
  $("screen-hint").hidden = false;
  $("screen-hint").querySelector("p").textContent = "Loading emulator…";
  updateUI();
  log(`Starting ${preset === 2 ? "MUpic Player" : preset === 1 ? "WOLF64U" : "C64 BASIC"}.`);
  try {
    if (!presenter) presenter = await CrtPresenter.create(canvas, showError);
    if (token !== generation) return;
    const link = new MessageChannel();
    const { clockLink, audioLink } = workerAudioTransport;
    attachAudioTransport();
    let compiledWasmModule = null;
    const sidParallelAvailable = window.crossOriginIsolated && typeof SharedArrayBuffer === "function";
    const currentSidWorker = new Worker(sidParallelAvailable ? "sid-pool-worker.js" : "sid-worker.js", { type: "module" });
    currentSidWorker.sidPool = sidParallelAvailable;
    sidWorker = currentSidWorker;
    const sidReady = new Promise((resolve, reject) => {
      let ready = false;
      const timeout = setTimeout(() => reject(new Error("The SID worker has not responded for 30 seconds.")), 30000);
      currentSidWorker.onmessage = ({ data }) => {
        if (sidWorker !== currentSidWorker) return;
        if (receiveAudioReady(data)) return;
        if (data.type === "ready") {
          ready = true; compiledWasmModule = data.wasmModule;
          clearTimeout(timeout); state.audioTransport = data.transport; state.sidSampling = data.sampling || "interpolate";
          state.sidParallel = data.sidParallel === true; resolve();
          log(`Independent reSID Rust · ${data.transport}.`);
        } else if (data.type === "pool-state") {
          state.sidWorkers = data.sidWorkers; state.activeSids = data.activeSids;
        } else if (data.type === "error") {
          clearTimeout(timeout);
          if (ready) showError(data.message); else reject(new Error(data.message));
        }
        else if (data.type === "log") log(data.message);
      };
      currentSidWorker.onerror = (event) => {
        if (sidWorker !== currentSidWorker) return;
        const message = event.message || "SID worker error.";
        clearTimeout(timeout);
        if (ready) showError(message); else reject(new Error(message));
      };
    });
    const sidTransfers = [link.port2];
    if (audioLink) sidTransfers.push(audioLink.port1);
    currentSidWorker.postMessage({ type: "start", cpuPort: link.port2, audioPort: audioLink?.port1,
      ring: audioRing, sampleRate: AUDIO_SAMPLE_RATE, audioEnabled: false }, sidTransfers);
    await sidReady;
    if (token !== generation) return;
    const currentWorker = new Worker("worker.js", { type: "module" });
    worker = currentWorker;
    currentWorker.onmessage = (event) => {
      if (worker === currentWorker) onWorkerMessage(event.data);
    };
    currentWorker.onerror = (event) => {
      if (worker === currentWorker) showError(event.message || "WebAssembly worker error.");
    };
    const cpuTransfers = [link.port1];
    if (clockLink) cpuTransfers.push(clockLink.port1);
    currentWorker.postMessage({ type: "start", preset, sampleRate: AUDIO_SAMPLE_RATE,
      audioEnabled: false, sidPort: link.port1, ring: audioRing, audioClockPort: clockLink?.port1,
      wasmModule: compiledWasmModule, sidParallel: state.sidParallel }, cpuTransfers);
  } catch (error) {
    if (token === generation) showError(error.message || error);
  }
}

function showError(message) {
  invalidateBenchmark("emulator-or-gpu-error");
  releaseAllKeys();
  releaseGamepad();
  worker?.terminate();
  worker = null;
  void stopSidWorker(sidWorker);
  sidWorker = null;
  pendingFrames.length = 0;
  clearAudio();
  state.state = "error";
  state.error = String(message);
  log(message, "error");
  updateUI();
  $("screen-hint").hidden = false;
  $("screen-hint").querySelector("p").textContent = String(message);
}

function schedulePaint() {
  if (paintQueued) return;
  paintQueued = true;
  requestAnimationFrame(() => {
    paintQueued = false;
    if (!pendingFrames.length || state.state === "error") return;
    if (!presenter?.ready) { schedulePaint(); return; }
    const frame = pendingFrames[0];
    try {
      presenter.enabled = state.crtEnabled;
      if (presenter.draw(frame)) {
        state.presentedFrames++;
        state.presentedVicFrameId = frame.frameId;
        state.presentedFrameEmuMs = frame.emuMs;
        state.gpuSubmitCpuMs = presenter.submitCpuMs;
        state.lastFrameAt = performance.now();
        $("screen-hint").hidden = true;
      } else {
        schedulePaint();
        return;
      }
    } catch (error) { showError(error.message || error); }
    pendingFrames.shift();
    if (worker && frame.buffer.byteLength) worker.postMessage({ type: "frame-return", buffer: frame.buffer }, [frame.buffer]);
    if (pendingFrames.length) schedulePaint();
  });
}

function invalidateBenchmark(reason) {
  const measure = state.benchmark;
  if (measure?.running && !measure.invalidReasons.includes(reason)) measure.invalidReasons.push(reason);
}

function startBenchmark() {
  if (state.state !== "running" || state.benchmark?.running) return;
  state.benchmark = {
    running: true, startedAt: null, initialEmuMs: null,
    initialUnderflows: state.audioUnderflows, initialDropped: state.audioDroppedFrames,
    initialPresented: state.presentedFrames, initialGpuCompleted: presenter.completedFrames,
    previousGpuCompleted: presenter.completedFrames, invalidReasons: [], samples: [],
    initialCaptured: state.capturedFrames, initialMissedVic: state.missedVicFrames,
    initialVideoDrops: state.videoBackpressureDrops,
    initialPresentationDrops: state.presentationSkippedFrames,
    maxVideoQueue: pendingFrames.length,
  };
  if (!audioEnabled || state.audioState !== "running") invalidateBenchmark("audio-not-active-at-start");
  if (!state.crtEnabled || !state.gpuReady) invalidateBenchmark("crt-not-active-at-start");
  if (document.hidden) invalidateBenchmark("page-hidden");
  log("Realtime measurement started: 60 seconds, one-second speed window.");
  worker.postMessage({ type: "benchmark-start" });
  updateUI();
}

function sampleBenchmark() {
  const measure = state.benchmark;
  if (!measure?.running || measure.startedAt === null) return;
  const elapsedMs = performance.now() - measure.startedAt;
  const completedFrames = presenter.completedFrames;
  const completedInWindow = completedFrames - measure.previousGpuCompleted;
  measure.previousGpuCompleted = completedFrames;
  measure.samples.push({
    elapsedMs: Math.round(elapsedMs), emuMs: state.emuMs, speedPercent: +state.speedPercent.toFixed(3),
    windowMs: +state.speedWindowMs.toFixed(1), audioEnabled, audioState: state.audioState,
    crtEnabled: state.crtEnabled, gpuReady: state.gpuReady, audioQueuedMs: +state.audioQueuedMs.toFixed(2),
    underflows: state.audioUnderflows, droppedFrames: state.audioDroppedFrames, sidPending: state.sidPending,
    stepCpuMs: +state.stepCpuMs.toFixed(2), sidCpuMs: +state.sidCpuMs.toFixed(2),
    videoCpuMs: +state.videoCpuMs.toFixed(2), audioCpuMs: +state.audioCpuMs.toFixed(2),
    presentedFrames: state.presentedFrames, skippedFrames: state.skippedFrames,
    lastCapturedFrameId: state.lastCapturedFrameId, lastPresentedFrameId: state.presentedVicFrameId,
    capturedFrameEmuMs: state.capturedFrameEmuMs, presentedFrameEmuMs: state.presentedFrameEmuMs,
    capturedFrames: state.capturedFrames, missedVicFrames: state.missedVicFrames,
    videoBackpressureDrops: state.videoBackpressureDrops, presentationSkippedFrames: state.presentationSkippedFrames,
    videoQueue: pendingFrames.length, maxVideoQueue: state.maxVideoQueue,
    audioReservedMs: +state.audioReservedMs.toFixed(2),
    gpuCompletedFrames: completedFrames, gpuCompletedInWindow: completedInWindow, gpuInFlight: presenter.inFlight,
    gpuSubmitCpuMs: +state.gpuSubmitCpuMs.toFixed(2),
    pageHidden: document.hidden,
  });
  if (elapsedMs < 60000) return;
  measure.running = false;
  const endEmuMs = state.emuMs;
  const active = measure.invalidReasons.length === 0 && measure.samples.every((item) => item.audioEnabled && item.audioState === "running" && item.crtEnabled && item.gpuReady && !item.pageHidden);
  const report = {
    schema: "ue2-webrust-realtime-v1", timestamp: new Date().toISOString(), build: state.build, wolfRelease: state.wolfRelease,
    preset: state.preset === 2 ? "mupic" : state.preset === 1 ? "wolf64" : "c64", workload: state.preset === 1 && measure.initialEmuMs >= 40000 ? "wolf-post40s" : "boot-or-menu",
    userAgent: navigator.userAgent,
    crossOriginIsolated: state.crossOriginIsolated, threading: state.threading, audioTransport: state.audioTransport,
    sidParallel: state.sidParallel, sidWorkers: state.sidWorkers, activeSids: state.activeSids,
    sampleRate: audioContext?.sampleRate || null, renderer: state.renderer, sourceSize: [state.width, state.height],
    outputSize: [canvas.width, canvas.height], elapsedMs: +elapsedMs.toFixed(1), emulatedMs: endEmuMs - measure.initialEmuMs,
    averageSpeedPercent: +((endEmuMs - measure.initialEmuMs) / elapsedMs * 100).toFixed(3),
    minimumWindowSpeedPercent: Math.min(...measure.samples.map((item) => item.speedPercent)),
    audioCrtActiveThroughout: active, underflows: state.audioUnderflows - measure.initialUnderflows,
    droppedFrames: state.audioDroppedFrames - measure.initialDropped,
    presentedFrames: state.presentedFrames - measure.initialPresented,
    gpuCompletedFrames: completedFrames - measure.initialGpuCompleted, maxGpuFrameGapMs: +presenter.measurementGapMs().toFixed(2),
    minimumGpuWindowFrames: Math.min(...measure.samples.slice(1).map((item) => item.gpuCompletedInWindow)),
    maxGpuInFlight: presenter.measurement.maxInFlight, invalidReasons: measure.invalidReasons,
    audioQueueMaxMs: Math.max(...measure.samples.map((item) => item.audioQueuedMs)), samples: measure.samples,
    audioReservedMaxMs: Math.max(...measure.samples.map((item) => item.audioReservedMs)),
    videoCadence: {
      capturedFrames: state.capturedFrames - measure.initialCaptured,
      capturedFramesPerSecond: +((state.capturedFrames - measure.initialCaptured) * 1000 / elapsedMs).toFixed(3),
      completedFramesPerSecond: +((completedFrames - measure.initialGpuCompleted) * 1000 / elapsedMs).toFixed(3),
      missedVicFrames: state.missedVicFrames - measure.initialMissedVic,
      videoBackpressureDrops: state.videoBackpressureDrops - measure.initialVideoDrops,
      presentationSkippedFrames: state.presentationSkippedFrames - measure.initialPresentationDrops,
      lastCapturedFrameId: state.lastCapturedFrameId, lastPresentedFrameId: state.presentedVicFrameId,
      maxVideoQueue: measure.maxVideoQueue,
    },
  };
  // Tolerate clock/sample-window rounding by 0.5%; audio underruns and disabled
  // effects still make the full realtime+audio+CRT result fail.
  report.passed = active && report.averageSpeedPercent >= 99.5 && report.underflows === 0 && report.droppedFrames === 0
    && report.gpuCompletedFrames >= 600 && report.minimumGpuWindowFrames >= 10
    && report.maxGpuFrameGapMs <= 250 && report.audioQueueMaxMs <= 150
    && report.videoCadence.capturedFramesPerSecond >= 49 && report.videoCadence.completedFramesPerSecond >= 49
    && report.videoCadence.missedVicFrames === 0 && report.videoCadence.videoBackpressureDrops === 0
    && report.videoCadence.presentationSkippedFrames === 0;
  presenter.measurement = null;
  const completedBenchmark = { ...measure, report };
  state.benchmark = completedBenchmark;
  const result = `${report.averageSpeedPercent.toFixed(1)}% average · ${report.underflows} audio underruns · ${(report.gpuCompletedFrames / (elapsedMs / 1000)).toFixed(1)} GPU frames/s · ${report.passed ? "passed" : "failed"}`;
  log(`Realtime measurement: ${result}.`);
  fetch("benchmark-report", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(report) })
    .then(async (response) => {
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const metadata = await response.json();
      completedBenchmark.saved = metadata;
      log(`Report saved: ${metadata.path || metadata.file || "benchmark-report"}.`);
    }).catch((error) => log(`Report available in window.__ue2Status; save error: ${error.message || error}.`));
  updateUI();
}

function onWorkerMessage(data) {
  switch (data.type) {
    case "mupic-status":
      state.mupic = { ...data.values };
      break;
    case "log": log(data.message, data.level); break;
    case "error": showError(data.message); break;
    case "storage-stats": state.storage = { ...data.stats }; break;
    case "wolf-release":
      state.wolfRelease = data.release;
      log(data.release.source === "published" ? `WOLF64U build ${data.release.revision} selected.` : "WOLF64U initial snapshot selected; no published build yet.");
      updateUI();
      break;
    case "benchmark-baseline":
      if (state.benchmark?.running && state.benchmark.startedAt === null) {
        state.benchmark.startedAt = performance.now();
        state.benchmark.initialEmuMs = data.emuMs;
        state.benchmark.initialGpuCompleted = presenter.completedFrames;
        state.benchmark.initialPresented = state.presentedFrames;
        state.benchmark.previousGpuCompleted = presenter.completedFrames;
        state.benchmark.initialCaptured = data.frames;
        state.benchmark.initialMissedVic = data.missedVicFrames;
        state.benchmark.initialVideoDrops = data.videoBackpressureDrops;
        state.benchmark.initialPresentationDrops = state.presentationSkippedFrames;
        state.benchmark.maxVideoQueue = pendingFrames.length;
        presenter.beginMeasurement();
      }
      break;
    case "video-config":
      presenter.configure(data.shader, data.params).then(() => {
        state.gpuReady = true; log("WebGPU CRT ready · shader and parameters compiled from Rust."); updateUI();
      }).catch(showError);
      break;
    case "state":
      state.state = data.state;
      if (data.state !== "running") releaseGamepad();
      if (data.state === "paused") clearAudio();
      audioNode?.port.postMessage({ type: "enabled", enabled: audioEnabled && data.state === "running" });
      updateUI(data.message);
      break;
    case "download": {
      state.download = { ...data };
      const progress = $("download-progress");
      progress.hidden = false;
      progress.max = data.count;
      progress.value = data.index - 1 + (data.total ? Math.min(1, data.loaded / data.total) : 0);
      $("runtime-status").textContent = `Loading ${data.file.split("/").pop()}…`;
      break;
    }
    case "ready":
      state.workerReady = true;
      state.width = data.width;
      state.height = data.height;
      state.threading = data.threading;
      state.build = data.build || null;
      state.wolfRelease = data.wolfRelease || null;
      // Toggles during asset download arrive before Rust initialization. Apply
      // the latest choice again once this worker can accept audio commands.
      syncAudioTransport();
      log(`WebAssembly ready · ${data.width} × ${data.height} · audio ${audioContext?.sampleRate || 48000} Hz.`);
      updateUI();
      break;
    case "frame":
      // Preserve each A/B colour-mixing phase in arrival order. The worker
      // bounds outstanding transfers; replacing a pending frame loses phases.
      pendingFrames.push(data);
      state.lastCapturedFrameId = data.frameId;
      state.capturedFrameEmuMs = data.emuMs;
      state.capturedFrames++;
      state.maxVideoQueue = Math.max(state.maxVideoQueue, pendingFrames.length);
      if (state.benchmark?.running) state.benchmark.maxVideoQueue = Math.max(state.benchmark.maxVideoQueue, pendingFrames.length);
      schedulePaint();
      state.width = data.width;
      state.height = data.height;
      break;
    case "stats":
      Object.assign(state, { emuMs: data.emuMs, wallMs: data.wallMs, speedPercent: data.speedPercent,
        frames: data.frames, bootPercent: data.bootPercent, stepCpuMs: data.stepCpuMs,
        videoCpuMs: data.videoCpuMs, audioCpuMs: data.audioCpuMs, sidCpuMs: data.sidCpuMs,
        sidPending: data.sidPending, skippedFrames: data.skippedFrames, speedWindowMs: data.speedWindowMs,
        missedVicFrames: data.missedVicFrames, videoBackpressureDrops: data.videoBackpressureDrops,
        videoPending: data.videoPending,
        audioReservedMs: data.audioReservedMs, maxAudioReservedMs: data.maxAudioReservedMs });
      state.gpuCompletedFrames = presenter.completedFrames;
      state.gpuInFlight = presenter.inFlight;
      state.speedHistory.push({ at: performance.now(), emuMs: data.emuMs, speedPercent: data.speedPercent, windowMs: data.speedWindowMs });
      if (state.speedHistory.length > 120) state.speedHistory.shift();
      sampleBenchmark();
      updateUI();
      break;
    case "prg-loaded":
      log(`PRG loaded: ${data.name}.`);
      break;
  }
}

// Physical host-key mapping mirrors crates/ue2emu/src/keymap.rs.
const keys = {
  KeyA: [10], KeyB: [28], KeyC: [20], KeyD: [18], KeyE: [14], KeyF: [21],
  KeyG: [26], KeyH: [29], KeyI: [33], KeyJ: [34], KeyK: [37], KeyL: [42],
  KeyM: [36], KeyN: [39], KeyO: [38], KeyP: [41], KeyQ: [62], KeyR: [17],
  KeyS: [13], KeyT: [22], KeyU: [30], KeyV: [31], KeyW: [9], KeyX: [23], KeyY: [25], KeyZ: [12],
  Digit0: [35], Digit1: [56], Digit2: [59], Digit3: [8], Digit4: [11],
  Digit5: [16], Digit6: [19], Digit7: [24], Digit8: [27], Digit9: [32],
  Period: [44], Comma: [47], Semicolon: [45], Quote: [50], Slash: [55],
  Minus: [40], Equal: [43], BracketLeft: [46], BracketRight: [49], Backslash: [54], Backquote: [57],
  Space: [60], Enter: [1], NumpadEnter: [1], Backspace: [0], Delete: [0],
  ArrowDown: [7], ArrowUp: [15, 7], ArrowRight: [2], ArrowLeft: [15, 2],
  F1: [4], F2: [15, 4], F3: [5], F4: [15, 5], F5: [6], F6: [15, 6], F7: [3], F8: [15, 3],
  F9: [53], F10: [48], F11: [51], Home: [51],
  ShiftLeft: [15], ShiftRight: [52], ControlLeft: [58], ControlRight: [58], AltLeft: [61], AltRight: [61], Escape: [63],
  F12: ["menu"], PageUp: ["restore"],
};
const held = new Map();
const keyRefs = new Map();

function sendKey(key, down) {
  if (typeof key === "number") worker?.postMessage({ type: "key", index: key, down });
  else worker?.postMessage({ type: key, down });
}

function press(source, mapped) {
  if (held.has(source)) return;
  held.set(source, mapped);
  for (const key of mapped) {
    const refs = keyRefs.get(key) || 0;
    keyRefs.set(key, refs + 1);
    if (refs === 0) sendKey(key, true);
  }
}

function release(source) {
  const mapped = held.get(source);
  if (!mapped) return;
  held.delete(source);
  for (const key of [...mapped].reverse()) {
    const refs = (keyRefs.get(key) || 1) - 1;
    if (refs === 0) {
      keyRefs.delete(key);
      sendKey(key, false);
    } else keyRefs.set(key, refs);
  }
}

function releaseAllKeys() {
  for (const source of [...held.keys()]) release(source);
}

function releaseKeyboardKeys() {
  for (const source of [...held.keys()]) {
    if (!source.startsWith("gamepad-key-")) release(source);
  }
}

function sendGamepadEvents(events) {
  for (const event of events) {
    if (event.type === "key") {
      const source = `gamepad-key-${event.index}`;
      if (event.down) press(source, [event.index]);
      else release(source);
    } else worker?.postMessage(event);
  }
}
function releaseGamepad() {
  sendGamepadEvents(gamepadInput.reset());
}
let gamepadReadFailed = false;
function pollGamepads() {
  let pads = [];
  try { pads = navigator.getGamepads?.() || []; }
  catch (error) {
    if (!gamepadReadFailed) log(`Gamepad unavailable: ${error.message || error}`, "error");
    gamepadReadFailed = true;
  }
  const focused = state.workerReady && state.state === "running" && !document.hidden && document.hasFocus();
  sendGamepadEvents(gamepadInput.update(pads, { wolf: state.preset === 1, focused }));
  const connected = Array.from(pads).filter((pad) => pad?.connected && pad.mapping === "standard").length;
  if (connected !== state.gamepads) {
    state.gamepads = connected;
    canvas.dataset.gamepads = String(connected);
  }
  requestAnimationFrame(pollGamepads);
}

window.addEventListener("keydown", (event) => {
  if (document.activeElement !== canvas || state.state !== "running" || event.metaKey) return;
  const mapped = keys[event.code];
  if (!mapped) return;
  event.preventDefault();
  if (!event.repeat) {
    activateAudioFromInput(event);
    press(event.code, mapped);
  }
});
window.addEventListener("keyup", (event) => {
  if (!held.has(event.code)) return;
  event.preventDefault();
  release(event.code);
});
window.addEventListener("blur", () => { releaseAllKeys(); releaseGamepad(); });
canvas.addEventListener("blur", releaseKeyboardKeys);
canvas.addEventListener("pointerdown", () => canvas.focus({ preventScroll: true }));
canvas.addEventListener("click", activateAudioFromInput);
function onVisibilityChange() {
  if (document.hidden) {
    videoWasHidden = true;
    releaseAllKeys(); releaseGamepad(); invalidateBenchmark("page-hidden");
    return;
  }
  if (!videoWasHidden) return;
  videoWasHidden = false;
  // Animation callbacks pause in a hidden tab while emulation/audio can keep
  // advancing. Recycle the old FIFO instead of showing its stale phases.
  state.presentationSkippedFrames += pendingFrames.length;
  for (const frame of pendingFrames.splice(0)) {
    if (worker && frame.buffer.byteLength) worker.postMessage({ type: "frame-return", buffer: frame.buffer }, [frame.buffer]);
  }
  presenter?.resetHistory?.();
}
document.addEventListener("visibilitychange", onVisibilityChange);
controls.c64.addEventListener("click", () => {
  canvas.focus({ preventScroll: true });
  start(mupicPage ? 2 : 0);
});
controls.audio.addEventListener("click", () => setAudio(!audioEnabled));
controls.fullscreen.addEventListener("click", async () => {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await $("display").requestFullscreen();
    canvas.focus({ preventScroll: true });
  } catch (error) { log(`Fullscreen: ${error.message || error}`, "error"); }
});
$("exit-fullscreen").addEventListener("click", async () => {
  try {
    await document.exitFullscreen();
    canvas.focus({ preventScroll: true });
  } catch (error) { log(`Exit fullscreen: ${error.message || error}`, "error"); }
});
document.addEventListener("fullscreenchange", () => {
  controls.fullscreen.textContent = document.fullscreenElement ? "EXIT FULLSCREEN" : "FULLSCREEN";
});
updateUI();
requestAnimationFrame(pollGamepads);
// The standalone MUpic page selects its player; the emulator page starts Wolf.
if (mupicPage) canvas.focus({ preventScroll: true });
start(mupicPage ? 2 : 1);
