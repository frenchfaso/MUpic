// Read-only USB disk: HTTP ranges live in a bounded host cache, never a whole
// image in WASM. A miss yields to fetch while Rust holds the guest clock.
export class RemoteDisk {
  constructor({ url, bytes, regions = [], chunkBytes = 262144, cacheBytes = 16777216,
    transport = "range", chunks = null, compression = "none", storedBytes = null },
    { fetcher = globalThis.fetch.bind(globalThis), onStats = () => {} } = {}) {
    if (typeof url !== "string" || !url || !Number.isSafeInteger(bytes) || bytes < 512 || bytes > 0xffffffff || bytes % 512)
      throw new Error("Invalid remote USB disk.");
    if (!Number.isSafeInteger(chunkBytes) || chunkBytes < 512 || chunkBytes % 512 ||
        !Number.isSafeInteger(cacheBytes) || cacheBytes < chunkBytes * 8 || cacheBytes % chunkBytes)
      throw new Error("Invalid remote disk cache.");
    this.url = url; this.bytes = bytes; this.chunkBytes = chunkBytes;
    if (transport !== "range" && transport !== "chunks") throw new Error("Unknown disk transport.");
    if (transport === "chunks" && (!Array.isArray(chunks) || chunks.length !== Math.ceil(bytes / chunkBytes) ||
      Array.from(chunks).some(h => h !== null && (typeof h !== "string" || !/^[a-f0-9]{64}$/.test(h)))))
      throw new Error("Invalid immutable disk chunks.");
    if (compression !== "none" && compression !== "gzip") throw new Error("Unknown disk chunk compression.");
    if (compression === "gzip" && transport !== "chunks") throw new Error("Compressed disks require immutable chunk transport.");
    if (compression === "gzip" && (!Array.isArray(storedBytes) || storedBytes.length !== chunks.length ||
      Array.from(storedBytes).some((n, i) => !Number.isSafeInteger(n) ||
        (chunks[i] === null ? n !== 0 : n <= 0 || n > chunkBytes + 65536))))
      throw new Error("Invalid compressed disk chunk lengths.");
    if (compression === "gzip" && typeof DecompressionStream !== "function")
      throw new Error("This browser cannot decompress streamed movies. Please update your browser or use the native player.");
    this.transport = transport; this.chunks = chunks; this.compression = compression; this.storedBytes = storedBytes;
    this.capacity = cacheBytes / chunkBytes;
    this.regions = regions.filter(r => Number.isSafeInteger(r.start) && Number.isSafeInteger(r.end) &&
      r.start >= 0 && r.end > r.start && r.end <= bytes);
    this.fetcher = fetcher; this.onStats = onStats;
    this.cache = new Map(); this.inflight = new Map(); this.controllers = new Set();
    this.demand = null; this.failure = null; this.etag = null; this.closed = false;
    this.lastChunk = new Map();
    this.stats = { diskBytes: bytes, chunkBytes, cacheLimitBytes: cacheBytes,
      downloadedBytes: 0, requests: 0, hits: 0, misses: 0, waits: 0, waitMs: 0,
      evictions: 0, cachedBytes: 0, peakCachedBytes: 0, pending: false };
  }
  report() {
    this.stats.cachedBytes = [...this.cache.values()].reduce((n, b) => n + b.length, 0);
    this.stats.peakCachedBytes = Math.max(this.stats.peakCachedBytes, this.stats.cachedBytes);
    this.stats.pending = Boolean(this.demand);
    this.onStats({ ...this.stats });
  }
  indices(offset, length) {
    const first = Math.floor(offset / this.chunkBytes), last = Math.floor((offset + length - 1) / this.chunkBytes);
    return Array.from({ length: last - first + 1 }, (_, i) => first + i);
  }
  // Called synchronously from the USB backend. No fetch, await or guest-memory
  // mutation occurs until every requested byte is available.
  read(offset, length, destination, memory) {
    if (this.failure || this.closed) return -1;
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(length) || length <= 0 ||
        offset < 0 || offset + length > this.bytes || length > this.chunkBytes * (this.capacity - 4) ||
        !Number.isSafeInteger(destination) || destination < 0 ||
        destination + length > memory.buffer.byteLength) {
      this.failure = new Error("Remote USB read is out of bounds."); return -1;
    }
    const indices = this.indices(offset, length);
    if (indices.some(i => !this.cache.has(i))) {
      this.demand = { offset, length, indices }; this.stats.misses++; return 1;
    }
    const heap = new Uint8Array(memory.buffer);
    let at = offset, dst = destination, remaining = length;
    for (const index of indices) {
      const data = this.cache.get(index), start = at - index * this.chunkBytes;
      const count = Math.min(remaining, data.length - start);
      heap.set(data.subarray(start, start + count), dst);
      this.cache.delete(index); this.cache.set(index, data);
      at += count; dst += count; remaining -= count;
    }
    const resumed = Boolean(this.demand);
    this.demand = null; this.stats.hits++;
    if (resumed) this.report();
    this.prefetch(indices.at(-1), offset);
    return 0;
  }
  trim() {
    const pinned = new Set(this.demand?.indices || []);
    for (const index of this.cache.keys()) {
      if (this.cache.size <= this.capacity) break;
      if (!pinned.has(index)) { this.cache.delete(index); this.stats.evictions++; }
    }
  }
  async chunk(index) {
    if (this.cache.has(index)) return;
    if (this.inflight.has(index)) return this.inflight.get(index);
    const operation = this.fetchChunk(index).finally(() => this.inflight.delete(index));
    this.inflight.set(index, operation);
    return operation;
  }
  async fetchChunk(index) {
    const start = index * this.chunkBytes, end = Math.min(start + this.chunkBytes, this.bytes) - 1;
    if (this.transport === "chunks" && this.chunks[index] === null) {
      this.cache.set(index, new Uint8Array(end - start + 1)); this.trim(); this.report(); return;
    }
    const controller = new AbortController(); this.controllers.add(controller);
    const timeout = setTimeout(() => controller.abort(), 20000);
    let response;
    try {
      this.stats.requests++;
      const headers = this.transport === "range" ? { Range: `bytes=${start}-${end}` } : {};
      if (this.etag) headers["If-Range"] = this.etag;
      const url = this.transport === "chunks" ? `${this.url}${String(index).padStart(4, "0")}.bin${this.compression === "gzip" ? ".gz" : ""}` : this.url;
      response = await this.fetcher(url, { headers, signal: controller.signal, cache: this.transport === "chunks" ? "force-cache" : "no-store" });
      if (this.transport === "range") {
      // A server that ignores Range must never trigger a full-image fallback.
      if (response.status !== 206) throw new Error(`Remote disk requires HTTP Range (received ${response.status}).`);
      if (response.headers.get("Content-Range") !== `bytes ${start}-${end}/${this.bytes}`)
        throw new Error("Remote disk returned an incorrect byte range.");
      if (response.headers.get("Content-Type")?.toLowerCase().startsWith("multipart/"))
        throw new Error("Remote disk returned multiple ranges.");
      if (response.headers.get("Content-Encoding") && response.headers.get("Content-Encoding") !== "identity")
        throw new Error("Remote disk ranges must use identity encoding.");
      const declared = response.headers.get("Content-Length");
      if (declared !== null && Number(declared) !== end - start + 1)
        throw new Error("Remote disk returned an incorrect range length.");
      const tag = response.headers.get("ETag");
      if (!tag || tag.startsWith("W/") || !/^"[^"\r\n]+"$/.test(tag))
        throw new Error("Remote disk requires a strong ETag.");
      if (this.etag && tag !== this.etag) throw new Error("Remote disk changed during playback.");
      this.etag = tag;
      } else if (response.status !== 200) {
        throw new Error(`Disk chunk request failed (${response.status}).`);
      }
      const data = new Uint8Array(end - start + 1);
      let at = 0;
      if (!response.body) throw new Error("Remote disk response has no body.");
      const compressed = this.transport === "chunks" && this.compression === "gzip";
      let reader;
      if (compressed) {
        // Explicit .gz assets must reach JavaScript as gzip bytes. A server
        // Content-Encoding would transparently inflate fetch's body, defeating
        // the manifest's separate compressed-input bound and wire-byte count.
        const encoding = response.headers.get("Content-Encoding");
        if (encoding && encoding.toLowerCase() !== "identity")
          throw new Error("Compressed disk assets must be served without Content-Encoding; use the project Pages build.");
        const expected = this.storedBytes[index], declared = response.headers.get("Content-Length");
        if (declared !== null && Number(declared) !== expected)
          throw new Error("Compressed disk chunk has an incorrect stored length.");
        const network = response.body.getReader();
        let received = 0;
        const boundedInput = new ReadableStream({
          pull: async stream => {
            try {
              const {value, done} = await network.read();
              if (done) {
                if (received !== expected) throw new Error("Compressed disk chunk was truncated.");
                stream.close(); return;
              }
              this.stats.downloadedBytes += value.length;
              if (received + value.length > expected) {
                await network.cancel(); throw new Error("Compressed disk chunk is too large.");
              }
              received += value.length; stream.enqueue(value);
            } catch (error) { stream.error(error); }
          },
          cancel: reason => network.cancel(reason),
        });
        reader = boundedInput.pipeThrough(new DecompressionStream("gzip")).getReader();
      } else reader = response.body.getReader();
      try {
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          if (at + value.length > data.length) { await reader.cancel(); throw new Error("Remote disk range is too large."); }
          data.set(value, at); at += value.length;
          if (!compressed) this.stats.downloadedBytes += value.length;
        }
      } catch (error) {
        await reader.cancel(error).catch(() => {});
        if (compressed) throw new Error(`Disk chunk gzip decompression failed: ${error.message || "corrupt or incomplete gzip data"}.`, { cause: error });
        throw error;
      }
      if (at !== data.length) throw new Error("Remote disk range was truncated.");
      if (this.transport === "chunks") {
        const hash = [...new Uint8Array(await crypto.subtle.digest("SHA-256", data))].map(x => x.toString(16).padStart(2, "0")).join("");
        if (hash !== this.chunks[index]) throw new Error("Disk chunk failed its SHA-256 check.");
      }
      this.cache.set(index, data); this.trim(); this.report();
    } catch (error) {
      controller.abort();
      if (response?.body && !response.body.locked) await response.body.cancel().catch(() => {});
      throw error;
    } finally { clearTimeout(timeout); this.controllers.delete(controller); }
  }
  async settle() {
    if (this.failure) throw this.failure;
    if (!this.demand) throw new Error("USB is waiting without a remote read.");
    const begin = performance.now(); this.stats.waits++; this.report();
    try { await Promise.all(this.demand.indices.map(i => this.chunk(i))); }
    catch (error) { this.failure = error; throw error; }
    finally { this.stats.waitMs += performance.now() - begin; this.report(); }
  }
  prefetch(index, offset) {
    const region = this.regions.find(r => offset >= r.start && offset < r.end);
    if (!region) return;
    const previous = this.lastChunk.get(region.start);
    this.lastChunk.set(region.start, index);
    // A header probe alone does not download a movie. Read-ahead starts only
    // after playback traverses two consecutive chunks of that file.
    if (previous !== index - 1 && previous !== index) return;
    if (previous === index && !this.prefetchActive?.has(region.start)) return;
    (this.prefetchActive ||= new Set()).add(region.start);
    for (let ahead = 1; ahead <= 4; ahead++) {
      const next = index + ahead;
      if (next * this.chunkBytes >= region.end || this.inflight.size >= 5) break;
      this.chunk(next).catch(() => {}); // Demand retries transient prefetch failures.
    }
  }
  close() { this.closed = true; for (const c of this.controllers) c.abort(); }
}
