#!/usr/bin/env python3
"""Validate a complete offline RGB24 table and publish its lossless asset."""
import argparse
import array
import gzip
import hashlib
import json
import pathlib
import re
import shutil
import struct
import subprocess
import sys
import time

DOMAIN = 1 << 24
BLOCK_SIZE = 4096
BLOCK_COUNT = DOMAIN // BLOCK_SIZE
BLOCK_BYTES = 80 + BLOCK_SIZE * 4
ROOT = pathlib.Path(__file__).resolve().parents[1]


def validate_asset(site, *, asset_path=None, manifest=None):
    """Fail publication if the distributed table is missing, stale or damaged."""
    directory = site / 'assets'
    if manifest is None:
        manifest = json.loads((directory / 'rgb24-lut.json').read_text())
    if (manifest['schema'] != 1 or manifest['compression'] != 'gzip'
            or manifest['asset'] != 'rgb24-lut.bin.gz'
            or not re.fullmatch('[a-f0-9]{64}', manifest['algorithm_identity'])):
        raise ValueError('Invalid distributed LUT manifest')
    core = (ROOT / 'encoder/src/core.rs').read_text()
    palette = re.search(r'pub const PALETTE:.*?\n\];', core, flags=re.S).group()
    identity = hashlib.sha256((ROOT / 'encoder/src/author_dither.rs').read_bytes()
                             + palette.encode()).hexdigest()
    if identity != manifest['algorithm_identity']:
        raise ValueError('Distributed LUT does not match the current algorithm/palette')
    path = asset_path or directory / manifest['asset']
    compressed_hash = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b''):
            compressed_hash.update(chunk)
    if (path.stat().st_size != manifest['compressed_bytes']
            or compressed_hash.hexdigest() != manifest['compressed_sha256']):
        raise ValueError('Distributed LUT compressed checksum mismatch')
    with gzip.open(path, 'rb') as stream:
        header = stream.read(128)
        if (len(header) != 128 or header[:8] != b'MUPLUT1\0'
                or struct.unpack_from('<I', header, 8)[0] != 1
                or header[12] not in (0, 1) or header[13:16] != bytes(3)
                or struct.unpack_from('<I', header, 16)[0] != DOMAIN
                or header[32:64].hex() != manifest['algorithm_identity']):
            raise ValueError('Invalid distributed LUT header')
        expected = DOMAIN * (4 if header[12] == 0 else 3)
        if (struct.unpack_from('<Q', header, 24)[0] != expected
                or manifest['uncompressed_bytes'] != expected + 128):
            raise ValueError('Invalid distributed LUT size')
        full_hash, payload_hash = hashlib.sha256(header), hashlib.sha256()
        received = 0
        for chunk in iter(lambda: stream.read(1 << 20), b''):
            received += len(chunk)
            if received > expected:
                raise ValueError('Distributed LUT exceeds declared size')
            full_hash.update(chunk)
            payload_hash.update(chunk)
        if (received != expected or payload_hash.digest() != header[96:128]
                or full_hash.hexdigest() != manifest['uncompressed_sha256']):
            raise ValueError('Distributed LUT expanded checksum mismatch')
    # The compiled shared core owns candidate ordering, patterns and ID bounds.
    # Validate through its production importer instead of duplicating that logic.
    node = shutil.which('node')
    if not node:
        raise ValueError('Node.js is required to validate the distributed LUT')
    script = """
import {readFileSync, createReadStream} from 'node:fs';
import {createGunzip} from 'node:zlib';
import {pathToFileURL} from 'node:url';
const [adapter, wasm, asset, sha] = process.argv.slice(1);
const {EncoderCore} = await import(pathToFileURL(adapter).href);
const {instance} = await WebAssembly.instantiate(readFileSync(wasm), {});
const core = new EncoderCore(instance.exports);
let header = Buffer.alloc(0), begun = false;
for await (let chunk of createReadStream(asset).pipe(createGunzip())) {
  if (!begun) {
    const need = 128 - header.length;
    header = Buffer.concat([header, chunk.subarray(0, need)]);
    chunk = chunk.subarray(need);
    if (header.length < 128) continue;
    core.beginLutAsset(header); begun = true;
  }
  if (chunk.length) core.appendLutAsset(chunk);
}
if (!begun) throw new Error('Truncated LUT header');
core.finishLutAsset(sha);
"""
    subprocess.run([node, '--input-type=module', '--eval', script,
        str(ROOT / 'site/wasm-core.mjs'), str(directory / 'encoder.wasm'),
        str(path), manifest['uncompressed_sha256']], check=True,
        stdout=subprocess.DEVNULL)
    return manifest


