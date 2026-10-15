import {fetchBytes} from './fetch-bytes.mjs';

// ROMs remain virtual files for the emulator, but their bytes are downloaded
// only once, inside the verified flash image. Offsets belong to that snapshot.
export async function registerBootAssets(entries, register, onProgress = () => {}, options = {}) {
  const loaded = new Map();
  for (const [index, entry] of entries.entries()) {
    const embedded = typeof entry.source === 'string';
    if (embedded === (typeof entry.url === 'string') ||
        !Number.isSafeInteger(entry.bytes) || entry.bytes <= 0 ||
        (entry.aliases !== undefined && !Array.isArray(entry.aliases)))
      throw new Error('Invalid file manifest entry.');
    const paths = [entry.path, ...(entry.aliases || [])];
    if (paths.some(path => typeof path !== 'string' || !path.startsWith('/') ||
        path.split('/').includes('..') || loaded.has(path)))
      throw new Error('Invalid or duplicate file path in the manifest.');
    let bytes;
    const progress = (count, total) => onProgress({file: entry.path, index: index + 1,
      count: entries.length, loaded: count, total});
    if (embedded) {
      const source = loaded.get(entry.source);
      if (!source || !Number.isSafeInteger(entry.offset) || entry.offset < 0 ||
          entry.offset > source.length - entry.bytes)
        throw new Error(`${entry.path}: invalid embedded asset bounds.`);
      bytes = source.subarray(entry.offset, entry.offset + entry.bytes);
      progress(bytes.length, bytes.length);
    } else {
      bytes = await fetchBytes(entry.url, progress, options);
    }
    if (bytes.length !== entry.bytes) throw new Error(`${entry.path}: size mismatch.`);
    const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
    if (Array.from(digest, b => b.toString(16).padStart(2, '0')).join('') !== entry.sha256)
      throw new Error(`${entry.path}: SHA-256 mismatch.`);
    for (const path of paths) { register(path, bytes); loaded.set(path, bytes); }
  }
}
