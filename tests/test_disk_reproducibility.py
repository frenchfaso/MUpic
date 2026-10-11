"""Exercise actual mtools output, including source time and ordering changes."""
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from package_disk import build_image
from stage_media import disk_regions


@unittest.skipUnless(shutil.which("mformat") and shutil.which("mcopy"), "requires mtools")
class DiskReproducibility(unittest.TestCase):
    def test_movie_stream_extent_is_distinct_from_header_probes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            movie = root / "movie"
            header = bytearray(256)
            header[:4] = b"MUP9"
            header[56:60] = (65536).to_bytes(4, "little")
            movie.write_bytes(header + bytes(65536 - 256) + bytes([42]) * 262144)
            image = root / "demo.img"
            build_image(image, {"MOVIE.MUP": movie}, 64)
            region, = disk_regions(image)
            self.assertEqual(region["streamStart"], region["start"] + 65536)
            self.assertEqual(region["end"] - region["start"], movie.stat().st_size)

    def test_identical_files_produce_identical_disk_and_file_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            player, movie = root / "player", root / "movie"
            player.write_bytes(bytes(range(256)) * 3)
            movie.write_bytes(bytes(range(255, -1, -1)) * 4096)
            os.utime(player, (946684800, 946684800))
            os.utime(movie, (946684800, 946684800))
            first = build_image(root / "first.img", {"MUPIC.PRG": player, "MOVIE.MUP": movie}, 64)
            os.utime(player, (1767225598, 1767225598))
            os.utime(movie, (1767225598, 1767225598))
            second = build_image(root / "second.img", {"MOVIE.MUP": movie, "MUPIC.PRG": player}, 64)
            self.assertEqual(first["image_sha256"], second["image_sha256"])
            self.assertEqual(first["files"], second["files"])


if __name__ == "__main__":
    unittest.main()
