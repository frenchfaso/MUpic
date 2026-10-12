# MUpic

**16 colors is all you need!**

Movie night, Commodore style. MUpic turns video into 16-color pictures and stereo sound for the **C64 Ultimate at 64 MHz**. The Rust CLI and browser converter share the same codec, positional dithering and cycle-budget planner.

**[Watch the demo](https://frenchfaso.github.io/MUpic/)** · **[Convert a video](https://frenchfaso.github.io/MUpic/convert.html)** · **[Download the player](releases/MUPIC.PRG)**

The SD canvas is 360×200 with up to 356×200 active picture, preserved aspect ratio and about 16.7 fps. The CLI and browser share a lookup table covering all 16,777,216 RGB colors. Choose a video and audio format, then press **Encode** to start browser conversion. Available audio is included with automatic level adjustment, and conversion stays local. Original and decoded MUP previews have independent playback controls; MUP playback follows the file's own timing.

## Convert and play

Install Rust stable, FFmpeg and `ffprobe`, then run:

```sh
cargo build --locked --release --manifest-path encoder/Cargo.toml
encoder/target/release/mupic-encoder input.mp4 -o movie.mup
encoder/target/release/mupic-encoder input.mp4 -o movie-pcm8.mup --audio-format pcm8
```

Use `--help` for options and `--threads N` to set the native CPU thread count for ordinary color search. The browser selects its fallback worker count automatically.

PCM16 at 22.085 kHz remains the default. Select **PCM8 · HQ noise shaping** on the encoder page or use `--audio-format pcm8` in the CLI to preserve the source sample rate (8–48 kHz) with Shibata noise shaping and TPDF dithering. Both use the same Rust quantizer, with automatic headroom to prevent clipping. The Ultimate's nearest hardware divider introduces a small rate rounding; audio sample counts and preview playback follow its actual timing. Unreadable or unsupported source rates are rejected rather than silently changed. PCM8 movies require the current `MUPIC.PRG`; existing PCM16 movies remain compatible. Playback bandwidth is two bytes per stereo sample for PCM8, four for PCM16; MUP cache blocks remain padded to 256 KiB.

The complete table is generated offline and distributed as a losslessly compressed asset. The CLI embeds it; the browser downloads it once and saves it in IndexedDB. Neither requires a lengthy local precalculation. Encoding uses direct lookups in a 64 MiB table, preserving the original positional dithering. The table works for any video decoded to RGB24 with the current palette and algorithm.

Checksums and an algorithm/palette fingerprint protect loading and cache reuse. The CLI caches the expanded asset in the system user-cache directory (`--cache-dir PATH` overrides it). Use `mupic-encoder --prepare-lut` to warm this cache, or `--no-cache` for ordinary exact color search. Unavailable browser storage still allows acceleration for the current page. Clearing storage requires downloading the compressed asset again. Palette or algorithm changes require regenerating it offline.

Copy `movie.mup` and `MUPIC.PRG` to a FAT32 USB stick. Use PAL, 64 MHz, HDMI SD output with 4:3 shape, at least 2 MB REU with preload disabled, command interface enabled, and UltimateAudio at `$DF20–DFFF`. Set sampler L/R volume to 0 dB and pan left/right; see the [reference configuration](releases/ultimate-pal.cfg).

Run the PRG from the Ultimate file browser. Arrow keys select, Return plays, Left goes up a directory, Space pauses, and RUN/STOP returns to the list. EOF returns to the browser. Playback uses two 256 KiB REU cache slots. Physical hardware validation remains outstanding.

The web demo silently preloads about nine seconds of both movies during startup and maintains that reserve for the playing movie within its 16 MiB browser cache. Audio starts automatically when browser policy permits; otherwise the first click, tap or key on the page enables it.

Both demo movies use stereo PCM8 with HQ noise shaping at their source rates: 44.1 kHz for Big Buck Bunny and 48 kHz for CYBER77. Their video frames, timing and titles are unchanged.

## Build and run locally

Use Python 3 and Node.js 24 for tooling and tests. Both WASM builds need Binaryen's `wasm-opt` on `PATH` (or `--wasm-opt /path/to/wasm-opt`) and use SIMD128, linker O3 and Binaryen O3. Rebuild browser assets after changing their sources:

```sh
rustup target add wasm32-unknown-unknown
python3 tools/build_encoder_wasm.py
python3 tools/build_emulator_wasm.py
python3 tools/serve.py
```

Open `http://localhost:6411/` or `/convert.html`. To build the player, install Java and [KickAssembler](https://www.theweb.dk/KickAssembler/), then run `python3 tools/build_player.py --kickass /path/to/KickAss.jar`; output is `build/player/MUPIC.PRG`.

The two demo movies are compressed Git LFS inputs. Install Git LFS and mtools, then run `git lfs pull` and `python3 tools/build_demo.py` to generate streaming media. Build outputs and expanded movies stay ignored.

To regenerate the LUT, install [WABT](https://github.com/WebAssembly/wabt) and a C11 compiler:

```sh
python3 tools/build_lut_generator.py --wasm2c /path/to/wabt/bin/wasm2c
build/lut-generator/mupic-lut-generate --output build/rgb24-lut --threads auto
python3 tools/package_lut.py build/rgb24-lut --metadata build/rgb24-lut/lut_metadata.json --compare
```

The offline tool translates the shared codec into C, preserving its math routines. For multiple machines, compile the same generated C sources on each host and assign disjoint `--start`/`--end` block intervals. Completed blocks are validated and reused; merge all 4,096 blocks before packaging. Generation tools are excluded from production WASM.

## Tests and sources

```sh
cargo test --locked --manifest-path encoder/Cargo.toml
node --test tests/*.test.mjs
python3 -m unittest discover -s tests -p 'test_*.py'
node encoder/tests/wasm-abi.cjs site/assets/encoder.wasm
node encoder/tests/cli-failures.cjs
node encoder/tests/cli-wasm-parity.cjs
node encoder/tests/pcm8-parity.cjs
```

The CLI checks need FFmpeg and the release encoder built above; disk tests need mtools. Encoder sources live in `encoder/`, assembly in `player/`, emulator sources in `demo/emulator/`, browser code in `site/`, and build tools in `tools/`.

GPL-3.0; see [LICENSE](LICENSE) and [CREDITS](CREDITS.md) for authors, media attribution and third-party licenses.
