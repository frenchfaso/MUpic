#!/usr/bin/env python3
"""Build the portable encoder and atomically stage its raw-ABI WASM module."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
REQUIRED = {
    "memory", "mup_alloc", "mup_free", "mup_begin", "mup_quantize",
    "mup_quantize_seeded", "mup_gpu_search_ptr", "mup_gpu_search_len",
    "mup_encode_packed", "mup_finish", "mup_result_ptr", "mup_result_len",
    "mup_output_ptr", "mup_output_len", "mup_output_clear",
    "mup_preview_ptr", "mup_preview_len", "mup_error_ptr", "mup_error_len",
    "mup_peak_slots", "mup_adapted_frames", "mup_received_frames",
}

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Build and verify the checked-in asset without replacing it")
    parser.add_argument("--offline", action="store_true", help="Use only the existing Cargo dependency cache")
    args = parser.parse_args()
    common = ["--manifest-path", str(ROOT / "encoder/Cargo.toml"), "--locked"]
    offline = ["--offline"] if args.offline else []
    subprocess.run(["cargo", "build", "--release", "--lib", "--target", "wasm32-unknown-unknown", *common, *offline], cwd=ROOT, check=True)
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps", *common, *offline], cwd=ROOT))
    source = Path(metadata["target_directory"]) / "wasm32-unknown-unknown/release/mupic_encoder.wasm"
    payload = source.read_bytes()
    if not payload.startswith(b"\x00asm\x01\x00\x00\x00"):
        raise SystemExit("Build did not produce a WebAssembly module")
    if shutil.which("node"):
        verifier = """const fs=require('node:fs');const m=new WebAssembly.Module(fs.readFileSync(process.argv[1]));
if(WebAssembly.Module.imports(m).length)throw Error('The encoder must not import host functions');
const names=new Set(WebAssembly.Module.exports(m).map(e=>e.name));
for(const name of JSON.parse(process.argv[2]))if(!names.has(name))throw Error('Missing raw ABI export: '+name);"""
        subprocess.run(["node", "-e", verifier, str(source), json.dumps(sorted(REQUIRED))], check=True)
    destination = ROOT / "site/assets/encoder.wasm"
    shader_source = ROOT / "encoder/shaders/author-seed.wgsl"
    shader_destination = ROOT / "site/assets/author-seed.wgsl"
    shader = shader_source.read_bytes()
    shader_matched = shader_destination.exists() and shader_destination.read_bytes() == shader
    matched = destination.exists() and destination.read_bytes() == payload
    if args.check and not shader_matched:
        raise SystemExit("Checked-in author-seed.wgsl differs from the native shader; rerun without --check")
    if args.check and not matched:
        raise SystemExit("Checked-in encoder.wasm differs from this build; rerun without --check")
    if not args.check:
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary = destination.with_name(".encoder.wasm.tmp")
        temporary.write_bytes(payload)
        os.replace(temporary, destination)
        shader_temporary = shader_destination.with_name(".author-seed.wgsl.tmp")
        shader_temporary.write_bytes(shader)
        os.replace(shader_temporary, shader_destination)
    print(json.dumps({"artifact": str(destination), "bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest(), "byte_identical_before_staging": matched, "checked": args.check, "shader_sha256": hashlib.sha256(shader).hexdigest(), "shader_byte_identical_before_staging": shader_matched}, indent=2))

if __name__ == "__main__":
    main()
