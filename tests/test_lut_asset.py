"""Validate complete synthetic LUT assets through the production WASM importer."""
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import package_lut

DOMAIN = 1 << 24
CANDIDATE_COUNT = 99384  # Fixture for the repository's current 16-color palette.
ZERO_CHUNK = bytes(1 << 20)


@unittest.skipUnless(shutil.which("node"), "requires Node.js")
class LutAssetValidation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        script = """
import {readFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
const [adapter, wasm] = process.argv.slice(1);
const {EncoderCore} = await import(pathToFileURL(adapter).href);
const {instance} = await WebAssembly.instantiate(readFileSync(wasm), {});
const core = new EncoderCore(instance.exports);
console.log(JSON.stringify([core.lutIdentity(), core.lutPatternFingerprint()]));
"""
        cls.identity, cls.pattern = json.loads(subprocess.check_output([
            shutil.which("node"), "--input-type=module", "--eval", script,
            str(ROOT / "site/wasm-core.mjs"), str(ROOT / "site/assets/encoder.wasm")],
            text=True))

    def setUp(self):
        (ROOT / "build").mkdir(exist_ok=True)
        directory = tempfile.TemporaryDirectory(prefix="lut-asset-test-", dir=ROOT / "build")
        self.addCleanup(directory.cleanup)
        self.site = Path(directory.name)
        (self.site / "assets").mkdir()
        shutil.copyfile(ROOT / "site/assets/encoder.wasm", self.site / "assets/encoder.wasm")

    def asset(self, *, candidate_count=CANDIDATE_COUNT, pattern=None, first_plan=0):
        # Stream the full 64 MiB domain; never retain its plaintext in Python.
        def chunks():
            yield struct.pack("<I", first_plan) + ZERO_CHUNK[4:]
            for _ in range(DOMAIN * 4 // len(ZERO_CHUNK) - 1):
                yield ZERO_CHUNK

        payload_hash = hashlib.sha256()
        for chunk in chunks():
            payload_hash.update(chunk)
        header = (b"MUPLUT1\0" + struct.pack("<IB3xIIQ", 1, 0, DOMAIN,
                  candidate_count, DOMAIN * 4) + bytes.fromhex(self.identity)
                  + bytes.fromhex(pattern or self.pattern) + payload_hash.digest())
        full_hash = hashlib.sha256(header)
        path = self.site / "assets/rgb24-lut.bin.gz"
        with path.open("wb") as output, gzip.GzipFile(fileobj=output, mode="wb",
                compresslevel=1, mtime=0) as compressed:
            compressed.write(header)
            for chunk in chunks():
                compressed.write(chunk)
                full_hash.update(chunk)
        return dict(schema=1, compression="gzip", asset=path.name,
            algorithm_identity=self.identity, compressed_bytes=path.stat().st_size,
            compressed_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
            uncompressed_bytes=128 + DOMAIN * 4, uncompressed_sha256=full_hash.hexdigest())

    def rejected_by_core(self, manifest, message):
        run = subprocess.run

        def capture(*args, **kwargs):
            return run(*args, **kwargs, stderr=subprocess.PIPE)

        with mock.patch.object(package_lut.subprocess, "run", side_effect=capture):
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                package_lut.validate_asset(self.site, manifest=manifest)
        self.assertIn(message, caught.exception.stderr.decode())

    def test_complete_zero_table_is_valid(self):
        manifest = self.asset()
        self.assertEqual(package_lut.validate_asset(self.site, manifest=manifest), manifest)

    def test_incompatible_candidate_metadata_with_valid_checksums_is_rejected(self):
        changed_pattern = bytes([int(self.pattern[:2], 16) ^ 1]).hex() + self.pattern[2:]
        for changes in ({"candidate_count": CANDIDATE_COUNT + 1}, {"pattern": changed_pattern}):
            with self.subTest(changes=changes):
                self.rejected_by_core(self.asset(**changes), "Stale or incompatible LUT asset header")

    def test_out_of_range_plan_with_valid_checksums_is_rejected(self):
        self.rejected_by_core(self.asset(first_plan=CANDIDATE_COUNT), "Invalid LUT asset candidate id")


if __name__ == "__main__":
    unittest.main()
