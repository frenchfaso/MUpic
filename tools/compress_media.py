#!/usr/bin/env python3
"""Prepare lossless, reproducible Git LFS deployment inputs for the demo."""
import argparse, gzip, hashlib, json, shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(1<<20),b''):h.update(b)
    return h.hexdigest()
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source_directory',type=Path)
    args=parser.parse_args()
    manifest=ROOT/'media/checksums.json';data=json.loads(manifest.read_text())
    for name,spec in data['files'].items():
        source=args.source_directory/name
        if source.stat().st_size!=spec['bytes'] or digest(source)!=spec['sha256']:
            raise ValueError(f'Original checksum differs: {name}')
        target=ROOT/'media'/(name+'.gz')
        if target.exists():raise FileExistsError(target)
        with source.open('rb') as src,target.open('xb') as raw:
            with gzip.GzipFile(filename='',fileobj=raw,mode='wb',compresslevel=9,mtime=0) as dest:
                shutil.copyfileobj(src,dest,1<<20)
        h=hashlib.sha256();total=0
        with gzip.open(target,'rb') as f:
            for b in iter(lambda:f.read(1<<20),b''):
                total+=len(b);h.update(b)
        if total!=spec['bytes'] or h.hexdigest()!=spec['sha256']:raise ValueError(f'Lossless round-trip failed: {name}')
        spec.update(compressed_bytes=target.stat().st_size,compressed_sha256=digest(target))
        print(json.dumps({'file':target.name,'bytes':spec['compressed_bytes'],'original_bytes':spec['bytes'],'lossless_roundtrip':'passed'}),flush=True)
    manifest.write_text(json.dumps(data,indent=2)+'\n')
if __name__=='__main__':main()
