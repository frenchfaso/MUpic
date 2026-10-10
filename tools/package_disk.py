#!/usr/bin/env python3
"""Build and independently read back a regular-file MBR/FAT32 USB image.

Uses mtools rather than mounting an image or touching a block device.  The
result may be passed directly to ue2emu run --usb IMAGE, or its files copied
onto a FAT32 USB stick for an Ultimate.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess

SECTOR = 512
PARTITION_LBA = 2048


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def _checksum(short_name: bytes) -> int:
    value = 0
    for byte in short_name:
        value = (((value & 1) << 7) + (value >> 1) + byte) & 255
    return value


def audit_image(image: Path, expected: dict[str, Path] | None = None) -> dict:
    """Read FAT structures and file chains independently of mtools."""
    size = image.stat().st_size
    with image.open("rb") as disk:
        def read_at(offset: int, length: int) -> bytes:
            if offset < 0 or offset + length > size:
                raise ValueError("read outside image")
            disk.seek(offset)
            result = disk.read(length)
            if len(result) != length:
                raise ValueError("truncated image")
            return result

        mbr = read_at(0, SECTOR)
        if mbr[510:] != b"\x55\xaa" or mbr[450] not in (11, 12):
            raise ValueError("expected MBR with FAT32 first partition")
        start_lba, sectors = struct.unpack_from("<II", mbr, 454)
        start, partition_bytes = start_lba * SECTOR, sectors * SECTOR
        if start_lba != PARTITION_LBA or not sectors or start + partition_bytes != size:
            raise ValueError("partition bounds do not match image")
        boot = read_at(start, SECTOR)
        bytes_per_sector = struct.unpack_from("<H", boot, 11)[0]
        cluster_sectors = boot[13]
        reserved = struct.unpack_from("<H", boot, 14)[0]
        fats = boot[16]
        total = struct.unpack_from("<I", boot, 32)[0]
        fat_sectors = struct.unpack_from("<I", boot, 36)[0]
        root = struct.unpack_from("<I", boot, 44)[0]
        hidden = struct.unpack_from("<I", boot, 28)[0]
        if (boot[510:] != b"\x55\xaa" or bytes_per_sector != SECTOR
                or not cluster_sectors or cluster_sectors & (cluster_sectors - 1)
                or cluster_sectors > 128 or reserved < 3 or fats != 2
                or total != sectors or hidden != start_lba or not fat_sectors
                or struct.unpack_from("<H", boot, 17)[0]
                or struct.unpack_from("<H", boot, 22)[0]):
            raise ValueError("invalid FAT32 BPB")
        data_sectors = sectors - reserved - fats * fat_sectors
        cluster_count = data_sectors // cluster_sectors
        if cluster_count < 65525 or fat_sectors * SECTOR // 4 < cluster_count + 2:
            raise ValueError("not a FAT32 cluster layout")
        info_sector, backup_sector = struct.unpack_from("<HH", boot, 48)
        if not 0 < info_sector < reserved or not 0 < backup_sector < reserved:
            raise ValueError("invalid FAT32 metadata sector offsets")
        info = read_at(start + info_sector * SECTOR, SECTOR)
        if info[:4] != b"RRaA" or info[484:488] != b"rrAa" or info[508:] != b"\0\0\x55\xaa":
            raise ValueError("invalid FAT32 FSInfo signatures")
        backup = read_at(start + backup_sector * SECTOR, SECTOR)
        if backup != boot:
            raise ValueError("backup boot sector differs")
        fat = read_at(start + reserved * SECTOR, fat_sectors * SECTOR)
        if fat != read_at(start + (reserved + fat_sectors) * SECTOR, len(fat)):
            raise ValueError("FAT copies differ")
        data_start = start + (reserved + fats * fat_sectors) * SECTOR
        cluster_bytes = cluster_sectors * SECTOR
        owned: set[int] = set()

        def chain(first: int) -> list[int]:
            clusters = []
            cluster = first
            while True:
                if not 2 <= cluster < cluster_count + 2 or cluster in owned:
                    raise ValueError("invalid, cyclic or overlapping cluster chain")
                owned.add(cluster)
                clusters.append(cluster)
                cluster = struct.unpack_from("<I", fat, cluster * 4)[0] & 0x0FFFFFFF
                if cluster >= 0x0FFFFFF8:
                    return clusters
                if cluster < 2 or cluster == 0x0FFFFFF7:
                    raise ValueError("unterminated cluster chain")

        def cluster_data(cluster: int) -> bytes:
            return read_at(data_start + (cluster - 2) * cluster_bytes, cluster_bytes)

        directory = b"".join(cluster_data(c) for c in chain(root))
        files = {}
        long_parts: dict[int, bytes] = {}
        long_count = 0
        long_checksum = None
        for position in range(0, len(directory), 32):
            entry = directory[position:position + 32]
            if entry[0] == 0:
                break
            if entry[0] == 229:
                long_parts.clear()
                long_count = 0
                continue
            if entry[11] == 15:
                sequence = entry[0] & 31
                if entry[0] & 64:
                    long_parts.clear()
                    long_count = sequence
                    long_checksum = entry[13]
                if not 1 <= sequence <= long_count or entry[13] != long_checksum:
                    raise ValueError("invalid long filename sequence")
                long_parts[sequence] = entry[1:11] + entry[14:26] + entry[28:32]
                continue
            if entry[11] & 8:
                long_parts.clear()
                long_count = 0
                continue
            if entry[11] & 16:
                raise ValueError("unexpected directory in flat distribution")
            if long_parts:
                if set(long_parts) != set(range(1, long_count + 1)) or long_checksum != _checksum(entry[:11]):
                    raise ValueError("invalid long filename checksum")
                encoded = b"".join(long_parts[i] for i in range(1, long_count + 1))
                units = struct.unpack("<" + "H" * (len(encoded) // 2), encoded)
                characters = []
                for unit in units:
                    if unit in (0, 65535):
                        break
                    characters.append(unit)
                name = struct.pack("<" + "H" * len(characters), *characters).decode("utf-16le")
            else:
                base, extension = entry[:8].decode("ascii").rstrip(), entry[8:11].decode("ascii").rstrip()
                name = base + ("." + extension if extension else "")
                if entry[12] & 8:
                    base = base.lower()
                if entry[12] & 16:
                    extension = extension.lower()
                name = base + ("." + extension if extension else "")
            long_parts.clear()
            long_count = 0
            if name.casefold() in {n.casefold() for n in files}:
                raise ValueError("duplicate filename")
            first = (struct.unpack_from("<H", entry, 20)[0] << 16) | struct.unpack_from("<H", entry, 26)[0]
            length = struct.unpack_from("<I", entry, 28)[0]
            clusters = chain(first) if first else []
            if len(clusters) != math.ceil(length / cluster_bytes):
                raise ValueError("file chain length differs from file size")
            remaining = length
            digest = hashlib.sha256()
            for cluster in clusters:
                data = cluster_data(cluster)[:remaining]
                digest.update(data)
                remaining -= len(data)
            files[name] = {"bytes": length, "sha256": digest.hexdigest(), "clusters": len(clusters)}
        allocated = {cluster for cluster in range(2, cluster_count + 2)
                     if struct.unpack_from("<I", fat, cluster * 4)[0] & 0x0FFFFFFF}
        if allocated != owned:
            raise ValueError("unreferenced allocated clusters")
        free, next_free = struct.unpack_from("<II", info, 488)
        if free not in (0xFFFFFFFF, cluster_count - len(owned)):
            raise ValueError("FSInfo free cluster count differs")
        if next_free != 0xFFFFFFFF and not 2 <= next_free < cluster_count + 2:
            raise ValueError("invalid FSInfo next-free cluster")
        if expected is not None:
            if set(files) != set(expected):
                raise ValueError(f"directory mismatch: {set(files)} versus {set(expected)}")
            for name, source in expected.items():
                if files[name]["bytes"] != source.stat().st_size or files[name]["sha256"] != sha256_file(source):
                    raise ValueError(f"image content differs: {name}")
        return {"status": "passed", "image": str(image.resolve()), "image_bytes": size,
                "partition_start_lba": start_lba, "partition_sectors": sectors,
                "cluster_bytes": cluster_bytes, "fat_cluster_count": cluster_count,
                "files": files, "allocated_bytes": image.stat().st_blocks * SECTOR}


def build_image(image: Path, files: dict[str, Path], size_mib: int = 512) -> dict:
    if size_mib < 64 or size_mib > 2048 * 1024 or not files:
        raise ValueError("require files and FAT32 size from 64 MiB to 2 TiB")
    checked = {}
    for name, source in files.items():
        if not name or name in (".", "..") or any(c in name for c in '/\\:*?"<>|\x00'):
            raise ValueError("file names must be FAT basenames")
        if any(name.casefold() == old.casefold() for old in checked):
            raise ValueError("duplicate filename")
        source = source.resolve(strict=True)
        if not source.is_file() or source.stat().st_size >= 1 << 32:
            raise ValueError("FAT files must be regular and smaller than 4 GiB")
        checked[name] = source
    image = image.absolute()
    if image.exists() or image.is_symlink():
        raise FileExistsError(f"refusing to replace {image}")
    image_bytes = size_mib << 20
    if sum(s.stat().st_size for s in checked.values()) > (image_bytes - (PARTITION_LBA * SECTOR)) * 97 // 100:
        raise ValueError("image too small for files and FAT metadata")
    mformat, mcopy = shutil.which("mformat"), shutil.which("mcopy")
    if not mformat or not mcopy:
        raise RuntimeError("mtools mformat and mcopy are required")
    image.parent.mkdir(parents=True, exist_ok=True)
    with image.open("xb") as disk:
        disk.truncate(image_bytes)
        mbr = bytearray(SECTOR)
        mbr[446:454] = bytes.fromhex("00feffff0cfeffff")
        struct.pack_into("<II", mbr, 454, PARTITION_LBA, image_bytes // SECTOR - PARTITION_LBA)
        mbr[510:] = b"\x55\xaa"
        disk.write(mbr)
    volume = f"{image}@@{PARTITION_LBA * SECTOR}"
    cluster_sectors = 1
    partition_bytes = image_bytes - PARTITION_LBA * SECTOR
    while cluster_sectors < 8 and partition_bytes // (cluster_sectors * 2 * SECTOR) >= 70000:
        cluster_sectors *= 2
    subprocess.run([mformat, "-i", volume, "-F", "-T", str(image_bytes // SECTOR - PARTITION_LBA),
                    "-H", str(PARTITION_LBA), "-c", str(cluster_sectors), "-v", "MUPIC", "::"], check=True)
    for name, source in checked.items():
        subprocess.run([mcopy, "-i", volume, str(source), "::/" + name], check=True)
    report = audit_image(image, checked)
    report["source_files"] = {name: str(source) for name, source in checked.items()}
    report["image_sha256"] = sha256_file(image)
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--size-mib", type=int, default=512)
    parser.add_argument("--report", type=Path)
    parser.add_argument("files", nargs="+", help="FAT_NAME=source_path")
    args = parser.parse_args()
    files = {}
    for argument in args.files:
        name, separator, path = argument.partition("=")
        if not separator or name in files:
            parser.error("each file needs a unique FAT_NAME=source_path")
        files[name] = Path(path)
    report = build_image(args.output, files, args.size_mib)
    report_path = args.report or args.output.with_suffix(args.output.suffix + ".json")
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
