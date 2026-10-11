#!/usr/bin/env python3
"""Split a qualified FAT32 disk into immutable, hash-checked GitHub Pages blocks."""
import argparse,gzip,hashlib,json,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
CHUNK=262144

def digest(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for b in iter(lambda:f.read(1<<20),b''):h.update(b)
    return h.hexdigest()

def disk_regions(image):
    # Only contiguous file extents qualify for sequential read-ahead. The FAT
    # image itself remains authoritative, including fragmented file support.
    with image.open("rb") as f:
        f.seek(454); start = struct.unpack("<I", f.read(4))[0] * 512
        f.seek(start); boot = f.read(512)
        reserved = struct.unpack_from("<H", boot, 14)[0]
        fat_sectors = struct.unpack_from("<I", boot, 36)[0]
        cluster_bytes = boot[13] * 512
        data = start + (reserved + boot[16] * fat_sectors) * 512
        f.seek(start + reserved * 512); fat = f.read(fat_sectors * 512)
        root = struct.unpack_from("<I", boot, 44)[0]
        f.seek(data + (root - 2) * cluster_bytes); directory = f.read(cluster_bytes)
        regions = []
        for at in range(0, len(directory), 32):
            entry = directory[at:at + 32]
            if entry[0] == 0: break
            if entry[0] == 0xe5 or entry[11] & 0x18 or entry[8:11] != b"MUP": continue
            cluster = (struct.unpack_from("<H", entry, 20)[0] << 16) | struct.unpack_from("<H", entry, 26)[0]
            size = struct.unpack_from("<I", entry, 28)[0]
            first = cluster; count = (size + cluster_bytes - 1) // cluster_bytes
            for i in range(count):
                if cluster != first + i: break
                cluster = struct.unpack_from("<I", fat, cluster * 4)[0] & 0xfffffff
            else:
                if cluster >= 0xffffff8:
                    offset = data + (first - 2) * cluster_bytes
                    region = {"name": entry[:8].decode("ascii").rstrip() + ".MUP", "start": offset, "end": offset + size}
                    f.seek(offset); header = f.read(256)
                    if len(header) == 256 and header[:4] == b"MUP9":
                        stream = struct.unpack_from("<I", header, 56)[0]
                        if 256 <= stream < size:
                            region["streamStart"] = offset + stream
                    regions.append(region)
        return regions

def main():
    a=argparse.ArgumentParser(description=__doc__)
    a.add_argument('image',type=Path)
    a.add_argument('--site',type=Path,default=ROOT/'site')
    a.add_argument('--expected-sha256',required=True)
    a.add_argument('--compression',choices=['none','gzip'],default='gzip')
    o=a.parse_args();image=o.image.resolve();site=o.site.resolve()
    sha=digest(image)
    if sha!=o.expected_sha256:raise ValueError('Demo disk failed SHA-256 verification')
    size=image.stat().st_size
    if size%512:raise ValueError('Disk must have whole sectors')
    dest=site/'media'/sha;dest.mkdir(parents=True,exist_ok=True)
    hashes=[];stored=[];total=0;inflated=0
    with image.open('rb') as f:
        for i in range((size+CHUNK-1)//CHUNK):
            b=f.read(CHUNK)
            if any(b):
                h=hashlib.sha256(b).hexdigest()
                payload=gzip.compress(b,compresslevel=9,mtime=0) if o.compression=='gzip' else b
                suffix='.bin.gz' if o.compression=='gzip' else '.bin'
                p=dest/f'{i:04d}{suffix}'
                if p.exists():
                    if p.read_bytes()!=payload:raise ValueError(f'Existing immutable chunk changed: {i}')
                else:p.write_bytes(payload)
                if o.compression=='gzip' and gzip.decompress(payload)!=b:raise ValueError(f'Gzip round-trip failed: {i}')
                hashes.append(h);stored.append(len(payload));total+=len(payload);inflated+=len(b)
            else:hashes.append(None);stored.append(0)
    p=site/'manifest.json';m=json.loads(p.read_text())
    m['mupic']['disk']=dict(url=f'media/{sha}/',bytes=size,sha256=sha,transport='chunks',chunks=hashes,compression=o.compression,storedBytes=stored,chunkBytes=CHUNK,cacheBytes=16<<20,regions=disk_regions(image))
    p.write_text(json.dumps(m,indent=2)+'\n')
    out=dict(schema='mupic-pages-media-v1',image_sha256=sha,disk_bytes=size,downloadable_chunk_bytes=total,uncompressed_nonzero_chunk_bytes=inflated,compression=o.compression,chunk_bytes=CHUNK,total_chunks=len(hashes),zero_chunks=hashes.count(None),movies=[r['name'] for r in m['mupic']['disk']['regions']])
    print(json.dumps(out,indent=2))
if __name__=='__main__':main()
