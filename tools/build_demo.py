#!/usr/bin/env python3
"""Verify the Git LFS movies, build a FAT32 disk, and stage compressed web blocks."""
import argparse, gzip, hashlib, json, re, subprocess, sys
from pathlib import Path
from package_disk import build_image, sha256_file
from package_lut import validate_asset
ROOT = Path(__file__).resolve().parent.parent

def version_browser_entrypoints(site):
    """Refresh entrypoints and their workers whenever a deployed module changes."""
    # A module worker resolves its own imports independently of the page URL.
    # Hash dependencies before their importers so cached workers cannot combine
    # a previous adapter with a new raw-WASM ABI.
    for parent, child in [
        ('worker.js', 'remote-disk.js'),
        ('worker.js', 'emulated-keys.mjs'),
        ('app.js', 'mobile-keyboard.mjs'),
        ('worker-pool.mjs', 'lut-cache.mjs'),
        ('quantizer-worker.mjs', 'wasm-core.mjs'),
        ('encoder-worker.mjs', 'wasm-core.mjs'),
        ('converter.mjs', 'worker-pool.mjs'),
        ('converter.mjs', 'media.mjs'),
        ('converter.mjs', 'conversion-progress.mjs'),
        ('preview-player.mjs', 'media.mjs'),
        ('converter.mjs', 'preview-player.mjs'),
    ]:
        path = site/parent
        version = hashlib.sha256((site/child).read_bytes()).hexdigest()[:16]
        text, count = re.subn(r"(from\s+['\"]\./"+re.escape(child)+r")(?:\?[^'\"]*)?(['\"])",
                             lambda match: match[1]+'?v='+version+match[2], path.read_text())
        if count != 1:
            raise ValueError(f'Expected one {child} import in {parent}')
        path.write_text(text)
    digest = hashlib.sha256()
    for source in sorted(site.rglob('*')):
        if source.suffix in {'.js', '.mjs', '.wasm'} and source.is_file():
            digest.update(source.relative_to(site).as_posix().encode()+b'\0')
            digest.update(source.read_bytes())
    version = digest.hexdigest()[:16]
    for name, module in [('index.html', 'app.js'), ('convert.html', 'converter.mjs')]:
        page = site/name
        text, count = re.subn(r'(src="(?:\./)?'+re.escape(module)+r')(?:\?[^" ]*)?(")',
                             lambda match: match[1]+'?v='+version+match[2], page.read_text())
        if count != 1:
            raise ValueError(f'Expected one {module} entrypoint')
        for stylesheet in ['style.css', 'mupic.css', 'converter.css']:
            if stylesheet == 'converter.css' and name != 'convert.html':
                continue
            css_version = hashlib.sha256((site/stylesheet).read_bytes()).hexdigest()[:16]
            text, count = re.subn(r'(href="(?:\./)?'+re.escape(stylesheet)+r')(?:\?[^" ]*)?(")',
                                 lambda match: match[1]+'?v='+css_version+match[2], text)
            if count != 1:
                raise ValueError(f'Expected one {stylesheet} link in {name}')
        page.write_text(text)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'build'/'MUpic-demo.img')
    parser.add_argument('--site', type=Path, default=ROOT/'site')
    args = parser.parse_args()
    validate_asset(args.site)
    specs = json.loads((ROOT/'media'/'checksums.json').read_text())['files']
    files = {'MUPIC.PRG': ROOT/'releases'/'MUPIC.PRG'}
    # The emulator boots the registered PRG before browsing the USB image.
    # Keep that copy and its integrity metadata aligned with the USB player.
    manifest_path = args.site/'manifest.json'
    manifest = json.loads(manifest_path.read_text())
    player_asset = next(asset for asset in manifest['mupic']['assets'] if asset['path']=='/mupic.prg')
    player_copy = args.site/'data'/'mupic.prg'
    player_copy.write_bytes(files['MUPIC.PRG'].read_bytes())
    player_asset.update(bytes=player_copy.stat().st_size, sha256=sha256_file(player_copy))
    manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
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
    version_browser_entrypoints(args.site)

if __name__ == '__main__':
    main()
