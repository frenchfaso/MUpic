# MUpic

**16 colors is all you need!**

MUpic turns video into 16-color pictures and stereo sound for the **C64 Ultimate at 64 MHz**. The native CLI and browser encoder share the same Rust codec, positional dithering and playback cycle model.

**[Watch the demo](https://frenchfaso.github.io/MUpic/)** · **[Convert a video](https://frenchfaso.github.io/MUpic/convert.html)** · **[Download the player](releases/MUPIC.PRG)**

## Convert a video

In the browser, choose a video, select the target **System**, **HDMI** and **Audio**, then press **Encode**. Conversion stays on your device. Original and decoded MUP previews have independent controls; the download appears below the MUP preview when ready.

For the CLI, install Rust stable, FFmpeg and `ffprobe`:

```sh
cargo build --locked --release --manifest-path encoder/Cargo.toml
encoder/target/release/mupic-encoder input.mp4 -o movie.mup
encoder/target/release/mupic-encoder input.mp4 -o movie-pcm8.mup --audio-format pcm8
encoder/target/release/mupic-encoder input.mp4 -o movie-ntsc.mup --system ntsc --video-output sd
```

Defaults are **PAL, HD and PCM16**. Use `--help` for trimming and other options, or `--threads N` to set native CPU workers. Browser workers are selected automatically.

### Video profiles

The full **360×200** canvas is available in every profile. Automatic centered fitting preserves source proportions, compensates HDMI pixel shape and adds black padding where needed, without cropping. Match the profile to your Ultimate's configuration:

| System | HDMI output | Frame rate | Pixel aspect |
| --- | --- | --- | --- |
| PAL | SD: 576p, 4:3 | 16.708 fps | 1:1 |
| PAL | HD: 720p / 1080p | 16.708 fps | 15:16 |
| NTSC | SD: 480p | 14.957 fps | 5:6 |
| NTSC | HD: 720p / 1080p | 14.957 fps | 5:6 |

For example, a 16:9 source fits as 356×200 in PAL SD, 360×190 in PAL HD, or 360×169 in NTSC. PAL uses three refreshes per picture; NTSC uses four. Files support up to 65,535 refresh records, about 21 minutes PAL or 18 minutes NTSC. Use CLI `--start` and `--duration` to convert longer videos in sections.

### Audio

**PCM16** and **PCM8 HQ** retain the source sample rate (8–48 kHz), with automatic gain and clipping protection. PCM8 uses Shibata noise shaping and TPDF dithering. It uses two bytes per stereo sample, compared with four for PCM16; container blocks remain padded to 256 KiB.

The Ultimate's nearest hardware divider slightly rounds the playback rate. Sample counts and the preview follow that actual clock. Unsupported or unreadable source rates are rejected. Videos without audio produce silent movies.

## Play on the Ultimate

Copy `movie.mup` and the current `MUPIC.PRG` to a FAT32 USB stick. Configure matching PAL/NTSC and HDMI modes, **64 MHz**, at least **2 MB REU**, REU preload disabled, command interface enabled, and UltimateAudio at `$DF20–DFFF`. Set sampler L/R volume to 0 dB and pan left/right.

The [reference configuration](releases/ultimate-pal.cfg) uses PAL SD for the demo movies. The player displays the selected movie's HDMI requirement and rejects a PAL/NTSC mismatch; firmware modes must be selected manually. New profile-aware files need the current player, which also accepts existing MUP9 movies.

Run the PRG from the Ultimate file browser. Arrow keys select, Return plays, Left opens the parent directory, Space pauses/resumes, and RUN/STOP returns to the list. EOF also returns to the list.

The encoder balances packet costs before simplifying picture details. Budgets are **428,652 turbo slots per PAL refresh** and **241,605 per NTSC refresh**, retaining two raster lines for service work. Full-width scanout, instruction costs, REU transfers, cache boundaries and audio timing are checked in the emulator. Physical USB, sampler and memory-contention validation remains outstanding.

## Demo and local development

Both demo movies use PAL SD and stereo PCM8 HQ at source rates: 44.1 kHz for Big Buck Bunny and 48 kHz for CYBER77. The demo preloads about nine seconds of both movies. Browser audio starts automatically where permitted, or after the first interaction. CRT rendering follows the display's native pixel density.

The repository includes production WASM and player binaries. For a fresh local demo, install Python 3, Node.js 24, Git LFS and mtools:

```sh
git lfs install
git lfs pull
python3 tools/build_demo.py
python3 tools/serve.py
```

Open `http://localhost:6411/` or `/convert.html`. The server supplies COOP/COEP headers; GitHub Pages uses a local service worker, with one refresh on first visit. Isolation enables shared encoder buffers, the SID worker pool and shared audio ring; serial fallbacks remain available.

To rebuild changed sources, install Rust's WASM target, Node.js 24, Binaryen and Java/[KickAssembler](https://www.theweb.dk/KickAssembler/):

```sh
rustup target add wasm32-unknown-unknown
python3 tools/build_encoder_wasm.py
python3 tools/build_emulator_wasm.py
python3 tools/build_player.py --kickass /path/to/KickAss.jar
cp build/player/MUPIC.PRG releases/MUPIC.PRG
python3 tools/build_demo.py
```

Both WASM builds use release optimization, SIMD128, linker O3 and Binaryen O3, without debug symbols. Set `--wasm-opt /path/to/wasm-opt` if needed. `build_demo.py` refreshes the browser player, disk manifest and asset versions. Build outputs, expanded media and local experiments stay outside Git.

## Color lookup table

A losslessly compressed, complete RGB24 lookup table accelerates the same color search for any input video. The CLI embeds it and caches the expanded 64 MiB table; the browser downloads it once and caches it in IndexedDB. Workers share the table where supported. `--cache-dir PATH` overrides the CLI cache, `--prepare-lut` warms it, and `--no-cache` uses exact search directly.

Regeneration requires [WABT](https://github.com/WebAssembly/wabt) and a C11 compiler:

```sh
python3 tools/build_lut_generator.py --wasm2c /path/to/wabt/bin/wasm2c
build/lut-generator/mupic-lut-generate --output build/rgb24-lut --threads auto
python3 tools/package_lut.py build/rgb24-lut --metadata build/rgb24-lut/lut_metadata.json --compare
```

The generator translates the shared encoder to C. Distribute disjoint `--start`/`--end` block ranges across machines if needed; merge all 4,096 blocks before packaging. Checksums and algorithm fingerprints protect cache compatibility. Generation tools are excluded from production WASM.

## Tests and repository layout

```sh
cargo test --locked --manifest-path encoder/Cargo.toml
node --test tests/*.test.mjs
python3 -m unittest discover -s tests -p 'test_*.py'
node encoder/tests/wasm-abi.cjs site/assets/encoder.wasm
node encoder/tests/cli-failures.cjs
node encoder/tests/cli-wasm-parity.cjs
node encoder/tests/pcm8-parity.cjs
python3 tools/audit_publication.py
```

CLI checks need FFmpeg and the release encoder; disk tests need mtools. Assembled player tests additionally need `py65` and `build/player/` (`MUPIC_PLAYER_BUILD` overrides it). After assembly changes, recalibrate and rerun timing tests before updating the pinned source fingerprint.

Sources: `encoder/` (Rust codec/CLI/WASM), `player/` (assembly), `demo/emulator/` (vendored emulator), `site/` (browser), `tools/` (build and regeneration), and `tests/`. Compressed demo movies are in Git LFS; generated streaming blocks belong only in the Pages artifact. The publication audit checks the public file set, media integrity and accidental local artifacts before deployment.

GPL-3.0; see [LICENSE](LICENSE) and [CREDITS](CREDITS.md) for attribution and third-party licenses.
