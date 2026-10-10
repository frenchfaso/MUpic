import * as wasmGlue from "./wasm.js";
// Rust owns SID state, sample clocks and the common mixer. The coordinator
// retains one receiver; children transport raw contributions through atomics.
const node = typeof process !== "undefined" && !!process.versions?.node;
const { instantiateUE2 } = node ? wasmGlue.default : globalThis.UE2Wasm;
let nodeThreads;
const runtime = node ? import("node:worker_threads").then(value => { nodeThreads = value; }) : Promise.resolve();
const send = (data, transfer = []) => node ? nodeThreads.parentPort.postMessage(data, transfer) : postMessage(data, transfer);
const MAX_EVENTS = 1_048_576, MAX_RAW = 384_000, RAW_BYTES = MAX_RAW * 4;
const EPOCH = 0, EVENT_BYTES = 1, DONE = 2;
const HEADER = 16, STRIDE = 16, COUNT = 0, MEMORY = 1, ERROR = 2;
const TIMEOUT_MS = 30_000;
let module, cpuPort, audioPort, control, samples;
let capacity = 0, enabled = true, sampleRate = 48000;
let serial = false, initialized = false, closing = false, quarantined = false;
let leader = 0, activeMask = 0, shared = null, queue = Promise.resolve();
let clock = { lo: 0, hi: 0, hz: 985248 };
const children = new Map(), pcmPool = [];
const check = value => { if (value !== 0) throw new Error(module.UTF8ToString(module._ue2_error())); };
const errorText = error => String(error?.message || error);
function fault(error) {
  if (closing || quarantined) return;
  quarantined = true;
  const message = errorText(error);
  cpuPort?.postMessage({ type: "error", message });
  send({ type: "error", message });
}

function publishPcm(pcm) {
  if (control) {
    if (!Atomics.load(control, 5)) return 0;
    const read = Atomics.load(control, 0);
    let write = Atomics.load(control, 1);
    const frames = Math.floor(pcm.length / 2), free = capacity - 1 - ((write - read + capacity) % capacity);
    if (frames > free) { Atomics.add(control, 3, frames); return 0; }
    for (let frame = 0; frame < frames; frame++) {
      samples[write * 2] = pcm[frame * 2] / 32768;
      samples[write * 2 + 1] = pcm[frame * 2 + 1] / 32768;
      write = (write + 1) % capacity;
    }
    Atomics.add(control, 7, 1);
    Atomics.store(control, 1, write);
    Atomics.add(control, 4, frames);
    Atomics.add(control, 7, 1);
    return frames;
  }
  if (audioPort) {
    const byteLength = pcm.length * 2;
    let buffer = pcmPool.pop();
    if (!buffer || buffer.byteLength < byteLength) buffer = new ArrayBuffer(Math.max(4096, byteLength));
    new Int16Array(buffer, 0, pcm.length).set(pcm);
    audioPort.postMessage({ type: "samples", buffer, samples: pcm.length }, [buffer]);
    return Math.floor(pcm.length / 2);
  }
  return 0;
}

function batchReturn(data, audioFramesSent, cpuMs) {
  const transfers = [data.buffer];
  if (data.sampler) transfers.push(data.sampler);
  cpuPort.postMessage({ type: "batch-return", buffer: data.buffer, sampler: data.sampler,
    samplerSamples: data.samplerSamples, audioFramesSent, cpuMs }, transfers);
}

