#!/usr/bin/env python3
"""Package DICT4 profile13 assets into one self-contained MUP9 file.

Envelope layout is documented in container_format.json.  The codec8 cache
stream is copied exactly; only its startup metadata and initial frame move
into this envelope.  Audit reads bounded blocks and independently replays the
video, including when streaming encodes retain hashes instead of full oracles.
Codec8 retains the 190-line HDMI profile; codec9 carries the full 200-line SD
profile in the same envelope and memory map.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib

HEADER_BYTES = 256
BLOCK_BYTES = 262144
AUDIO_OFFSET = 225280
INITIAL_BYTES = 46080
TICKS_PER_BLOCK = 20
FRAME_TICKS = 3
PAL_HZ = 985248 / (63 * 312)

FIELDS = {
    "container_version": (4, 1), "codec_version": (5, 1), "profile": (6, 1), "flags": (7, 1),
    "header_bytes": (8, 2), "width": (10, 2), "height": (12, 2), "active_width": (14, 2),
    "active_height": (16, 2), "active_top": (18, 2), "frame_ticks": (20, 1),
    "video_standard": (21, 1), "prefetch_bytes_per_row": (22, 1),
    "record_count": (24, 4), "block_count": (28, 4), "audio_divider": (32, 2),
    "ticks_per_block": (34, 2), "audio_offset": (36, 4), "block_bytes": (40, 4),
    "audio_table_offset": (44, 4), "initial_offset": (48, 4), "initial_bytes": (52, 4),
    "stream_offset": (56, 4), "stream_bytes": (60, 4), "file_bytes": (64, 4),
    "source_frame_count": (68, 4), "last_block_ticks": (72, 2), "audio_sample_rate": (76, 4),
}
FIXED = {"container_version": 9, "codec_version": 8, "profile": 13, "header_bytes": 256,
         "width": 360, "height": 200, "active_width": 360, "active_height": 190,
         "active_top": 5, "frame_ticks": 3, "video_standard": 0, "prefetch_bytes_per_row": 13,
         "ticks_per_block": 20, "audio_offset": AUDIO_OFFSET, "block_bytes": BLOCK_BYTES,
         "audio_table_offset": 256, "initial_bytes": INITIAL_BYTES}
PROFILES = {
    8: FIXED,
    9: FIXED | {"codec_version": 9, "active_width": 356, "active_height": 200, "active_top": 0},
}


def profile(codec_version: int) -> dict:
    require(codec_version in PROFILES, "unsupported codec_version")
    return PROFILES[codec_version]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def align(value: int, boundary: int) -> int:
    return (value + boundary - 1) // boundary * boundary


def pcm_bytes_at_tick(tick: int, divider: int) -> int:
    numerator = tick * 6250000 * (63 * 312)
    denominator = (divider + 2) * 985248
    whole, remainder = divmod(numerator, denominator)
    rounded = whole + int(remainder * 2 > denominator or (remainder * 2 == denominator and whole % 2))
    return rounded * 4


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def _validate_abi() -> None:
    schema = json.loads(Path(__file__).with_name("container_format.json").read_text())
    documented = {field["name"]: (field["offset"], field["bytes"]) for field in schema["fields"]}
    require(schema["header_bytes"] == HEADER_BYTES and all(documented[k] == v for k, v in FIELDS.items()),
            "container implementation differs from canonical ABI")


def read_header(raw: bytes, actual_bytes: int) -> dict:
    require(len(raw) == HEADER_BYTES and raw[:4] == b"MUP9", "invalid MUP9 magic/header")
    require(zlib.crc32(raw[:224] + raw[228:]) == int.from_bytes(raw[224:228], "little"), "header CRC32 mismatch")
    header = {name: int.from_bytes(raw[offset:offset + length], "little")
              for name, (offset, length) in FIELDS.items()}
    for name, value in profile(header["codec_version"]).items():
        require(header[name] == value, f"unsupported {name}")
    require(not any(raw[23:24] + raw[74:76] + raw[80:96] + raw[228:]), "nonzero reserved header bytes")
    require(header["flags"] in (0, 1), "unknown flags")
    records, blocks = header["record_count"], header["block_count"]
    require(6 <= records <= 65535 and records % FRAME_TICKS == 0, "record count unsupported by current player")
    require(blocks == (records + TICKS_PER_BLOCK - 1) // TICKS_PER_BLOCK, "incorrect block count")
    require(header["source_frame_count"] == records // FRAME_TICKS, "incorrect source frame count")
    require(header["last_block_ticks"] == records - (blocks - 1) * TICKS_PER_BLOCK, "incorrect final block cadence")
    require(header["initial_offset"] == align(HEADER_BYTES + 3 * blocks, 256), "incorrect initial frame offset")
    require(header["stream_offset"] == align(header["initial_offset"] + INITIAL_BYTES, 65536), "incorrect stream offset")
    require(header["stream_bytes"] == blocks * BLOCK_BYTES, "incorrect stream size")
    require(header["file_bytes"] == header["stream_offset"] + header["stream_bytes"] == actual_bytes < 1 << 32,
            "incorrect file bounds")
    require(header["audio_divider"] > 0 and header["audio_sample_rate"] > 0, "invalid audio timing")
    title = raw[160:224]
    zero = title.find(b"\0")
    require(zero >= 0 and not any(title[zero:]), "invalid title padding")
    try:
        header["title"] = title[:zero].decode("ascii")
    except UnicodeDecodeError as error:
        raise ValueError("title must be ASCII") from error
    header["initial_sha256"] = raw[96:128].hex()
    header["stream_sha256"] = raw[128:160].hex()
    return header


def package(assets: Path, output: Path, title: str) -> dict:
    _validate_abi()
    assets = assets.resolve(strict=True)
    manifest = json.loads((assets / "manifest.json").read_text())
    mux = json.loads((assets / "cache-mux.json").read_text())
    selected = profile(manifest["span_version"])
    expected_left = (selected["width"] - selected["active_width"]) // 2
    require((manifest["span_version"], manifest["width"], manifest["height"], manifest["active_width"],
             manifest["active_height"], manifest["active_top"], manifest["active_left"], manifest["frame_ticks"],
             manifest["row_prefetch_bytes_per_line"]) == (selected["codec_version"], 360, 200,
             selected["active_width"], selected["active_height"], selected["active_top"], expected_left, 3, 13),
            "source is not a supported DICT4/profile13 geometry")
    if selected["codec_version"] == 9:
        require(manifest["display_pixel_aspect_ratio"] == 1, "SD profile requires square pixels")
    records = manifest["phases"]
    blocks = (records + TICKS_PER_BLOCK - 1) // TICKS_PER_BLOCK
    require(mux["block_bytes"] == BLOCK_BYTES and mux["audio_offset"] == AUDIO_OFFSET
            and mux["fixed_ticks_per_block"] == TICKS_PER_BLOCK, "unsupported source cache layout")
    require(len(mux["audio_lengths"]) == blocks and mux["file_bytes"] == blocks * BLOCK_BYTES,
            "inconsistent source cache metadata")
    require(manifest["source_frames"] * FRAME_TICKS == records, "source cadence mismatch")
    initial = assets / "initial.bin"
    stream = assets / mux["file"]
    require(stream.parent == assets and stream.is_file(), "stream must be a direct asset file")
    require(initial.stat().st_size == INITIAL_BYTES and stream.stat().st_size == blocks * BLOCK_BYTES,
            "source payload size mismatch")
    try:
        title_bytes = title.encode("ascii")
    except UnicodeEncodeError as error:
        raise ValueError("title must be ASCII") from error
    require(len(title_bytes) <= 63 and b"\0" not in title_bytes, "title must fit 63 ASCII bytes")
    header = bytearray(HEADER_BYTES)
    header[:4] = b"MUP9"
    initial_offset = align(HEADER_BYTES + 3 * blocks, 256)
    stream_offset = align(initial_offset + INITIAL_BYTES, 65536)
    values = selected | {"flags": int(manifest.get("lossy_rate_control", False)), "record_count": records,
                      "block_count": blocks, "audio_divider": mux["audio_divider"],
                      "initial_offset": initial_offset, "stream_offset": stream_offset,
                      "stream_bytes": blocks * BLOCK_BYTES, "file_bytes": stream_offset + blocks * BLOCK_BYTES,
                      "source_frame_count": manifest["source_frames"],
                      "last_block_ticks": records - (blocks - 1) * TICKS_PER_BLOCK,
                      "audio_sample_rate": manifest["audio_sample_rate"]}
    for name, value in values.items():
        offset, length = FIELDS[name]
        require(isinstance(value, int) and 0 <= value < 1 << (length * 8), f"field overflow: {name}")
        header[offset:offset + length] = value.to_bytes(length, "little")
    header[96:128] = bytes.fromhex(sha256_file(initial))
    header[128:160] = bytes.fromhex(sha256_file(stream))
    header[160:160 + len(title_bytes)] = title_bytes
    header[224:228] = zlib.crc32(header[:224] + header[228:]).to_bytes(4, "little")
    read_header(header, values["file_bytes"])
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as destination:
        destination.write(header)
        for length in mux["audio_lengths"]:
            require(0 <= length <= BLOCK_BYTES - AUDIO_OFFSET and length % 4 == 0, "invalid source audio length")
            destination.write(length.to_bytes(3, "little"))
        destination.seek(initial_offset)
        destination.write(initial.read_bytes())
        destination.seek(stream_offset)
        with stream.open("rb") as source:
            while chunk := source.read(1 << 20):
                destination.write(chunk)
    return audit(output, assets)


def audit(path: Path, assets: Path | None = None) -> dict:
    _validate_abi()
    path = path.resolve(strict=True)
    source_manifest = source_mux = None
    hash_oracles = None
    full_oracles_verified = []
    if assets is not None:
        assets = assets.resolve(strict=True)
        source_manifest = json.loads((assets / "manifest.json").read_text())
        source_mux = json.loads((assets / "cache-mux.json").read_text())
        hash_path = assets / "oracle-hashes.json"
        if hash_path.exists():
            hash_oracles = json.loads(hash_path.read_text())
    with path.open("rb") as source:
        header = read_header(source.read(HEADER_BYTES), path.stat().st_size)
        top = header["active_top"]
        rows = 200 if header["codec_version"] == 9 else 192
        words = rows // 4
        padding_start = top + header["active_height"]
        blocks, records = header["block_count"], header["record_count"]
        table = source.read(blocks * 3)
        require(len(table) == blocks * 3, "truncated audio table")
        audio_lengths = [int.from_bytes(table[i:i + 3], "little") for i in range(0, len(table), 3)]
        require(not any(source.read(header["initial_offset"] - source.tell())), "nonzero audio table padding")
        front = bytearray(source.read(INITIAL_BYTES))
        require(len(front) == INITIAL_BYTES and hashlib.sha256(front).hexdigest() == header["initial_sha256"],
                "initial frame hash mismatch")
        require(not any(source.read(header["stream_offset"] - source.tell())), "nonzero startup padding")
        for column in range(180):
            require(not any(front[column * 256:column * 256 + top])
                    and not any(front[column * 256 + padding_start:(column + 1) * 256]), "nonzero initial black rows/padding")
        if header["codec_version"] == 9:
            require(not any(front[:256] + front[-256:]), "nonzero SD side borders")
        logical_digest = hashlib.sha256(front)
        phase_digest = hashlib.sha256()
        stream_digest = hashlib.sha256()
        pcm_digest = hashlib.sha256()
        if hash_oracles:
            require(len(hash_oracles["logical_frame_sha256"]) == header["source_frame_count"]
                    and len(hash_oracles["phase_sha256"]) == records, "incorrect oracle hash counts")
            require(hashlib.sha256(front).hexdigest() == hash_oracles["logical_frame_sha256"][0], "initial oracle differs")
        initial_source = (assets / "initial.bin").open("rb") if assets else None
        pcm_source = (assets / "audio.pcm").open("rb") if assets else None
        stream_source = (assets / source_mux["file"]).open("rb") if assets else None
        try:
            if initial_source:
                require(initial_source.read() == front, "initial frame differs from source")
            cumulative_pcm = 0
            completed = columns = wire = record_index = total_ticks = 0
            previous_column = -1
            dictionary = None
            block_counts = []
            record_offsets = []
            for block_index in range(blocks):
                block = source.read(BLOCK_BYTES)
                require(len(block) == BLOCK_BYTES, "truncated stream block")
                stream_digest.update(block)
                if stream_source:
                    require(stream_source.read(BLOCK_BYTES) == block, f"stream differs in block {block_index}")
                block_ticks = min(TICKS_PER_BLOCK, records - record_index)
                block_counts.append(block_ticks)
                cursor = 0
                for _ in range(block_ticks):
                    phase_digest.update(front)
                    if hash_oracles:
                        require(hashlib.sha256(front).hexdigest() == hash_oracles["phase_sha256"][record_index],
                                f"phase oracle differs at {record_index}")
                    record_offsets.append(block_index * BLOCK_BYTES + cursor)
                    length = int.from_bytes(block[cursor:cursor + 2], "little")
                    end = cursor + 2 + length
                    require(length >= 2 and end + 2 <= AUDIO_OFFSET, f"invalid video record {record_index}")
                    payload = block[cursor + 2:end]
                    cursor = end
                    active = record_index < records - FRAME_TICKS
                    flags = (2 if active and record_index % FRAME_TICKS == 0 else 0) | (1 if active and record_index % FRAME_TICKS == 2 else 0)
                    require(payload[0] == flags and payload[1] <= 180, f"invalid record flags/count {record_index}")
                    at = 2
                    if flags & 2:
                        require(length >= 1026, "truncated dictionary")
                        planes = payload[2:1026]
                        require(all(planes[p * 256 + t] == 0 for p in range(4) for t in (254, 255)), "reserved dictionary entries differ")
                        dictionary = [bytes(planes[p * 256 + t] for p in range(4)) for t in range(254)]
                        previous_column = -1
                        at = 1026
                    for _ in range(payload[1]):
                        require(at + 2 <= length, "truncated column")
                        column, tag = payload[at:at + 2]
                        at += 2
                        require(previous_column < column < 180, "columns not strictly ascending")
                        previous_column = column
                        destination = column * 256 + top
                        if tag == 255:
                            require(at + rows <= length, "truncated RAW column")
                            front[destination:destination + rows] = payload[at:at + rows]
                            at += rows
                        else:
                            require(dictionary is not None and tag % 5 == 0 and tag <= words * 5 and at + words + tag <= length,
                                    "invalid dictionary column")
                            tokens = payload[at:at + words]
                            at += words
                            escapes = []
                            for word, token in enumerate(tokens):
                                row = word * 4
                                if token < 254:
                                    front[destination + row:destination + row + 4] = dictionary[token]
                                elif token == 255:
                                    escapes.append(row)
                            require(len(escapes) * 5 == tag, "incorrect escape count")
                            for row in escapes:
                                require(payload[at] == row, "incorrect escape row")
                                front[destination + row:destination + row + 4] = payload[at + 1:at + 5]
                                at += 5
                        require(not any(front[column * 256:column * 256 + top]) and
                                not any(front[column * 256 + padding_start:(column + 1) * 256]),
                                "black padding overwritten")
                        if header["codec_version"] == 9 and column in (0, 179):
                            require(not any(front[column * 256:(column + 1) * 256]), "SD side border overwritten")
                        columns += 1
                    require(at == length and (active or payload == b"\0\0"), "trailing bytes or invalid end hold")
                    if flags & 1:
                        completed += 1
                        logical_digest.update(front)
                        if hash_oracles:
                            require(hashlib.sha256(front).hexdigest() == hash_oracles["logical_frame_sha256"][completed],
                                    f"logical oracle differs at {completed}")
                    wire += length + 2
                    record_index += 1
                require(not any(block[cursor:AUDIO_OFFSET]), "nonzero video padding")
                total_ticks += block_ticks
                next_cumulative_pcm = pcm_bytes_at_tick(total_ticks, header["audio_divider"])
                pcm_length = audio_lengths[block_index]
                require(pcm_length == next_cumulative_pcm - cumulative_pcm and pcm_length <= BLOCK_BYTES - AUDIO_OFFSET,
                        "audio timing/table mismatch")
                cumulative_pcm = next_cumulative_pcm
                pcm = block[AUDIO_OFFSET:AUDIO_OFFSET + pcm_length]
                pcm_digest.update(pcm)
                if pcm_source:
                    require(pcm_source.read(pcm_length) == pcm, "PCM differs from source")
                require(not any(block[AUDIO_OFFSET + pcm_length:]), "nonzero PCM padding")
            require(record_index == records and completed == header["source_frame_count"] - 1, "incorrect frame totals")
            require(stream_digest.hexdigest() == header["stream_sha256"], "stream SHA256 mismatch")
            if source_mux:
                require(audio_lengths == source_mux["audio_lengths"] and block_counts == source_mux["block_records"]
                        and record_offsets == source_mux["record_offsets"], "source mux metadata differs")
                require(header["audio_divider"] == source_mux["audio_divider"]
                        and wire == source_mux["useful_video_bytes"] and cumulative_pcm == source_mux["useful_audio_bytes"],
                        "source mux byte totals/timing differ")
                for name, digest in (("oracle.frames", logical_digest), ("oracle.phases", phase_digest)):
                    oracle = assets / name
                    if oracle.exists():
                        require(sha256_file(oracle) == digest.hexdigest(), f"{name} replay differs")
                        full_oracles_verified.append(name)
            require(not source.read(1), "unexpected trailing bytes")
        finally:
            for opened in (initial_source, pcm_source, stream_source):
                if opened:
                    opened.close()
    quality = source_manifest.get("rate_control") if source_manifest else None
    quality_summary = {key: value for key, value in quality.items() if key != "frames"} if quality else None
    return {"status": "passed", "container": str(path), "source_assets": str(assets) if assets else None,
            "header": header, "source_stream_byte_exact": bool(assets), "source_initial_byte_exact": bool(assets),
            "source_pcm_byte_exact": bool(assets), "oracle_frame_hashes_verified": bool(hash_oracles),
            "full_oracle_files_verified": full_oracles_verified,
            "duration_seconds": records / PAL_HZ, "content_fps": PAL_HZ / FRAME_TICKS,
            "phase_fps": PAL_HZ, "audio_sample_hz": 6250000 / (header["audio_divider"] + 2),
            "rate_control": quality_summary,
            "replayed_records": records, "completed_frames": completed, "updated_columns": columns,
            "logical_oracle_sha256": logical_digest.hexdigest(), "phase_oracle_sha256": phase_digest.hexdigest(),
            "pcm_bytes": cumulative_pcm, "pcm_sha256": pcm_digest.hexdigest(), "wire_bytes": wire,
            "source_pcm_bytes": (assets / "audio.pcm").stat().st_size if assets else None,
            "source_nominal_pcm_tail_bytes": (assets / "audio.pcm").stat().st_size - cumulative_pcm if assets else None,
            "container_sha256": sha256_file(path)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    create = sub.add_parser("package")
    create.add_argument("assets", type=Path)
    create.add_argument("--output", type=Path, required=True)
    create.add_argument("--title", required=True)
    create.add_argument("--report", type=Path)
    check = sub.add_parser("audit")
    check.add_argument("container", type=Path)
    check.add_argument("--assets", type=Path)
    check.add_argument("--report", type=Path)
    args = parser.parse_args()
    report = package(args.assets, args.output, args.title) if args.command == "package" else audit(args.container, args.assets)
    report_path = args.report or (args.output if args.command == "package" else args.container).with_suffix(".container.json")
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
