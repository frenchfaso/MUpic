// Behavioral checks for the production USB Range cache; no complete disk is
// registered in WASM. Run: node tests/remote-disk.mjs
import assert from 'node:assert/strict';
import test from 'node:test';
import { RemoteDisk } from '../site/remote-disk.js';

const CHUNK = 512;
const BYTES = 64 * CHUNK;
const TAG = '"disk-version-a"';
const image = (start, length) => Uint8Array.from({ length }, (_, i) => {
  const at = start + i;
  return (at * 17 + (at >>> 9) * 53 + (at >>> 17)) & 255;
});
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((ok, bad) => { resolve = ok; reject = bad; });
  return { promise, resolve, reject };
};

function server({ bytes = BYTES, change = () => {}, before = async () => {} } = {}) {
  const calls = [];
  const observations = { readers: 0, cancellations: 0 };
  const fetcher = async (url, options) => {
    const headers = new Headers(options.headers);
    const range = /^bytes=(\d+)-(\d+)$/.exec(headers.get('Range'));
    assert.ok(range, 'the backend must always request a single bounded Range');
    const start = Number(range[1]), end = Number(range[2]);
    assert.ok(start >= 0 && end >= start && end < bytes, 'network request must stay within the disk');
    const call = { url, start, end, headers, signal: options.signal };
    calls.push(call);
    await before(call);
    const reply = {
      status: 206,
      headers: {
        'Content-Range': `bytes ${start}-${end}/${bytes}`,
        'Content-Length': String(end - start + 1),
        'Content-Type': 'application/octet-stream',
        ETag: TAG,
      },
      data: image(start, end - start + 1),
      noBody: false,
    };
    change(reply, call);
    const stream = reply.noBody ? null : new ReadableStream({
      start(controller) {
        // Deliberately split across non-sector boundaries: HTTP stream packets
        // are unrelated to USB sectors or Range cache chunks.
        const split = Math.min(173, reply.data.length);
        if (split) controller.enqueue(reply.data.subarray(0, split));
        if (split < reply.data.length) controller.enqueue(reply.data.subarray(split));
        controller.close();
      },
    });
    const response = new Response(stream, { status: reply.status, headers: reply.headers });
    // Use real Response and ReadableStream, while observing whether production
    // code tries to consume a response that should be rejected at its headers.
    return {
      status: response.status,
      headers: response.headers,
      get body() {
        if (!response.body) return null;
        return {
          get locked() { return response.body.locked; },
          getReader() { observations.readers++; return response.body.getReader(); },
          cancel() { observations.cancellations++; return response.body.cancel(); },
        };
      },
    };
  };
  return { calls, observations, fetcher };
}

function fixture(options = {}, backend = server()) {
  const snapshots = [];
  const disk = new RemoteDisk({ url: '/movies.img', bytes: BYTES,
    chunkBytes: CHUNK, cacheBytes: 8 * CHUNK, ...options },
  { fetcher: backend.fetcher, onStats: snapshot => snapshots.push(snapshot) });
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2 });
  return { disk, memory, backend, snapshots };
}

async function read(f, offset, length = 512, destination = 123) {
  const heap = new Uint8Array(f.memory.buffer);
  heap.fill(0xa5);
  const result = f.disk.read(offset, length, destination, f.memory);
  assert.ok(result === 0 || result === 1, 'valid read must succeed or wait');
  if (result === 1) {
    assert.ok(heap.every(byte => byte === 0xa5), 'a pending read must leave guest RAM untouched');
    await f.disk.settle();
    assert.equal(f.disk.read(offset, length, destination, f.memory), 0);
  }
  const current = new Uint8Array(f.memory.buffer);
  assert.deepEqual(current.slice(destination, destination + length), image(offset, length));
  assert.ok(current.subarray(0, destination).every(byte => byte === 0xa5));
  assert.ok(current.subarray(destination + length).every(byte => byte === 0xa5));
}

async function drain(f) {
  while (f.disk.inflight.size) await Promise.allSettled([...f.disk.inflight.values()]);
}

await test('sector bytes, unaligned cross-chunk data, and final-sector bounds are exact', async () => {
  const f = fixture();
  await read(f, 0);
  await read(f, CHUNK - 73, 512);
  await read(f, BYTES - 512);
  assert.equal(f.backend.calls.length, 3);
  assert.equal(f.backend.calls.at(-1).end, BYTES - 1);
  f.disk.close();
});