// Bound the mailbox before any SID state advances. Timing is still implemented
// by Rust; this conservative duration/carry bound only limits raw-buffer size.
function preflight(data) {
  const bytes = data.byteLength, samplerCount = data.samplerSamples || 0;
  if (!(data.buffer instanceof ArrayBuffer) || !Number.isSafeInteger(bytes) || bytes <= 0 || bytes % 16 || bytes > MAX_EVENTS || bytes > data.buffer.byteLength) throw new Error("Invalid SID batch.");
  if (!Number.isSafeInteger(samplerCount) || samplerCount < 0 || samplerCount > MAX_RAW || samplerCount % 2 ||
      (samplerCount && (!(data.sampler instanceof ArrayBuffer) || samplerCount * 2 > data.sampler.byteLength))) throw new Error("Invalid SID sampler batch.");
  const words = new Uint32Array(data.buffer, 0, bytes / 4);
  let { lo, hi, hz } = clock, frames = 0, needed = 0, first = 0, active = activeMask;
  for (let i = 0; i < words.length; i += 4) {
    const header = words[i], kind = header & 255, reg = (header >>> 8) & 255, mask = header >>> 16;
    const value = words[i + 1], nextLo = words[i + 2], nextHi = words[i + 3];
    const one = mask !== 0 && (mask & (mask - 1)) === 0;
    if (mask & ~511) throw new Error("SID event has an invalid receiver mask.");
    if (kind < 1 || kind > 8 || (kind === 1 && (!one || value > 1)) || (kind === 2 && !one) ||
        (kind === 5 && (!one || (value & 65535) > 255 || (value >>> 16) > 255)) ||
        (kind === 3 && (reg > 31 || value > 255))) throw new Error("Invalid SID event payload.");
    if (kind === 1) { needed |= mask; active |= mask; if (!first) first = mask; }
    else if (kind === 2) active &= ~mask;
    else if (kind === 7) { lo = nextLo; hi = nextHi; }
    else if (kind === 8) {
      if (value < 100000 || value > 2000000) throw new Error("Invalid SID clock frequency.");
      hz = value;
    }
    if (kind === 3 || kind === 4) {
      if (nextHi < hi || (nextHi === hi && nextLo < lo)) throw new Error("SID event clock regressed.");
      let deltaHi = nextHi - hi, delta = nextLo - lo;
      if (delta < 0) { delta += 4294967296; deltaHi--; }
      if (deltaHi !== 0 || delta > hz * 2) throw new Error("SID event exceeded a two-second slice.");
      if (delta) {
        frames += Math.ceil(delta * 65536 / Math.floor(hz / sampleRate * 65536)) + Math.ceil(delta / 1048576);
        if (frames * 2 > MAX_RAW) throw new Error("SID output exceeds its bounded mailbox before synthesis.");
      }
      lo = nextLo; hi = nextHi;
    }
  }
  return { lo, hi, hz, needed, first, active };
}

function synth(events) {
  const ptr = module._ue2_sid_alloc(events.byteLength);
  if (!ptr && events.byteLength) throw new Error("Unable to allocate SID event memory.");
  module.HEAPU8.set(events, ptr);
  check(module._ue2_sid_process(events.byteLength));
}

function ensureMailbox() {
  if (shared) return;
  const eventBuffer = new SharedArrayBuffer(MAX_EVENTS);
  const controlBuffer = new SharedArrayBuffer((HEADER + 8 * STRIDE) * 4);
  shared = { eventBuffer, controlBuffer, events: new Uint8Array(eventBuffer), control: new Int32Array(controlBuffer) };
}

async function spawnPartition(mask, bootstrap) {
  ensureMailbox();
  const index = children.size, rawBuffer = new SharedArrayBuffer(RAW_BYTES);
  const worker = node ? new nodeThreads.Worker(new URL(import.meta.url)) : new Worker(new URL(import.meta.url), { type: "module" });
  let booted, ready, rejected, timer, settled = false;
  const boot = new Promise(resolve => { booted = resolve; });
  const started = new Promise((resolve, reject) => { ready = resolve; rejected = reject; });
  const record = { worker, mask, index, raw: new Int32Array(rawBuffer), starting: true, termination: null,
    terminate() { this.termination ||= Promise.resolve(worker.terminate()); return this.termination; },
    cancel() { if (!settled) { settled = true; clearTimeout(timer); rejected(new Error("SID pool is closing.")); } return this.terminate(); } };
  children.set(mask, record);
  const onError = error => {
    const failure = new Error(errorText(error));
    if (!settled) { settled = true; clearTimeout(timer); rejected(failure); }
    else if (!closing) fault(failure);
  };
  const onMessage = data => {
    if (data.type === "boot-ready") booted();
    else if (data.type === "partition-ready" && !settled) {
      settled = true; clearTimeout(timer); record.starting = false; ready();
    } else if (data.type === "error") onError(data.message);
  };
  if (node) {
    worker.on("message", onMessage); worker.on("error", onError);
    worker.on("exit", code => {
      if (!closing && !record.termination) onError(new Error(`SID receiver ${mask} exited unexpectedly (${code}).`));
    });
  }
  else { worker.onmessage = event => onMessage(event.data); worker.onerror = onError; }
  timer = setTimeout(() => onError(new Error("The SID partition did not start within 30 seconds.")), TIMEOUT_MS);
  // started is observed immediately; an import error may reject before boot.
  const bootAndStart = boot.then(() => {
    if (closing || settled) return;
    const snapshot = bootstrap.slice(0);
    worker.postMessage({ type: "partition-start", wasmModule: module.wasmModule, sampleRate, mask, index,
      eventBuffer: shared.eventBuffer, controlBuffer: shared.controlBuffer, rawBuffer, snapshot }, [snapshot]);
  });
  await Promise.race([started, bootAndStart.then(() => started)]);
}

