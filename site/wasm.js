// Browser ABI glue for a standalone Rust wasm32-unknown-unknown module.
// No generated runtime, libc, filesystem emulation or shared mutable memory.
(() => {
const encoder = new TextEncoder();
const decoder = new TextDecoder();

function adaptUE2(instance, wasmModule) {
  const exports = instance.exports;
  const memory = exports.memory;
  if (!(memory instanceof WebAssembly.Memory)) throw new Error("The Rust module does not export WASM memory.");
  const api = { wasmModule, memory };
  for (const [name, value] of Object.entries(exports)) {
    if (typeof value === "function") api[`_${name}`] = value;
  }
  let buffer, u8, i16, f32;
  function refresh() {
    if (buffer === memory.buffer) return;
    buffer = memory.buffer;
    u8 = new Uint8Array(buffer);
    i16 = new Int16Array(buffer);
    f32 = new Float32Array(buffer);
  }
  Object.defineProperties(api, {
    HEAPU8: { get() { refresh(); return u8; } },
    HEAP16: { get() { refresh(); return i16; } },
    HEAPF32: { get() { refresh(); return f32; } },
  });
  api.UTF8ToString = (ptr) => {
    const heap = api.HEAPU8;
    if (!Number.isInteger(ptr) || ptr < 0 || ptr >= heap.length) return "Invalid WASM error pointer.";
    let end = ptr;
    const limit = Math.min(heap.length, ptr + 65536);
    while (end < limit && heap[end] !== 0) end++;
    return decoder.decode(heap.subarray(ptr, end));
  };
  api.registerFile = (path, data) => {
    if (typeof path !== "string" || !path.startsWith("/") || path.includes("\0")) throw new Error("Invalid VFS path.");
    if (!(data instanceof Uint8Array)) throw new Error("A VFS file must be a byte buffer.");
    if (!api._ue2_alloc || !api._ue2_dealloc || !api._ue2_file_register) throw new Error("The Rust VFS ABI is unavailable.");
    const pathBytes = encoder.encode(path);
    const dataAllocation = Math.max(1, data.length);
    let pathPtr = 0, dataPtr = 0;
    try {
      pathPtr = api._ue2_alloc(pathBytes.length);
      if (!pathPtr) throw new Error("Insufficient WASM memory for the path.");
      dataPtr = api._ue2_alloc(dataAllocation);
      if (!dataPtr) throw new Error("Insufficient WASM memory for the file.");
      // Both allocations can grow memory: read the current view afterwards.
      api.HEAPU8.set(pathBytes, pathPtr);
      api.HEAPU8.set(data, dataPtr);
      if (api._ue2_file_register(pathPtr, pathBytes.length, dataPtr, data.length) !== 0) {
        throw new Error(api.UTF8ToString(api._ue2_error()));
      }
    } finally {
      if (dataPtr) api._ue2_dealloc(dataPtr, dataAllocation);
      if (pathPtr) api._ue2_dealloc(pathPtr, pathBytes.length);
    }
  };
  return api;
}

async function instantiateUE2({ wasmModule, wasmUrl, print = () => {}, printErr = print,
  utcMs = () => Date.now(), monotonicMs = () => performance.now(), remoteRead } = {}) {
  let compiled = wasmModule;
  if (!compiled) {
    const url = wasmUrl || new URL("./ue2_web.wasm", globalThis.location.href);
    if (!wasmUrl) url.search = new URL(globalThis.location.href).search;
    const response = await fetch(url, { cache: "no-cache" });
    if (!response.ok) throw new Error(`ue2_web.wasm: HTTP ${response.status}`);
    const mime = response.headers.get("Content-Type")?.split(";")[0].trim();
    compiled = WebAssembly.compileStreaming && mime === "application/wasm"
      ? await WebAssembly.compileStreaming(response)
      : await WebAssembly.compile(await response.arrayBuffer());
  }
  let instance;
  const pendingLogs = [];
  const decodeLog = (ptr, length) => decoder.decode(new Uint8Array(instance.exports.memory.buffer, ptr, length));
  const imports = { ue2_host: {
    utc_ms: utcMs,
    monotonic_ms: monotonicMs,
    remote_read: (offset, length, destination) => remoteRead && instance
      ? remoteRead(offset >>> 0, length >>> 0, destination >>> 0, instance.exports.memory) : -1,
    log: (ptr, length) => {
      if (!instance) pendingLogs.push({ ptr, length });
      else printErr(decodeLog(ptr, length));
    },
  } };
  try {
    instance = await WebAssembly.instantiate(compiled, imports);
    for (const message of pendingLogs) printErr(decodeLog(message.ptr, message.length));
    return adaptUE2(instance, compiled);
  } catch (error) {
    printErr(`Rust WASM: ${error.message || error}`);
    throw error;
  }
}
const api = Object.freeze({ adaptUE2, instantiateUE2 });
if (typeof module !== "undefined" && module.exports) module.exports = api;
else globalThis.UE2Wasm = api;
})();
