import { CrtPresenter } from "./gpu.js";
import { GamepadInput } from "./gamepad.js";
function assetUrl(name) {
  const url = new URL(name, import.meta.url);
  url.search = new URL(import.meta.url).search;
  return url;
}
const $ = (id) => document.getElementById(id);
const canvas = $("screen");
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
const gamepadInput = new GamepadInput();
const state = { state: "idle", workerReady: false, download: null, error: null };

function updateUI(message) {
  const loading = ["loading", "booting"].includes(state.state);
  controls.c64.disabled = loading;
  controls.audio.setAttribute("aria-pressed", String(audioEnabled));
  controls.audio.setAttribute("aria-label", `Audio ${audioEnabled ? "on" : "off"}`);
  controls.audio.title = audioEnabled ? "Turn audio off" : "Turn audio on";
  const status = {
    idle: "Ready.", loading: "Loading files…", booting: "Booting firmware…",
    running: "MUpic Player running.", paused: "Emulation paused.",
    error: state.error || "Emulator error.",
  };
  $("runtime-status").textContent = message || status[state.state] || state.state;
  canvas.dataset.state = state.state;
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
}

async function ensureAudio() {
  if (!audioContext) {
    const AudioContextClass = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextClass) throw new Error("Web Audio is not available in this browser.");
    audioContext = new AudioContextClass({ sampleRate: AUDIO_SAMPLE_RATE, latencyHint: "interactive" });
  }
  if (audioContext.sampleRate !== AUDIO_SAMPLE_RATE) throw new Error("This browser cannot create the required 48 kHz audio output.");
  // Create and resume within the user gesture, before the first await.
  const resumed = audioContext.resume();
  if (!audioSetup) {
    const setup = (async () => {
      if (!audioContext.audioWorklet) throw new Error("AudioWorklet is not available; use localhost or HTTPS.");
      await audioContext.audioWorklet.addModule(assetUrl("audio-worklet.js"));
      const node = new AudioWorkletNode(audioContext, "ue2-audio", {
        outputChannelCount: [2], numberOfInputs: 0, numberOfOutputs: 1, processorOptions: { ring: audioRing },
      });
      node.port.onmessage = ({ data }) => {
        if (audioNode !== node) return;
        receiveAudioReady(data);
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
      console.error(error.message || error);
    }
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

async function start() {
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
  Object.assign(state, { state: "loading", workerReady: false, error: null, download: null });
  $("screen-hint").hidden = false;
  $("screen-hint").querySelector("p").textContent = "Loading emulator…";
  updateUI();
  try {
    if (!presenter) presenter = await CrtPresenter.create(canvas, showError);
    if (token !== generation) return;
    const link = new MessageChannel();
    const { clockLink, audioLink } = workerAudioTransport;
    attachAudioTransport();
    let compiledWasmModule = null;
    const sidParallelAvailable = window.crossOriginIsolated && typeof SharedArrayBuffer === "function";
    const currentSidWorker = new Worker(assetUrl(sidParallelAvailable ? "sid-pool-worker.js" : "sid-worker.js"), { type: "module" });
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
          clearTimeout(timeout); resolve();
        } else if (data.type === "error") {
          clearTimeout(timeout);
          if (ready) showError(data.message); else reject(new Error(data.message));
        }
        else if (data.type === "log") console.error(data.message);
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
    const currentWorker = new Worker(assetUrl("worker.js"), { type: "module" });
    worker = currentWorker;
    currentWorker.onmessage = (event) => {
      if (worker === currentWorker) onWorkerMessage(event.data);
    };
    currentWorker.onerror = (event) => {
      if (worker === currentWorker) showError(event.message || "WebAssembly worker error.");
    };
    const cpuTransfers = [link.port1];
    if (clockLink) cpuTransfers.push(clockLink.port1);
    currentWorker.postMessage({ type: "start", sampleRate: AUDIO_SAMPLE_RATE,
      audioEnabled: false, sidPort: link.port1, ring: audioRing, audioClockPort: clockLink?.port1,
      wasmModule: compiledWasmModule }, cpuTransfers);
  } catch (error) {
    if (token === generation) showError(error.message || error);
  }
}

function showError(message) {
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
  console.error(message);
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
      if (presenter.draw(frame)) {
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

function onWorkerMessage(data) {
  switch (data.type) {
    case "log": console.error(data.message); break;
    case "error": showError(data.message); break;
    case "video-config":
      presenter.configure(data.shader, data.params).catch(showError);
      break;
    case "state":
      state.state = data.state;
      if (data.state !== "running") releaseGamepad();
      if (data.state === "paused") clearAudio();
      audioNode?.port.postMessage({ type: "enabled", enabled: audioEnabled && data.state === "running" });
      updateUI(data.message);
      break;
    case "download": {
      state.download = data;
      const progress = $("download-progress");
      progress.hidden = false;
      progress.max = data.count;
      progress.value = data.index - 1 + (data.total ? Math.min(1, data.loaded / data.total) : 0);
      $("runtime-status").textContent = `Loading ${data.file.split("/").pop()}…`;
      break;
    }
    case "ready":
      state.workerReady = true;
      // Toggles during asset download arrive before Rust initialization. Apply
      // the latest choice again once this worker can accept audio commands.
      syncAudioTransport();
      updateUI();
      break;
    case "frame":
      // Preserve each A/B colour-mixing phase in arrival order. The worker
      // bounds outstanding transfers; replacing a pending frame loses phases.
      pendingFrames.push(data);
      schedulePaint();
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

function sendGamepadEvents(events) {
  for (const event of events) worker?.postMessage(event);
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
  sendGamepadEvents(gamepadInput.update(pads, { focused }));
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
canvas.addEventListener("blur", releaseAllKeys);
canvas.addEventListener("pointerdown", () => canvas.focus({ preventScroll: true }));
canvas.addEventListener("click", activateAudioFromInput);
function onVisibilityChange() {
  if (document.hidden) {
    videoWasHidden = true;
    releaseAllKeys(); releaseGamepad();
    return;
  }
  if (!videoWasHidden) return;
  videoWasHidden = false;
  // Animation callbacks pause in a hidden tab while emulation/audio can keep
  // advancing. Recycle the old FIFO instead of showing its stale phases.
  for (const frame of pendingFrames.splice(0)) {
    if (worker && frame.buffer.byteLength) worker.postMessage({ type: "frame-return", buffer: frame.buffer }, [frame.buffer]);
  }
  presenter?.resetHistory?.();
}
document.addEventListener("visibilitychange", onVisibilityChange);
controls.c64.addEventListener("click", () => {
  canvas.focus({ preventScroll: true });
  start();
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
canvas.focus({ preventScroll: true });
start();