async function ensurePartitions(plan) {
  if (!leader && plan.first) { check(module._ue2_sid_partition_select(plan.first)); leader = plan.first; }
  const missing = [];
  for (let bit = 1; bit <= 256; bit <<= 1) if ((plan.needed & bit) && bit !== leader && !children.has(bit)) missing.push(bit);
  if (!missing.length) return;
  const ptr = module._ue2_sid_partition_snapshot(), bytes = module._ue2_sid_partition_snapshot_bytes();
  if (!ptr || bytes !== 244) throw new Error("Invalid SID global clock bootstrap.");
  const bootstrap = module.HEAPU8.slice(ptr, ptr + bytes).buffer;
  await Promise.all(missing.map(mask => spawnPartition(mask, bootstrap)));
}

function publishEvents(events) {
  if (!children.size) return;
  shared.events.set(events);
  Atomics.store(shared.control, DONE, 0);
  Atomics.store(shared.control, EVENT_BYTES, events.byteLength);
  for (const child of children.values()) {
    const base = HEADER + child.index * STRIDE;
    Atomics.store(shared.control, base + ERROR, 0);
    Atomics.store(shared.control, base + COUNT, 0);
  }
  Atomics.add(shared.control, EPOCH, 1);
  Atomics.notify(shared.control, EPOCH, children.size);
}

function waitPartitions() {
  if (!children.size) return;
  const deadline = performance.now() + TIMEOUT_MS;
  for (;;) {
    const completed = Atomics.load(shared.control, DONE);
    if (completed === children.size) break;
    if (completed > children.size) throw new Error("SID partition completion counter exceeded worker count.");
    const remaining = deadline - performance.now();
    if (remaining <= 0) throw new Error("The SID partition did not complete within 30 seconds.");
    Atomics.wait(shared.control, DONE, completed, remaining);
  }
  for (const child of children.values()) if (Atomics.load(shared.control, HEADER + child.index * STRIDE + ERROR)) throw new Error(`SID receiver ${child.mask} failed during synthesis.`);
}

function mixPartitions(data, count) {
  check(module._ue2_sid_mixer_begin(count));
  const add = (part, local = false) => {
    const ptr = module._ue2_sid_mixer_input_alloc(count);
    if (!ptr && count) throw new Error("Unable to allocate SID mixer memory.");
    // Input allocation can grow memory. Obtain the local raw view afterwards.
    if (local) part = new Int32Array(module.memory.buffer, module._ue2_sid_partition_audio(), count);
    new Int32Array(module.memory.buffer, ptr, count).set(part);
    check(module._ue2_sid_mixer_add(count));
  };
  add(null, true);
  for (const child of children.values()) {
    if (Atomics.load(shared.control, HEADER + child.index * STRIDE + COUNT) !== count) throw new Error("SID partition clocks differ.");
    add(child.raw.subarray(0, count));
  }
  const ptr = module._ue2_sid_sampler_alloc(count);
  if (!ptr && count) throw new Error("Unable to allocate sampler memory.");
  const sampler = module.HEAP16.subarray(ptr >>> 1, (ptr >>> 1) + count);
  if (data.samplerSamples) {
    if (data.samplerSamples !== count) throw new Error("SID/sampler clocks differ.");
    sampler.set(new Int16Array(data.sampler, 0, count));
  } else sampler.fill(0);
  check(module._ue2_sid_mixer_finish(count));
}