def read_plans(directory, candidate_count):
    plans = array.array('I')
    identity = None
    for number in range(BLOCK_COUNT):
        path = directory / f'block-{number:04d}.lut'
        raw = path.read_bytes()
        if (len(raw) != BLOCK_BYTES or raw[:4] != b'MULT'
                or struct.unpack_from('<II', raw, 4) != (1, number)
                or raw[12:16] != bytes(4)):
            raise ValueError(f'Invalid block: {path}')
        if identity is None:
            identity = raw[16:48]
        if raw[16:48] != identity:
            raise ValueError(f'Mixed generator identities: {path}')
        if hashlib.sha256(raw[:48] + raw[80:]).digest() != raw[48:80]:
            raise ValueError(f'Checksum mismatch: {path}')
        block = array.array('I')
        block.frombytes(raw[80:])
        if sys.byteorder != 'little':
            block.byteswap()
        if max(block) >= candidate_count:
            raise ValueError(f'Invalid candidate ID: {path}')
        plans.extend(block)
    assert len(plans) == DOMAIN
    return plans, identity.hex()


def envelope(payload, encoding, metadata):
    return (b'MUPLUT1\0' + struct.pack('<IB3xIIQ', 1, encoding, DOMAIN,
            metadata['candidateCount'], len(payload))
            + bytes.fromhex(metadata['algorithmIdentity'])
            + bytes.fromhex(metadata['patternFingerprint'])
            + hashlib.sha256(payload).digest() + payload)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('blocks', type=pathlib.Path,
                        help='Directory containing all 4096 block-NNNN.lut files')
    parser.add_argument('--metadata', type=pathlib.Path, required=True,
                        help='lut_metadata.json emitted by the C generator')
    parser.add_argument('--output', type=pathlib.Path,
                        default=ROOT / 'site/assets/rgb24-lut.bin.gz')
    parser.add_argument('--compare', action='store_true',
                        help='Also compare Brotli, Zstandard and XZ when installed')
    parser.add_argument('--report', type=pathlib.Path,
                        default=ROOT / 'build/lut-compression.json')
    args = parser.parse_args()
    if args.output.name != 'rgb24-lut.bin.gz':
        parser.error('--output must be named rgb24-lut.bin.gz')
    metadata = json.loads(args.metadata.read_text())
    if (metadata['domain'] != DOMAIN or metadata['headerBytes'] != 128
            or not 0 < metadata['candidateCount'] <= (1 << 24)):
        raise ValueError('Unsupported LUT metadata')
    plans, generator_identity = read_plans(args.blocks, metadata['candidateCount'])
    if metadata.get('blockIdentity', generator_identity) != generator_identity:
        raise ValueError('Blocks do not match their generator metadata')
    if sys.byteorder != 'little':
        plans.byteswap()
    u32 = plans.tobytes()
    # Three interleaved byte lanes retain RGB-key order while omitting padding.
    u24 = bytearray(DOMAIN * 3)
    u24[0::3], u24[1::3], u24[2::3] = u32[0::4], u32[1::4], u32[2::4]
    rows, best = [], None
    for encoding, payload in [(0, u32), (1, bytes(u24))]:
        raw = envelope(payload, encoding, metadata)
        start = time.monotonic()
        compressed = gzip.compress(raw, compresslevel=9, mtime=0)
        assert gzip.decompress(compressed) == raw
        row = dict(encoding=encoding, codec='gzip', bytes=len(compressed),
                   seconds=round(time.monotonic() - start, 3))
        rows.append(row)
        if best is None or len(compressed) < len(best[0]):
            best = compressed, raw, encoding
        if args.compare:
            for codec, command in [
                ('brotli', ['brotli', '-q', '11', '-c']),
                ('zstd', ['zstd', '-q', '-19', '-c']),
                ('xz', ['xz', '-9', '-c']),
            ]:
                try:
                    start = time.monotonic()
                    alternative = subprocess.run(command, input=raw,
                        stdout=subprocess.PIPE, check=True).stdout
                    restored = subprocess.run([codec, '-d', '-c'], input=alternative,
                        stdout=subprocess.PIPE, check=True).stdout
                    assert restored == raw
                    rows.append(dict(encoding=encoding, codec=codec,
                        bytes=len(alternative), seconds=round(time.monotonic() - start, 3)))
                except FileNotFoundError:
                    pass
    compressed, raw, encoding = best
    sha = lambda value: hashlib.sha256(value).hexdigest()
    manifest = dict(schema=1, algorithm_identity=metadata['algorithmIdentity'],
        asset=args.output.name, compression='gzip', compressed_bytes=len(compressed),
        compressed_sha256=sha(compressed), uncompressed_bytes=len(raw),
        uncompressed_sha256=sha(raw))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.output.with_name(args.output.name + '.partial')
    temporary.write_bytes(compressed)
    # Do not replace an existing good asset until the staged bytes pass the
    # current compiled core's validation as well as all independent checksums.
    validate_asset(args.output.parent.parent, asset_path=temporary, manifest=manifest)
    manifest_path = args.output.with_name('rgb24-lut.json')
    manifest_temporary = manifest_path.with_name(manifest_path.name + '.partial')
    manifest_temporary.write_text(json.dumps(manifest, indent=2) + '\n')
    temporary.replace(args.output)
    manifest_temporary.replace(manifest_path)
    report = dict(domain=DOMAIN, generator_identity=generator_identity,
        raw_u32_bytes=len(u32), raw_u32_sha256=sha(u32), encoding=encoding,
        manifest=manifest, comparisons=rows)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