await test('cached reads use current WASM memory after memory.grow', async () => {
  const f = fixture();
  await read(f, 0);
  f.memory.grow(1);
  await read(f, 0, 512, 70_000);
  assert.equal(f.backend.calls.length, 1);
  f.disk.close();
});

await test('concurrent settlement coalesces the same network request', async () => {
  const gate = deferred();
  const f = fixture({}, server({ before: () => gate.promise }));
  assert.equal(f.disk.read(0, 512, 0, f.memory), 1);
  const a = f.disk.settle(), b = f.disk.settle();
  assert.equal(f.backend.calls.length, 1);
  gate.resolve();
  await Promise.all([a, b]);
  await read(f, 0);
  assert.equal(f.backend.calls.length, 1);
  f.disk.close();
});

await test('long nonsequential browsing stays within the cache limit and evicts old sectors', async () => {
  const f = fixture();
  for (let i = 0; i < 12; i++) await read(f, i * CHUNK);
  assert.equal(f.backend.calls.length, 12);
  await read(f, 11 * CHUNK);
  assert.equal(f.backend.calls.length, 12, 'recent sector remains cached');
  await read(f, 0);
  assert.equal(f.backend.calls.length, 13, 'evicted oldest sector is fetched again');
  assert.ok(f.snapshots.every(s => s.cachedBytes <= s.cacheLimitBytes));
  assert.ok(f.disk.stats.evictions > 0);
  f.disk.close();
});

await test('a file header and repeated header probe never start movie read-ahead', async () => {
  const f = fixture({ regions: [{ start: 0, end: 16 * CHUNK },
    { start: 32 * CHUNK, end: 48 * CHUNK }] });
  await read(f, 0, 256);
  await read(f, 32 * CHUNK, 256);
  await read(f, 0, 256);
  await read(f, 32 * CHUNK, 256);
  await drain(f);
  assert.equal(f.backend.calls.length, 2);
  assert.equal(f.disk.stats.downloadedBytes, 2 * CHUNK);
  f.disk.close();
});

await test('sequential playback reads ahead but remains within the movie region', async () => {
  const f = fixture({ regions: [{ start: 0, end: 4 * CHUNK }] });
  await read(f, 0);
  assert.equal(f.backend.calls.length, 1);
  await read(f, CHUNK);
  await drain(f);
  assert.deepEqual(f.backend.calls.map(c => c.start).sort((a, b) => a - b),
    [0, CHUNK, 2 * CHUNK, 3 * CHUNK]);
  await read(f, 2 * CHUNK);
  await drain(f);
  assert.equal(f.backend.calls.length, 4, 'read-ahead data is reused');
  assert.ok(f.snapshots.every(s => s.cachedBytes <= s.cacheLimitBytes));
  f.disk.close();
});

await test('demand spanning several chunks survives concurrent cache eviction', async () => {
  const f = fixture();
  for (let i = 0; i < 8; i++) await read(f, i * CHUNK);
  assert.equal(f.disk.read(8 * CHUNK, 4 * CHUNK, 0, f.memory), 1);
  const demand = f.disk.settle();
  const other = [12, 13, 14, 15].map(i => f.disk.chunk(i));
  await Promise.all([demand, ...other]);
  assert.equal(f.disk.read(8 * CHUNK, 4 * CHUNK, 0, f.memory), 0);
  assert.deepEqual(new Uint8Array(f.memory.buffer, 0, 4 * CHUNK), image(8 * CHUNK, 4 * CHUNK));
  assert.ok(f.snapshots.every(s => s.cachedBytes <= s.cacheLimitBytes));
  f.disk.close();
});

const invalidHeaders = [
  ['HTTP 200 full-image fallback', reply => { reply.status = 200; }, /Range/],
  ['HTTP 416', reply => { reply.status = 416; }, /Range/],
  ['incorrect start/end', reply => { reply.headers['Content-Range'] = `bytes 1-512/${BYTES}`; }, /range/i],
  ['incorrect total image size', reply => { reply.headers['Content-Range'] = `bytes 0-511/${BYTES + 512}`; }, /range/i],
  ['missing Content-Range', reply => { delete reply.headers['Content-Range']; }, /range/i],
  ['incorrect declared length', reply => { reply.headers['Content-Length'] = '1024'; }, /length/i],
  ['compressed byte ranges', reply => { reply.headers['Content-Encoding'] = 'gzip'; }, /encoding/i],
  ['multipart response', reply => { reply.headers['Content-Type'] = 'multipart/byteranges; boundary=bad'; }, /multipart|multiple ranges/i],
  ['missing strong ETag', reply => { delete reply.headers.ETag; }, /ETag|validator/i],
  ['weak ETag', reply => { reply.headers.ETag = `W/${TAG}`; }, /ETag|validator/i],
];
for (const [name, change, expected] of invalidHeaders) {
  await test(`${name} is rejected before reading its body`, async () => {
    const f = fixture({}, server({ change }));
    assert.equal(f.disk.read(0, 512, 0, f.memory), 1);
    await assert.rejects(f.disk.settle(), expected);
    assert.equal(f.backend.observations.readers, 0);
    assert.equal(f.disk.stats.downloadedBytes, 0);
    assert.equal(f.disk.read(0, 512, 0, f.memory), -1);
    assert.ok(f.backend.calls[0].signal.aborted);
    f.disk.close();
  });
}