function poolState() {
  const masks = [];
  for (let bit = 1; bit <= 256; bit <<= 1) if (activeMask & bit) masks.push(bit);
  send({ type: "pool-state", sidWorkers: 1 + children.size, activeSids: masks.length, masks,
    workerMasks: leader ? [leader, ...children.keys()] : [] });
}

async function processBatch(data) {
  const begin = performance.now();
  if (closing || quarantined) { batchReturn(data, 0, 0); return; }
  try {
    const plan = preflight(data), previousWorkers = children.size, previousActive = activeMask;
    const events = new Uint8Array(data.buffer, 0, data.byteLength);
    if (serial) {
      synth(events);
      if (data.samplerSamples) {
        const ptr = module._ue2_sid_sampler_alloc(data.samplerSamples);
        if (!ptr) throw new Error("Unable to allocate sampler memory.");
        module.HEAP16.set(new Int16Array(data.sampler, 0, data.samplerSamples), ptr >>> 1);
        check(module._ue2_sid_mix(data.samplerSamples));
      }
    } else {
      await ensurePartitions(plan);
      if (closing || quarantined) { batchReturn(data, 0, performance.now() - begin); return; }
      publishEvents(events);
      let failure;
      try { synth(events); } catch (error) { failure = error; }
      // No immutable event mailbox is reused until every reader acknowledges,
      // including when local synthesis failed.
      try { waitPartitions(); } catch (error) { failure ||= error; }
      if (failure) throw failure;
      const count = module._ue2_sid_partition_audio_samples();
      if (count > MAX_RAW) throw new Error("SID raw output exceeds its mailbox.");
      mixPartitions(data, count);
    }
    clock = { lo: plan.lo, hi: plan.hi, hz: plan.hz }; activeMask = plan.active;
    const ptr = module._ue2_sid_audio(), count = module._ue2_sid_audio_samples();
    const audioFramesSent = enabled && ptr && count ? publishPcm(module.HEAP16.subarray(ptr >>> 1, (ptr >>> 1) + count)) : 0;
    batchReturn(data, audioFramesSent, performance.now() - begin);
    if (!serial && (activeMask !== previousActive || children.size !== previousWorkers)) poolState();
  } catch (error) {
    if (closing) batchReturn(data, 0, performance.now() - begin);
    else fault(error);
  }
}

async function start(options) {
  if (module || initialized) throw new Error("The SID worker is already initialized.");
  await runtime;
  sampleRate = options.sampleRate || 48000;
  cpuPort = options.cpuPort; audioPort = options.audioPort; enabled = Boolean(options.audioEnabled);
  if (!cpuPort) throw new Error("The SID CPU port is not connected.");
  if (options.ring) {
    control = new Int32Array(options.ring, 0, 8); samples = new Float32Array(options.ring, 32);
    capacity = samples.length / 2;
  }
  if (audioPort) {
    audioPort.onmessage = ({ data }) => { if (data.type === "pcm-return" && data.buffer?.byteLength && pcmPool.length < 4) pcmPool.push(data.buffer); };
    audioPort.start();
  }
  module = await instantiateUE2({ wasmModule: options.wasmModule,
    print: () => {}, printErr: message => send({ type: "log", message: String(message) }) });
  const abi = ["_ue2_sid_partition_control_init", "_ue2_sid_partition_select", "_ue2_sid_partition_snapshot",
    "_ue2_sid_partition_snapshot_bytes", "_ue2_sid_partition_snapshot_alloc", "_ue2_sid_partition_snapshot_import",
    "_ue2_sid_mixer_begin", "_ue2_sid_mixer_input_alloc", "_ue2_sid_mixer_add", "_ue2_sid_mixer_finish"];
  serial = typeof SharedArrayBuffer !== "function" || typeof Atomics.wait !== "function" ||
    (!node && globalThis.crossOriginIsolated !== true) || abi.some(name => typeof module[name] !== "function");
  check(serial ? module._ue2_sid_init(sampleRate) : module._ue2_sid_partition_control_init(sampleRate));
  initialized = true;
  cpuPort.onmessage = ({ data }) => {
    if (data.type !== "events") return;
    queue = queue.then(() => processBatch(data)).catch(fault);
  };
  cpuPort.start();
  send({ type: "ready", transport: control ? "shared-ring" : "transfer-port", sampling: "interpolate", wasmModule: module.wasmModule,
    ...(serial ? {} : { sidParallel: true, threading: "one-SID-per-worker" }) });
  if (!serial) poolState();
}

