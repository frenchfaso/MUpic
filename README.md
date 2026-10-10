# MUpic

**16 colors is all you need!**

Movie night, Commodore style. MUpic turns video into 16-color pictures and stereo sound for the **C64 Ultimate at 64 MHz**. The Rust CLI and browser converter share the same codec, positional dithering and cycle-budget planner.

**[Watch the demo](https://frenchfaso.github.io/MUpic/)** · **[Convert a video](https://frenchfaso.github.io/MUpic/convert.html)** · **[Download the player](releases/MUPIC.PRG)**

The SD canvas is 360×200 with up to 356×200 active picture, preserved aspect ratio and about 16.7 fps. Native CPU threads, browser workers and optional GPU hints accelerate conversion. Browser processing stays local; its preview reconstructs the encoded file.

## Convert and play

Install Rust stable, FFmpeg and `ffprobe`, then run:

```sh
cargo build --locked --release --manifest-path encoder/Cargo.toml
encoder/target/release/mupic-encoder input.mp4 -o movie.mup
```

Use `--help` for options. Native GPU hints require a build with `--features gpu` and conversion with `--gpu on`; CPU is the default and fallback.

Copy `movie.mup` and `MUPIC.PRG` to a FAT32 USB stick. Use PAL, 64 MHz, HDMI SD output with 4:3 shape, 16 MB REU with preload disabled, command interface enabled, and UltimateAudio at `$DF20–DFFF`. Set sampler L/R volume to 0 dB and pan left/right; see the [reference configuration](releases/ultimate-pal.cfg).

Run the PRG from the Ultimate file browser. Arrow keys select, Return plays, Left goes up a directory, Space pauses, and RUN/STOP returns to the list. EOF returns to the browser. Playback uses two 256 KiB REU cache slots. Physical hardware validation remains outstanding.

## Build and run locally

Use Python 3 and Node.js 24 for tooling and tests; the emulator build also needs Binaryen's `wasm-opt` on `PATH` (or `--wasm-opt /path/to/wasm-opt`). Rebuild browser assets after changing their sources:

```sh
rustup target add wasm32-unknown-unknown
python3 tools/build_encoder_wasm.py
python3 tools/build_emulator_wasm.py
python3 tools/serve.py
```

Open `http://localhost:6411/` or `/convert.html`. To build the player, install Java and [KickAssembler](https://www.theweb.dk/KickAssembler/), then run `python3 tools/build_player.py --kickass /path/to/KickAss.jar`; output is `build/player/MUPIC.PRG`.

The two demo movies are compressed Git LFS inputs. Install Git LFS and mtools, then run `git lfs pull` and `python3 tools/build_demo.py` to generate streaming media. Build outputs and expanded movies stay ignored.

## Tests and sources

```sh
cargo test --locked --manifest-path encoder/Cargo.toml
cargo check --locked --manifest-path encoder/Cargo.toml --features gpu
node --test tests/*.test.mjs
python3 -m unittest discover -s tests -p 'test_*.py'
node encoder/tests/wasm-abi.cjs site/assets/encoder.wasm
node encoder/tests/cli-failures.cjs
node encoder/tests/cli-wasm-parity.cjs
```

The CLI checks need FFmpeg and the release encoder built above; disk tests need mtools. Encoder sources live in `encoder/`, assembly in `player/`, emulator sources in `demo/emulator/`, browser code in `site/`, and build tools in `tools/`.

GPL-3.0; see [LICENSE](LICENSE) and [CREDITS](CREDITS.md) for authors, media attribution and third-party licenses.