for (const [name, change, expected] of [
  ['truncated body', reply => { reply.data = reply.data.subarray(0, 511); }, /truncated/i],
  ['oversized body', reply => { reply.data = new Uint8Array(513); }, /too large/i],
  ['missing body', reply => { reply.noBody = true; }, /no body/i],
]) {
  await test(`${name} never becomes a valid guest sector`, async () => {
    const f = fixture({}, server({ change }));
    new Uint8Array(f.memory.buffer).fill(0xa5);
    assert.equal(f.disk.read(0, 512, 0, f.memory), 1);
    await assert.rejects(f.disk.settle(), expected);
    assert.ok(new Uint8Array(f.memory.buffer).every(byte => byte === 0xa5));
    assert.equal(f.disk.read(0, 512, 0, f.memory), -1);
    f.disk.close();
  });
}

for (const missing of [false, true]) {
  await test(`${missing ? 'removed' : 'changed'} ETag rejects mixing disk versions`, async () => {
    const f = fixture({}, server({ change(reply, call) {
      if (call.start !== 0) {
        if (missing) delete reply.headers.ETag;
        else reply.headers.ETag = '"disk-version-b"';
      }
    } }));
    await read(f, 0);
    assert.equal(f.disk.read(CHUNK, 512, 0, f.memory), 1);
    await assert.rejects(f.disk.settle(), /changed|ETag|validator/i);
    assert.equal(f.backend.calls[1].headers.get('If-Range'), TAG);
    assert.equal(f.backend.observations.readers, 1, 'only the first valid version is consumed');
    f.disk.close();
  });
}

await test('an absent Content-Length works when the streamed range length is exact', async () => {
  const f = fixture({}, server({ change(reply) { delete reply.headers['Content-Length']; } }));
  await read(f, 0);
  assert.equal(f.disk.stats.downloadedBytes, CHUNK);
  f.disk.close();
});

await test('a stalled network request is aborted by the production timeout', async () => {
  // Accelerate only the production network deadline, avoiding a twenty-second
  // unit test while exercising the real AbortSignal/catch/finally path.
  const originalTimeout = globalThis.setTimeout;
  globalThis.setTimeout = (callback, delay, ...args) =>
    originalTimeout(callback, delay === 20_000 ? 5 : delay, ...args);
  const f = fixture({}, server({ before: call => new Promise((resolve, reject) => {
    call.signal.addEventListener('abort', () => reject(new DOMException('Network timed out', 'AbortError')), { once: true });
  }) }));
  try {
    assert.equal(f.disk.read(0, 512, 0, f.memory), 1);
    await assert.rejects(f.disk.settle(), { name: 'AbortError' });
    assert.equal(f.disk.read(0, 512, 0, f.memory), -1);
    assert.ok(f.backend.calls[0].signal.aborted);
  } finally { globalThis.setTimeout = originalTimeout; f.disk.close(); }
});

await test('closing the disk cancels its outstanding fetch and forbids later reads', async () => {
  const f = fixture({}, server({ before: call => new Promise((resolve, reject) => {
    call.signal.addEventListener('abort', () => reject(new DOMException('Closed', 'AbortError')), { once: true });
  }) }));
  assert.equal(f.disk.read(0, 512, 0, f.memory), 1);
  const waiting = f.disk.settle();
  f.disk.close();
  await assert.rejects(waiting, { name: 'AbortError' });
  assert.equal(f.disk.read(0, 512, 0, f.memory), -1);
  assert.equal(f.disk.controllers.size, 0);
});

await test('out-of-bounds reads fail without issuing a network request', () => {
  const f = fixture();
  assert.equal(f.disk.read(BYTES, 512, 0, f.memory), -1);
  assert.equal(f.backend.calls.length, 0);
  f.disk.close();
});
