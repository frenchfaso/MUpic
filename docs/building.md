# Build MUpic

## Native encoder

Install Rust stable and FFmpeg (including `ffprobe`), then:

```sh
cargo build --release --manifest-path encoder/Cargo.toml
encoder/target/release/mupic-encoder input.mp4 -o movie.mup
```

`--threads auto` uses the host CPU cores. The optional native WebGPU backend is built with `--features gpu`; see [acceleration](acceleration.md) for measured results and fallbacks. FFmpeg handles input decoding/resizing and PCM extraction. Rust performs author-preset dithering, the codec, the cycle-budget decisions and MUP packaging.

## Browser converter

```sh
rustup target add wasm32-unknown-unknown
python3 tools/build_encoder_wasm.py
python3 -m http.server 6411 --directory site --bind 127.0.0.1
```

Visit `http://localhost:6411/convert.html`. The deployed WASM binary is included, so rebuilding is optional for a local preview. All video processing stays in the browser. Independent worker instances parallelize color conversion without requiring cross-origin isolation; the history-dependent codec and mux stay ordered. WebGPU requires a compatible browser on HTTPS or localhost.

Browser input support depends on its media decoders. The page reports unsupported audio instead of silently omitting it; the CLI is the fallback for long videos and unsupported formats.

## C64 player

Java and [KickAssembler](http://www.theweb.dk/KickAssembler/) are required. Download the assembler separately and run:

```sh
python3 tools/build_player.py --kickass /path/to/KickAss.jar
```

The resulting `build/player/MUPIC.PRG` must match the shipped player's SHA-256 when using the same source and assembler. The player defaults to codec9, PAL and SD. `releases/ultimate-pal.cfg` contains the reference settings.

## Browser emulator

Corresponding source is in `demo/emulator`. Its Cargo workspace includes the local vendor sources and CRT contract. The checked-in `site/ue2_web.wasm` is the qualified SD renderer; it does not crop or shift the frontend picture. Encoder WASM is a separate, small module used by the converter.

## GitHub Pages media

Pages is a static host. The demo uses independently hash-checked 256 KiB disk files with a 16 MiB browser cache instead of depending on HTTP Range support. Known all-zero blocks are represented explicitly in the pinned manifest. The C64 still sees the original FAT32 sectors and uses its own 512 KiB REU cache.

The two sample movies are stored compressed as `.MUP.gz` files in Git LFS. After `git lfs pull`, install mtools and run `python3 tools/build_demo.py`. The tool verifies both compressed inputs, expands them into the ignored build directory, checks their original SHA-256, builds and independently audits a FAT32 disk, then stages its immutable gzip blocks. Uncompressed `.mup` files are never committed or served. The Pages workflow runs the same process; visitors initially download firmware and directory metadata, then the selected movie incrementally.

Each nonzero disk block is losslessly gzip-compressed separately. The browser uses `DecompressionStream` and verifies the exact original length and SHA-256 before supplying sectors to the emulator. Seeking stays local to the requested blocks; the C64 decoder and the original hardware files are unchanged. Pages serves materialized blocks, not LFS pointers.
