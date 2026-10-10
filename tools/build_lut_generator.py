#!/usr/bin/env python3
"""Build and verify the shared offline RGB24 generator as portable C."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
RUSTFLAGS = "-Ctarget-feature=-simd128 -Clink-arg=-O3 -Clink-arg=--strip-all"
CFLAGS = ["-std=c11", "-O3", "-D_DEFAULT_SOURCE", "-D_DARWIN_C_SOURCE",
          "-fno-fast-math", "-ffp-contract=off", "-frounding-math",
          "-fexcess-precision=standard", "-fno-optimize-sibling-calls", "-pthread"]
REQUIRED = ["memory", "mup_alloc", "mup_free", "mup_plan_keys",
            "mup_result_ptr", "mup_result_len", "mup_error_ptr", "mup_error_len",
            "mup_lut_identity", "mup_lut_patterns", "mup_lut_info",
            "mup_lut_block_identity", "mup_lut_candidate_count",
            "mup_lut_generate", "mup_lut_validate"]
VERIFIER = r"""
const fs = require('node:fs'), path = require('node:path');
const dir = process.argv[1];
const module = new WebAssembly.Module(fs.readFileSync(path.join(dir, 'generator.wasm')));
if (WebAssembly.Module.imports(module).length) throw Error('Host imports are forbidden');
const e = new WebAssembly.Instance(module, {}).exports;
for (const name of JSON.parse(process.argv[2]))
  if (!(name in e)) throw Error('Missing offline ABI export: ' + name);
const result = () => Buffer.from(new Uint8Array(e.memory.buffer, e.mup_result_ptr(), e.mup_result_len()));
function call(name, ...args) {
  if (e[name](...args))
    throw Error(Buffer.from(new Uint8Array(e.memory.buffer, e.mup_error_ptr(), e.mup_error_len())).toString());
  return result();
}
const info = call('mup_lut_info');
const metadata = {
  algorithmIdentity: call('mup_lut_identity').toString(),
  patternFingerprint: call('mup_lut_patterns').toString(),
  candidateCount: call('mup_lut_candidate_count').readUInt32LE(0),
  domain: info.readUInt32LE(12), headerBytes: 128,
  blockIdentity: call('mup_lut_block_identity').toString(),
};
if (info.length !== 16 || info.readUInt32LE(0) !== 4096 || info.readUInt32LE(4) !== 4096)
  throw Error('Incompatible offline block layout');
// Include neutral boundaries and colors with previously observed numeric ties.
const keys = [0, 0xffffff, 0x010101, 0xfefefe, 0xff0000, 0x00ff00, 0x0000ff,
  0x585858, 0x595959, 0x8a8a8a, 0x989898, 0xa2a2a2, 0xb99bf6, 0xe897a4,
  0xec0346, 0xf1447c, 0xf65143, 0x8a904c, 0x96989a, 0x9fbcda, 0xa4bedd, 0xa6c1df];