async function shutdown() {
  enabled = false;
  await Promise.all(Array.from(children.values(), child => child.terminate()));
  children.clear();
  cpuPort?.close(); audioPort?.close();
  send({ type: "closed" });
}

// The child starts at the exact global boundary BEFORE its first Ensure. Rust
// imports metadata/cadence only; its fresh engine is constructed by that same
// live full-stream Ensure, while established engines retain their phase.
async function startPartition(options) {
  await runtime;
  if (module) throw new Error("The SID partition is already initialized.");
  module = await instantiateUE2({ wasmModule: options.wasmModule, print: () => {}, printErr: () => {} });
  check(module._ue2_sid_partition_init(options.sampleRate, options.mask));
  if (!(options.snapshot instanceof ArrayBuffer) || options.snapshot.byteLength !== 244) throw new Error("Invalid SID clock snapshot.");
  const ptr = module._ue2_sid_partition_snapshot_alloc(options.snapshot.byteLength);
  if (!ptr) throw new Error("Unable to allocate SID clock snapshot.");
  module.HEAPU8.set(new Uint8Array(options.snapshot), ptr);
  check(module._ue2_sid_partition_snapshot_import(options.snapshot.byteLength));
  const mailbox = new Int32Array(options.controlBuffer), events = new Uint8Array(options.eventBuffer);
  const raw = new Int32Array(options.rawBuffer), base = HEADER + options.index * STRIDE;
  let seen = Atomics.load(mailbox, EPOCH);
  Atomics.store(mailbox, base + MEMORY, module.memory.buffer.byteLength);
  send({ type: "partition-ready" });
  for (;;) {
    let epoch = Atomics.load(mailbox, EPOCH);
    while (epoch === seen) { Atomics.wait(mailbox, EPOCH, seen); epoch = Atomics.load(mailbox, EPOCH); }
    seen = epoch;
    try {
      const bytes = Atomics.load(mailbox, EVENT_BYTES);
      if (bytes <= 0 || bytes > MAX_EVENTS || bytes % 16) throw new Error("Invalid SID mailbox batch.");
      synth(events.subarray(0, bytes));
      const count = module._ue2_sid_partition_audio_samples();
      if (count > MAX_RAW) throw new Error("SID raw output exceeds its mailbox.");
      raw.set(new Int32Array(module.memory.buffer, module._ue2_sid_partition_audio(), count));
      Atomics.store(mailbox, base + COUNT, count);
    } catch (_) {
      Atomics.store(mailbox, base + ERROR, 1);
      Atomics.store(mailbox, base + COUNT, 0);
    } finally {
      Atomics.store(mailbox, base + MEMORY, module.memory.buffer.byteLength);
      Atomics.add(mailbox, DONE, 1);
      Atomics.notify(mailbox, DONE, 1);
    }
  }
}

function onMainMessage(data) {
  if (data.type === "start") queue = queue.then(() => start(data)).catch(fault);
  else if (data.type === "partition-start") startPartition(data).catch(error => send({ type: "error", message: errorText(error) }));
  else if (data.type === "audio") {
    enabled = Boolean(data.enabled) && !closing;
    if (Number.isSafeInteger(data.request)) send({ type: "audio-ready", request: data.request });
  } else if (data.type === "close") {
    closing = true; enabled = false;
    for (const child of children.values()) if (child.starting) child.cancel();
    queue = queue.then(shutdown).catch(error => { send({ type: "error", message: errorText(error) }); send({ type: "closed" }); });
  }
}
if (node) runtime.then(() => { nodeThreads.parentPort.on("message", onMainMessage); send({ type: "boot-ready" }); });
else { self.onmessage = event => onMainMessage(event.data); send({ type: "boot-ready" }); }
