// The GPU provides candidate hints; Rust/WASM always refines them with exact
// f64 CIEDE2000. This shares the native encoder's WGSL and packed tree ABI.
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
    let alive = true;
    device.lost.then(() => { alive = false; });
    let pending = Promise.resolve();

    async function calculate(rgb) {
      if (!alive) throw new Error('GPU device was lost; continue with CPU workers');
      if (!(rgb instanceof Uint8Array) || rgb.length % 3) throw new Error('GPU planner requires RGB triplets');
      const count = rgb.length / 3;
      if (!count) return new Uint32Array();
      // Repeated pixels are common in dithered-video sources and black bars.
      // Only distinct RGB keys reach the shader; expand its hints afterwards.
      const unique = new Map();
      const keyList = [];
      const positions = new Uint32Array(count);
      for (let i = 0, p = 0; i < count; i++, p += 3) {
        const key = (rgb[p] << 16) | (rgb[p + 1] << 8) | rgb[p + 2];
        let index = unique.get(key);
        if (index === undefined) { index = keyList.length; unique.set(key, index); keyList.push(key); }
        positions[i] = index;
      }
      const keys = new Uint32Array(keyList);
      if (keys.length > device.limits.maxComputeWorkgroupsPerDimension * 64 || keys.byteLength > device.limits.maxStorageBufferBindingSize) {
        throw new Error('GPU seed batch exceeds device limits; continue with CPU workers');
      }
      const size = keys.byteLength;
      const input = device.createBuffer({ size, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST });
      const output = device.createBuffer({ size, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC });
      const readback = device.createBuffer({ size, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
      try {
        device.queue.writeBuffer(input, 0, keys);
        const bind = device.createBindGroup({ layout: pipeline.getBindGroupLayout(0), entries: [
          { binding: 0, resource: { buffer: tree } },
          { binding: 1, resource: { buffer: input } },
          { binding: 2, resource: { buffer: output } },
        ] });
        const encoder = device.createCommandEncoder();
        const pass = encoder.beginComputePass();
        pass.setPipeline(pipeline); pass.setBindGroup(0, bind);
        pass.dispatchWorkgroups(Math.ceil(keys.length / 64)); pass.end();
        encoder.copyBufferToBuffer(output, 0, readback, 0, size);
        device.queue.submit([encoder.finish()]);
        await readback.mapAsync(GPUMapMode.READ);
        const hints = new Uint32Array(readback.getMappedRange());
        const expanded = new Uint32Array(count);
        for (let i = 0; i < count; i++) expanded[i] = hints[positions[i]];
        readback.unmap();
        return expanded;
      } finally { input.destroy(); output.destroy(); readback.destroy(); }
    }

    return {
      // Serialise map/readback work. The independent CPU worker pool still
      // converts earlier frames while the GPU prepares later frame hints.
      seeds(rgb) {
        const job = pending.then(() => calculate(rgb));
        pending = job.catch(() => {});
        return job;
      },
      destroy() { alive = false; tree.destroy(); device.destroy(); },
    };
  } catch (error) {
    device?.destroy();
    console.info('MUpic WebGPU hints unavailable; CPU workers remain active.', error?.message || error);
    return null;
  }
}
