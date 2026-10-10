#!/usr/bin/env python3
"""Verify the Git LFS movies, build a FAT32 disk, and stage compressed web blocks."""
import argparse, gzip, hashlib, json, subprocess, sys
from pathlib import Path
from package_disk import build_image, sha256_file
ROOT = Path(__file__).resolve().parent.parent

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'build'/'MUpic-demo.img')
    parser.add_argument('--site', type=Path, default=ROOT/'site')
    args = parser.parse_args()
    specs = json.loads((ROOT/'media'/'checksums.json').read_text())['files']
    files = {'MUPIC.PRG': ROOT/'releases'/'MUPIC.PRG'}
    for name, spec in specs.items():
        packed = ROOT/'media'/(name+'.gz')
        if packed.stat().st_size != spec['compressed_bytes'] or sha256_file(packed) != spec['compressed_sha256']:
            raise ValueError(f'{packed.name} failed verification; run git lfs pull')
        source = ROOT/'build'/'expanded'/name
        source.parent.mkdir(parents=True,exist_ok=True)
        if not source.exists():
            try:
                total=0
                with gzip.open(packed,'rb') as stream, source.open('xb') as out:
                    while chunk:=stream.read(1<<20):
                        total+=len(chunk)
                        if total>spec['bytes']:raise ValueError(f'{name} exceeds declared length')
                        out.write(chunk)
            except BaseException:
                source.unlink(missing_ok=True)
                raise
        if source.stat().st_size != spec['bytes'] or sha256_file(source) != spec['sha256']:
            raise ValueError(f'{name} failed original-byte verification')
        files[name] = source
    report = build_image(args.output, files)
    (args.output.with_suffix('.audit.json')).write_text(json.dumps(report, indent=2)+'\n')
    subprocess.run([sys.executable, str(ROOT/'tools'/'stage_media.py'), str(args.output),
                    '--expected-sha256', report['image_sha256'], '--site', str(args.site),
                    '--compression', 'gzip'], check=True)

if __name__ == '__main__':
    main()
