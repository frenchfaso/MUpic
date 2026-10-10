// The GPU provides compact candidate hints; shared Rust code makes every final
// colour decision with exact f64 CIEDE2000 in both native and WASM encoders.
export async function createGpuPlanner(searchData, { shaderUrl = './assets/author-seed.wgsl' } = {}) {
  if (!globalThis.navigator?.gpu || !(searchData instanceof Uint32Array) || searchData.length < 8) return null;
  let device;
  try {
    const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
    if (!adapter) return null;
    device = await adapter.requestDevice();
    const source = await fetch(shaderUrl);
    if (!source.ok) throw new Error(`GPU shader HTTP ${source.status}`);
    device.pushErrorScope('validation');
    const module = device.createShaderModule({ label: 'MUpic author hints', code: await source.text() });
    const pipeline = await device.createComputePipelineAsync({ layout: 'auto', compute: { module, entryPoint: 'main' } });
    const error = await device.popErrorScope();
    if (error) throw error;
    const tree = device.createBuffer({ size: searchData.byteLength, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST });
    device.queue.writeBuffer(tree, 0, searchData);
    const slots = Array.from({ length: 2 }, () => ({ busy: false, size: 0 }));
    const waiters = [];
    let alive = true;
    function releaseWaiters() { while (waiters.length) waiters.shift()(); }
    device.lost.then(() => { alive = false; releaseWaiters(); });

    function destroyBuffers(slot) {
      slot.input?.destroy(); slot.output?.destroy(); slot.readback?.destroy();
      slot.size = 0;
    }

    async function acquire() {
      while (alive) {
        const slot = slots.find(value => !value.busy);
        if (slot) { slot.busy = true; return slot; }
        await new Promise(resolve => waiters.push(resolve));
      }
      throw new Error('GPU device was lost; continue with CPU workers');
    }

    const batchKeys = Math.min(65536, Math.floor(device.limits.maxStorageBufferBindingSize / 4),
      device.limits.maxComputeWorkgroupsPerDimension * 64);
    async function calculate(keys) {
      const slot = await acquire();
      try {
        const size = keys.byteLength;
        if (slot.size < size) {
          destroyBuffers(slot);
          slot.size = Math.max(256, 2 ** Math.ceil(Math.log2(size)));
          slot.input = device.createBuffer({ size: slot.size, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST });
          slot.output = device.createBuffer({ size: slot.size, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC });
          slot.readback = device.createBuffer({ size: slot.size, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
        }
        device.queue.writeBuffer(slot.input, 0, keys);
        // Bind the actual range: arrayLength(keys) must exclude unused capacity.
        const bind = device.createBindGroup({ layout: pipeline.getBindGroupLayout(0), entries: [
          { binding: 0, resource: { buffer: tree } },
          { binding: 1, resource: { buffer: slot.input, size } },
          { binding: 2, resource: { buffer: slot.output, size } },
        ] });
        const encoder = device.createCommandEncoder();
        const pass = encoder.beginComputePass();
        pass.setPipeline(pipeline); pass.setBindGroup(0, bind);
        pass.dispatchWorkgroups(Math.ceil(keys.length / 64)); pass.end();
        encoder.copyBufferToBuffer(slot.output, 0, slot.readback, 0, size);
        device.queue.submit([encoder.finish()]);
        await slot.readback.mapAsync(GPUMapMode.READ);
        return new Uint32Array(slot.readback.getMappedRange(0, size)).slice();
      } finally {
        if (slot.readback?.mapState === 'mapped') slot.readback.unmap();
        slot.busy = false;
        waiters.shift()?.();
      }
    }

    return {
      async seeds(keys) {
        if (!(keys instanceof Uint32Array)) throw new Error('GPU planner requires compact RGB keys');
        if (!alive) throw new Error('GPU device was lost; continue with CPU workers');
        const result = new Uint32Array(keys.length);
        await Promise.all(Array.from({ length: Math.ceil(keys.length / batchKeys) }, async (_, index) => {
          const offset = index * batchKeys;
          result.set(await calculate(keys.subarray(offset, offset + batchKeys)), offset);
        }));
        return result;
      },
      destroy() {
        alive = false; releaseWaiters();
        slots.forEach(destroyBuffers); tree.destroy(); device.destroy();
      },
    };
  } catch (error) {
    device?.destroy();
    console.info('MUpic WebGPU hints unavailable; CPU workers remain active.', error?.message || error);
    return null;
  }
}
