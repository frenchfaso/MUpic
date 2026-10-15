"""Release packages contain only runnable files, instructions and notices."""
import hashlib
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('package_release', ROOT / 'tools/package_release.py')
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)
COMMIT = 'a' * 40


class ReleasePackages(unittest.TestCase):
    def test_only_version_tags_can_be_published(self):
        for version in ('v1.0.0', 'v1.2.3-rc.1'):
            release.validate_identity(version, COMMIT, published=True)
        release.validate_identity('test-123', COMMIT)
        for version in ('test-123', 'main', 'v1', '../v1.0.0', 'v1.0.0;echo unsafe'):
            with self.assertRaises(ValueError):
                release.validate_identity(version, COMMIT, published=True)
        with self.assertRaises(ValueError):
            release.validate_identity('v1.0.0', 'not-a-commit', published=True)

    def test_archives_preserve_binary_permissions_and_required_notices(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            copyright_file = root / 'share/doc/rust/COPYRIGHT-library.html'
            copyright_file.parent.mkdir(parents=True)
            copyright_file.write_bytes(b'Compiler-specific runtime notices')
            binary = root / 'binary'
            binary.write_bytes(b'Exact compiled binary\0')
            for platform in release.PLATFORMS:
                with patch.object(release.subprocess, 'check_output', return_value=str(root)):
                    archive, files, executable = release.package(binary, platform, 'v1.0.0', COMMIT, root / 'out')
                self.assertEqual(set(files), {executable, 'README.txt', 'CREDITS.md', 'LICENSE',
                                             'COPYING.LGPL-2.1', 'rust-dependencies.txt',
                                             'rust-standard-library.html'})
                self.assertIn(b'Install FFmpeg separately', files['README.txt'])
                self.assertIn(COMMIT.encode(), files['README.txt'])
                self.assertEqual(files['rust-standard-library.html'], copyright_file.read_bytes())
                if archive.suffix == '.zip':
                    with zipfile.ZipFile(archive) as contents:
                        self.assertEqual(set(contents.namelist()), set(files))
                        self.assertEqual(contents.read(executable), binary.read_bytes())
                else:
                    with tarfile.open(archive) as contents:
                        self.assertEqual(set(contents.getnames()), set(files))
                        self.assertEqual(contents.getmember(executable).mode, 0o755)
                        self.assertEqual(contents.extractfile(executable).read(), binary.read_bytes())

    def test_incomplete_or_extra_assets_block_release_and_checksums_cover_every_asset(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / 'dist'
            directory.mkdir()
            with self.assertRaises(ValueError):
                release.collect(directory, 'v1.0.0', COMMIT)
            names = ['MUPIC.PRG', *(release.archive_name('v1.0.0', p) for p in release.PLATFORMS)]
            for name in names:
                (directory / name).write_bytes(name.encode())
            (directory / 'ffmpeg.exe').write_bytes(b'Not distributed')
            with self.assertRaises(ValueError):
                release.collect(directory, 'v1.0.0', COMMIT)
            (directory / 'ffmpeg.exe').unlink()
            release.collect(directory, 'v1.0.0', COMMIT)
            expected = ''.join(f'{hashlib.sha256(name.encode()).hexdigest()}  {name}\n' for name in sorted(names))
            self.assertEqual((directory / 'SHA256SUMS').read_text(), expected)
            self.assertIn(COMMIT, (directory.parent / 'release-notes.md').read_text())


if __name__ == '__main__':
    unittest.main()
