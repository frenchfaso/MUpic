import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {registerBootAssets} from '../site/boot-assets.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
test('the shipped flash supplies all ROM files and aliases without extra requests', async () => {
  const site = new URL('../site/', import.meta.url);
  const manifest = JSON.parse(await readFile(new URL('manifest.json', site)));
  const requests = [], files = new Map();
  await registerBootAssets(manifest.core, (path, bytes) => files.set(path, bytes), () => {}, {
    baseUrl: site,
    fetcher: async url => {
      requests.push(url);
      return new Response(await readFile(new URL(url)));
    },
  });
  assert.equal(requests.length, 2); // Firmware and flash only.
  assert.ok(requests.every(url => !url.includes('/roms/')));
  for (const entry of manifest.core.filter(e => e.source)) {
    assert.equal(sha(files.get(entry.path)), entry.sha256);
    assert.equal(files.get(entry.path).length, entry.bytes);
    for (const alias of entry.aliases || []) assert.equal(files.get(alias), files.get(entry.path));
  }
  assert.equal(files.get('/roms/basic.bin').length, 8192);
  assert.equal(files.get('/roms/kernal.bin').length, 8192);
  assert.equal(files.get('/roms/characters.bin').length, 4096);
  assert.equal(files.get('/roms/chars.bin').length, 2048);
});

test('invalid extraction and corrupted embedded bytes fail before registration', async () => {
  const data = Uint8Array.of(1, 2, 3, 4);
  const source = {url: 'flash.bin', path: '/flash.bin', bytes: 4, sha256: sha(data)};
  const rom = {source: '/flash.bin', path: '/rom', offset: 1, bytes: 2, sha256: sha(data.subarray(1, 3))};
  const options = {baseUrl: 'https://example.test/', fetcher: async () => new Response(data)};
  for (const change of [{offset: -1}, {offset: 3}, {offset: 0.5}, {source: '/missing'}, {sha256: '0'.repeat(64)}]) {
    const files = new Map();
    await assert.rejects(registerBootAssets([source, {...rom, ...change}],
      (path, bytes) => files.set(path, bytes), () => {}, options), /bounds|SHA-256/);
    assert.equal(files.has('/rom'), false);
  }
});
