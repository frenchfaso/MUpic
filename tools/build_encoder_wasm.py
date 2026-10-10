#!/usr/bin/env python3
"""Build, optimize, and atomically stage the production encoder WASM module."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
RUSTFLAGS = " ".join([
    "-Ctarget-feature=+simd128",
    "-Clink-arg=-O3",
    "-Clink-arg=--strip-all",
])
REQUIRED = {
    "memory", "mup_alloc", "mup_free", "mup_begin", "mup_quantize",
    "mup_encode_packed", "mup_finish", "mup_result_ptr", "mup_result_len",
    "mup_output_ptr", "mup_output_len", "mup_output_clear",
    "mup_preview_ptr", "mup_preview_len", "mup_error_ptr", "mup_error_len",
    "mup_peak_slots", "mup_adapted_frames", "mup_received_frames",
    "mup_prepare_keys", "mup_plan_keys", "mup_import_plans",
    "mup_lut_identity", "mup_lut_info", "mup_lut_patterns", "mup_lut_asset_begin", "mup_lut_asset_chunk",
    "mup_lut_asset_end", "mup_lut_asset_abort", "mup_lut_reset",
}

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Build and verify the checked-in asset without replacing it")
    parser.add_argument("--offline", action="store_true", help="Use only the existing Cargo dependency cache")
    parser.add_argument("--wasm-opt", metavar="PATH", help="Binaryen wasm-opt executable; defaults to PATH lookup")
    args = parser.parse_args()
    optimizer = shutil.which(args.wasm_opt or "wasm-opt")
    if optimizer is None:
        raise SystemExit("Binaryen wasm-opt was not found. Install Binaryen or pass --wasm-opt /path/to/wasm-opt.")
    node = shutil.which("node")
    if node is None:
        raise SystemExit("Node.js was not found. Install Node.js 24 to validate the encoder WASM module.")

    env = dict(os.environ, CARGO_INCREMENTAL="0", RUSTFLAGS=RUSTFLAGS)
    # Cargo gives this variable precedence over RUSTFLAGS; use the production flags.
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    common = ["--manifest-path", str(ROOT / "encoder/Cargo.toml"), "--locked"]
    offline = ["--offline"] if args.offline else []
    destination = ROOT / "site/assets/encoder.wasm"
    temporary: Path | None = None
    try:
        subprocess.run(["cargo", "build", "--release", "--lib", "--target", "wasm32-unknown-unknown", *common, *offline], cwd=ROOT, env=env, check=True)
        metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps", *common, *offline], cwd=ROOT, env=env))
        source = Path(metadata["target_directory"]) / "wasm32-unknown-unknown/release/mupic_encoder.wasm"
        unoptimized_bytes = source.stat().st_size
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(prefix=".encoder.", suffix=".wasm", dir=destination.parent, delete=False) as handle:
            temporary = Path(handle.name)
        subprocess.run([optimizer, str(source), "-O3", "--all-features", "--strip-debug", "-o", str(temporary)], cwd=ROOT, check=True)
        payload = temporary.read_bytes()
        if not payload.startswith(b"\x00asm\x01\x00\x00\x00"):
            raise ValueError("Optimizer did not produce a WebAssembly module")
        verifier = """const fs=require('node:fs');const m=new WebAssembly.Module(fs.readFileSync(process.argv[1]));
if(WebAssembly.Module.imports(m).length)throw Error('The encoder must not import host functions');
const names=new Set(WebAssembly.Module.exports(m).map(e=>e.name));
for(const name of JSON.parse(process.argv[2]))if(!names.has(name))throw Error('Missing raw ABI export: '+name);"""
        subprocess.run([node, "-e", verifier, str(temporary), json.dumps(sorted(REQUIRED))], check=True)
        matched = destination.exists() and destination.read_bytes() == payload
        if args.check and not matched:
            raise SystemExit("Checked-in encoder.wasm differs from this optimized build; rerun without --check")
        if not args.check:
            temporary.chmod(0o644)
            os.replace(temporary, destination)
            temporary = None
        print(json.dumps({"artifact": str(destination), "bytes": len(payload), "unoptimized_bytes": unoptimized_bytes, "sha256": hashlib.sha256(payload).hexdigest(), "byte_identical_before_staging": matched, "checked": args.check}, indent=2))
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        raise SystemExit(f"Encoder build failed: {error}") from error
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)

if __name__ == "__main__":
    main()
