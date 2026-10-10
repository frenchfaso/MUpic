import test from 'node:test';
import assert from 'node:assert/strict';
import {createGpuPlanner} from '../site/gpu-planner.js';

function installGpu() {
  const names = ['navigator', 'fetch', 'GPUBufferUsage', 'GPUMapMode'];
  const previous = names.map(name => Object.getOwnPropertyDescriptor(globalThis, name));
  const pending = [], buffers = [], bindings = [];
  let peak = 0, lost;
  const device = {
    lost: new Promise(resolve => { lost = resolve; }),
    limits: {maxComputeWorkgroupsPerDimension: 65535, maxStorageBufferBindingSize: 128 * 1024 * 1024},
    pushErrorScope() {}, async popErrorScope() {}, createShaderModule() {},
    async createComputePipelineAsync() { return {getBindGroupLayout() { return {}; }}; },
    createBuffer({size}) {
      const buffer = {
        bytes: new Uint8Array(size), mapState: 'unmapped', destroyed: false,
        mapAsync() {
          this.mapState = 'pending';
          return new Promise((resolve, reject) => {
            pending.push({buffer: this, resolve() { buffer.mapState = 'mapped'; resolve(); }, reject});
            peak = Math.max(peak, pending.length);
          });
        },
        getMappedRange(offset, size) { assert.equal(this.mapState, 'mapped'); return this.bytes.buffer.slice(offset, offset + size); },
        unmap() { this.mapState = 'unmapped'; },
        destroy() { this.destroyed = true; this.mapState = 'unmapped'; },
      };
      buffers.push(buffer); return buffer;
    },
    createBindGroup({entries}) { bindings.push(entries); return entries; },
    createCommandEncoder() {
      const operations = []; let bind;
      return {
        beginComputePass() {
          return {
            setPipeline() {}, setBindGroup(_, entries) { bind = entries; }, dispatchWorkgroups() {},
            end() {
              operations.push(() => {
                const input = bind[1].resource, output = bind[2].resource;
                const keys = new Uint32Array(input.buffer.bytes.buffer, 0, input.size / 4);
                const hints = new Uint32Array(output.buffer.bytes.buffer, 0, output.size / 4);
                for (let i = 0; i < keys.length; i++) hints[i] = keys[i] ^ 0x5a5a5a;
              });
            },
          };
        },
        copyBufferToBuffer(source, sourceOffset, target, targetOffset, size) {
          operations.push(() => target.bytes.set(source.bytes.subarray(sourceOffset, sourceOffset + size), targetOffset));
        },
        finish() { return operations; },
      };
    },
    queue: {
      writeBuffer(buffer, offset, data) { buffer.bytes.set(new Uint8Array(data.buffer, data.byteOffset, data.byteLength), offset); },
      submit(commands) { for (const operations of commands) for (const operation of operations) operation(); },
    },
    destroy() { while (pending.length) pending.shift().reject(new Error('GPU destroyed')); lost({}); },
  };
  Object.defineProperty(globalThis, 'navigator', {value: {gpu: {async requestAdapter() { return {async requestDevice() { return device; }}; }}}, configurable: true});
  globalThis.fetch = async () => new Response('test shader');
  globalThis.GPUBufferUsage = {STORAGE: 1, COPY_DST: 2, COPY_SRC: 4, MAP_READ: 8};
  globalThis.GPUMapMode = {READ: 1};
  return {
    pending, buffers, bindings, get peak() { return peak; },
    complete(index = 0) { pending.splice(index, 1)[0].resolve(); },
    restore() { names.forEach((name, index) => { if (previous[index]) Object.defineProperty(globalThis, name, previous[index]); else delete globalThis[name]; }); },
  };
}
const advance = () => new Promise(resolve => setImmediate(resolve));

test('GPU chunks reuse two independent buffers and preserve compact hint order', async () => {
  const gpu = installGpu(); let planner;
  try {
    planner = await createGpuPlanner(new Uint32Array(8));
    const keys = Uint32Array.from({length: 131075}, (_, index) => index);
    const result = planner.seeds(keys);
    await advance(); assert.equal(gpu.pending.length, 2);
    gpu.complete(1); await advance(); assert.equal(gpu.pending.length, 2);
    gpu.complete(0); gpu.complete(0);
    assert.deepEqual(await result, keys.map(key => key ^ 0x5a5a5a));
    assert.equal(gpu.peak, 2); assert.equal(gpu.buffers.length, 7);
    assert.deepEqual(gpu.bindings.map(bind => bind[1].resource.size), [262144, 262144, 12]);

    const smaller = planner.seeds(Uint32Array.of(0xff, 0x102030));
    await advance(); gpu.complete();
    assert.deepEqual(await smaller, Uint32Array.of(0xff ^ 0x5a5a5a, 0x102030 ^ 0x5a5a5a));
    assert.equal(gpu.buffers.length, 7);
    assert.equal(gpu.bindings.at(-1)[1].resource.size, 8);
    assert.deepEqual(await planner.seeds(new Uint32Array()), new Uint32Array());
    await assert.rejects(planner.seeds(new Uint8Array(3)), /compact RGB keys/);
  } finally { planner?.destroy(); gpu.restore(); }
});

test('GPU cancellation rejects active maps and waiting chunks', async () => {
  const gpu = installGpu(); let planner;
  try {
    planner = await createGpuPlanner(new Uint32Array(8));
    const pending = planner.seeds(new Uint32Array(131075));
    const rejected = assert.rejects(pending, /GPU destroyed|device was lost/);
    await advance(); assert.equal(gpu.pending.length, 2);
    planner.destroy(); await rejected; await advance();
    assert.ok(gpu.buffers.every(buffer => buffer.destroyed));
    await assert.rejects(planner.seeds(Uint32Array.of(1)), /device was lost/);
  } finally { planner?.destroy(); gpu.restore(); }
});