const input = Buffer.alloc(keys.length * 4);
keys.forEach((key, index) => input.writeUInt32LE(key, index * 4));
const pointer = e.mup_alloc(input.length);
new Uint8Array(e.memory.buffer, pointer, input.length).set(input);
fs.writeFileSync(path.join(dir, 'preflight-keys.le32'), input);
fs.writeFileSync(path.join(dir, 'preflight-plans.le32'), call('mup_plan_keys', pointer, input.length));
e.mup_free(pointer, input.length);
fs.writeFileSync(path.join(dir, 'wasm-block-0000.lut'), call('mup_lut_generate', 0));
console.log(JSON.stringify(metadata));
"""
MAKEFILE = """CC ?= cc
CFLAGS = {flags}
CPPFLAGS = -I.
LDLIBS = -lm -pthread
RUNTIME = generator.o wasm-rt-impl.o wasm-rt-mem-impl.o
all: mupic-lut-generate mupic-lut-preflight
mupic-lut-generate: lut_generate.o $(RUNTIME)
\t$(CC) $(CFLAGS) $^ $(LDLIBS) -o $@
mupic-lut-preflight: lut_preflight.o $(RUNTIME)
\t$(CC) $(CFLAGS) $^ $(LDLIBS) -o $@
%.o: %.c generator.h wasm-rt.h
\t$(CC) $(CPPFLAGS) $(CFLAGS) -c $< -o $@
.PHONY: all check
check: all
\t./mupic-lut-preflight preflight-keys.le32 preflight-plans.le32
"""


def executable(value: str, description: str) -> str:
    found = shutil.which(value)
    if found is None:
        raise ValueError(f"{description} was not found: {value}")
    # Preserve rustup/cc symlink names: these launchers dispatch from argv[0].
    return str(Path(found).absolute())


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wasm2c", metavar="PATH", default="wasm2c",
                        help="WABT wasm2c executable; its installation must include runtime sources")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "build/lut-generator",
                        help="Isolated build directory (default: build/lut-generator)")
    parser.add_argument("--cc", default=os.environ.get("CC", "cc"),
                        help="C compiler command (defaults to CC or cc)")
    parser.add_argument("--offline", action="store_true", help="Use the existing Cargo dependency cache")
    parser.add_argument("--wabt-license", type=Path,
                        help="WABT LICENSE file; also create a self-contained C source archive")
    args = parser.parse_args()
    try:
        cargo = executable("cargo", "Rust Cargo")
        node = executable("node", "Node.js")
        wasm2c = executable(args.wasm2c, "WABT wasm2c")
        compiler = shlex.split(args.cc)
        if not compiler:
            raise ValueError("Provide a C compiler command")
        compiler[0] = executable(compiler[0], "C compiler")
        prefix = Path(wasm2c).resolve().parent.parent
        include = prefix / "include"
        runtime = prefix / "share/wabt/wasm2c"
        sources = [runtime / "wasm-rt-impl.c", runtime / "wasm-rt-mem-impl.c"]
        for required in [include / "wasm-rt.h", runtime / "wasm-rt-impl.h", *sources]:
            if not required.is_file():
                raise ValueError(f"Incomplete WABT installation: missing {required}")
        output = args.output_dir.resolve()
        output.mkdir(parents=True, exist_ok=True)
        target = output / "cargo-target"
        env = dict(os.environ, CARGO_INCREMENTAL="0", CARGO_TARGET_DIR=str(target), RUSTFLAGS=RUSTFLAGS)
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        command = [cargo, "build", "--release", "--lib", "--locked",
                   "--manifest-path", str(ROOT / "encoder/Cargo.toml"),
                   "--target", "wasm32-unknown-unknown", "--features", "lut-tools"]
        if args.offline:
            command.append("--offline")
        subprocess.run(command, cwd=ROOT, env=env, check=True)
        with tempfile.TemporaryDirectory(prefix=".lut-build-", dir=output) as temporary:
            stage = Path(temporary)
            wasm = stage / "generator.wasm"
            shutil.copyfile(target / "wasm32-unknown-unknown/release/mupic_encoder.wasm", wasm)
            metadata = json.loads(subprocess.check_output(
                [node, "-e", VERIFIER, str(stage), json.dumps(REQUIRED)], cwd=ROOT))
            generated = stage / "generator.c"
            subprocess.run([wasm2c, str(wasm), "--module-name=generator", "-o", str(generated)], check=True)
            common = [*compiler, *CFLAGS, "-I", str(stage), "-I", str(include), "-I", str(runtime)]
            for source, name in [("lut_generate.c", "mupic-lut-generate"),
                                 ("lut_preflight.c", "mupic-lut-preflight")]:
                subprocess.run([*common, str(ROOT / "tools" / source), str(generated),
                                *map(str, sources), "-lm", "-o", str(stage / name)], check=True)
            subprocess.run([str(stage / "mupic-lut-preflight"), str(stage / "preflight-keys.le32"),
                            str(stage / "preflight-plans.le32")], check=True)
            check = stage / "check"
            subprocess.run([str(stage / "mupic-lut-generate"), "--output", str(check),
                            "--start", "0", "--end", "1", "--threads", "1"], check=True)
            if json.loads((check / "lut_metadata.json").read_text()) != metadata:
                raise ValueError("Compiled C metadata differs from the shared WASM core")
            if (check / "block-0000.lut").read_bytes() != (stage / "wasm-block-0000.lut").read_bytes():
                raise ValueError("Compiled C block 0 differs from the shared WASM core")
            (stage / "lut_metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
            report = {
                "generator": str(output / "mupic-lut-generate"),
                "source_bundle": str(output / "c-source-bundle.tar.gz") if args.wabt_license else None,
                "wasm_sha256": hashlib.sha256(wasm.read_bytes()).hexdigest(),
                "wasm2c_version": subprocess.check_output([wasm2c, "--version"], text=True).strip(),
                "c_flags": CFLAGS, "rust_flags": RUSTFLAGS,
                "verified_keys": 22, "verified_block": 0, "metadata": metadata,
            }
            (stage / "build.json").write_text(json.dumps(report, indent=2) + "\n")
            bundle = stage / "c-source"
            bundle.mkdir()
            for source in [generated, stage / "generator.h", wasm, stage / "lut_metadata.json",
                           stage / "build.json", stage / "preflight-keys.le32", stage / "preflight-plans.le32",
                           stage / "wasm-block-0000.lut", ROOT / "tools/lut_generate.c", ROOT / "tools/lut_preflight.c",
                           include / "wasm-rt.h", runtime / "wasm-rt-impl.h", *sources,
                           runtime / "wasm-rt-impl-tableops.inc", runtime / "wasm-rt-mem-impl-helper.inc"]:
                shutil.copyfile(source, bundle / source.name)
            shutil.copyfile(ROOT / "LICENSE", bundle / "LICENSE.project")
            (bundle / "Makefile").write_text(MAKEFILE.format(flags=" ".join(CFLAGS)))
            if args.wabt_license:
                shutil.copyfile(args.wabt_license, bundle / "LICENSE.wabt")
                with tarfile.open(stage / "c-source-bundle.tar.gz", "w:gz") as archive:
                    archive.add(bundle, arcname="mupic-lut-c")
            for source in bundle.iterdir():
                os.replace(source, output / source.name)
            for name in ["mupic-lut-generate", "mupic-lut-preflight"]:
                os.replace(stage / name, output / name)
            if args.wabt_license:
                os.replace(stage / "c-source-bundle.tar.gz", output / "c-source-bundle.tar.gz")
        print(json.dumps(report, indent=2))
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        raise SystemExit(f"Offline LUT generator build failed: {error}") from error


if __name__ == "__main__":
    main()
