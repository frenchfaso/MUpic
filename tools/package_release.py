#!/usr/bin/env python3
"""Package native CLIs, test the extracted files, and collect a tagged release."""
import argparse
import gzip
import hashlib
import io
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = ('linux-x64', 'macos-arm64', 'macos-x64', 'windows-x64')
NOTICES = {'LICENSE': 'LICENSE', 'COPYING.LGPL-2.1': 'encoder/COPYING.LGPL-2.1',
           'rust-dependencies.txt': 'site/licenses/rust-dependencies.txt'}


def archive_name(version, platform):
    suffix = '.zip' if platform == 'windows-x64' else '.tar.gz'
    return f'MUpic-{version}-{platform}{suffix}'


def validate_identity(version, commit, published=False):
    pattern = r'v\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?'
    if not published:
        pattern = f'(?:{pattern}|test-[0-9A-Za-z-]+)'
    if not re.fullmatch(pattern, version) or not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Expected a vMAJOR.MINOR.PATCH tag (optional prerelease) and full commit SHA')


def package(binary, platform, version, commit, destination):
    validate_identity(version, commit)
    executable = 'mupic-encoder' + ('.exe' if platform == 'windows-x64' else '')
    files = {executable: binary.read_bytes()}
    files.update({name: (ROOT / source).read_bytes() for name, source in NOTICES.items()})
    # Ship notices from the actual compiler used for this binary.
    sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
    files['rust-standard-library.html'] = (sysroot / 'share/doc/rust/COPYRIGHT-library.html').read_bytes()
    source_url = f'https://github.com/frenchfaso/MUpic/tree/{commit}'
    credits = (ROOT / 'CREDITS.md').read_text(encoding='utf-8')
    credits = re.sub(r'\]\((?![a-z]+:|#)([^)]+)\)',
                     lambda m: f']({source_url}/{m[1]})', credits)
    files['CREDITS.md'] = credits.encode()
    command = '.\\mupic-encoder.exe' if platform == 'windows-x64' else './mupic-encoder'
    files['README.txt'] = f'''MUpic {version} - {platform}
16 colors is all you need!

Install FFmpeg separately: https://ffmpeg.org/download.html
Both ffmpeg and ffprobe must be on PATH. Rust is not required.
The complete color lookup table is included and cached locally on first use.

{command} input.mp4 -o movie.mup
{command} --help

Defaults: PAL, HD, PCM16. Both PCM16 and PCM8 HQ keep the source sample rate.
Match --system pal|ntsc and --video-output hd|sd to your Ultimate.
Linux: x64 with glibc 2.35 or newer. macOS: 11 or newer, matching architecture.
Windows: x64; the C runtime is statically linked. FFmpeg remains separate.

Player, instructions and credits: {source_url}
Corresponding source: https://github.com/frenchfaso/MUpic/archive/{commit}.tar.gz
Original code: GPL-3.0-only; component notices are included in this package.
'''.encode()
    destination.mkdir(parents=True, exist_ok=True)
    archive = destination / archive_name(version, platform)
    if archive.exists():
        raise FileExistsError(archive)
    if platform == 'windows-x64':
        with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED) as output:
            for name, data in sorted(files.items()):
                entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                entry.compress_type = zipfile.ZIP_DEFLATED
                entry.external_attr = (0o100755 if name == executable else 0o100644) << 16
                output.writestr(entry, data)
    else:
        with archive.open('xb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', filename='', mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode='w') as output:
                for name, data in sorted(files.items()):
                    entry = tarfile.TarInfo(name)
                    entry.size = len(data)
                    entry.mode = 0o755 if name == executable else 0o644
                    output.addfile(entry, io.BytesIO(data))
    return archive, files, executable


def verify_package(archive, files, executable):
    with tempfile.TemporaryDirectory(prefix='mupic release ') as temporary:
        directory = Path(temporary)
        if archive.suffix == '.zip':
            with zipfile.ZipFile(archive) as source:
                source.extractall(directory)
        else:
            with tarfile.open(archive) as source:
                source.extractall(directory, filter='data')
        assert {p.name for p in directory.iterdir()} == set(files)
        for name, data in files.items():
            assert (directory / name).read_bytes() == data, name
        binary = directory / executable
        subprocess.run([str(binary), '--help'], check=True)
        subprocess.run([str(binary), '--prepare-lut', '--threads', '2',
                        '--cache-dir', str(directory / 'cache')], check=True)
        for test in ('cli-wasm-parity.cjs', 'pcm8-parity.cjs', 'cli-failures.cjs'):
            subprocess.run(['node', str(ROOT / 'encoder/tests' / test), str(binary)], check=True)
    print(f'Verified extracted package: {archive.name}')


def collect(directory, version, commit):
    validate_identity(version, commit, published=True)
    expected = {'MUPIC.PRG', *(archive_name(version, p) for p in PLATFORMS)}
    if {p.name for p in directory.iterdir()} != expected:
        raise ValueError('Release must contain exactly the player and all four CLI archives')
    checksums = ''.join(f'{hashlib.sha256((directory / name).read_bytes()).hexdigest()}  {name}\n'
                        for name in sorted(expected))
    (directory / 'SHA256SUMS').write_text(checksums, encoding='utf-8')
    notes = f'''Player and native encoders, built and tested from `{commit}`.

- **C64 Ultimate:** download `MUPIC.PRG` and copy it beside your MUP files on a FAT32 USB stick.
- **Linux / Mac / Windows:** download the archive for your system and extract it. Install [FFmpeg](https://ffmpeg.org/download.html) separately, with `ffmpeg` and `ffprobe` on PATH.
- The full color lookup table is included. No Rust installation is needed.
- Checksums: `SHA256SUMS`. Instructions and component licenses are inside each CLI archive.

[Setup and usage](https://github.com/frenchfaso/MUpic/blob/{commit}/README.md) · [Credits](https://github.com/frenchfaso/MUpic/blob/{commit}/CREDITS.md) · [GPL-3.0-only](https://github.com/frenchfaso/MUpic/blob/{commit}/LICENSE) · [Corresponding source](https://github.com/frenchfaso/MUpic/archive/{commit}.tar.gz)
'''
    (directory.parent / 'release-notes.md').write_text(notes, encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--platform', choices=PLATFORMS)
    parser.add_argument('--version', required=True)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--output', type=Path, default=ROOT / 'build/release')
    parser.add_argument('--collect', action='store_true')
    args = parser.parse_args()
    if args.collect:
        collect(args.output, args.version, args.commit)
    elif args.binary and args.platform:
        verify_package(*package(args.binary, args.platform, args.version, args.commit, args.output))
    else:
        parser.error('Provide --binary and --platform, or --collect')


if __name__ == '__main__':
    main()
