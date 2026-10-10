# MUpic

**16 colors is all you need!**

Movie night, Commodore style. MUpic turns video into 16-color pictures and stereo sound for the **C64 Ultimate at 64 MHz**. Modern computers do the hard part; your C64 gets the popcorn.

**[Watch the demo](https://frenchfaso.github.io/MUpic/)** · **[Convert a video](https://frenchfaso.github.io/MUpic/convert.html)** · **[Download the player](releases/MUPIC.PRG)**

- Rust CLI and a local, drag-and-drop WASM converter with original/MUpic previews.
- Native CPU threads, portable browser worker pools, and optional WebGPU compute.
- SD square pixels, up to 360×200, about 16.7 fps, stable positional dithering and UltimateAudio. A 16:9 picture fits at 356×200.
- A small 6502 player browses `.mup` files and streams them through a 512 KiB REU cache. Return plays, Space pauses, RUN/STOP goes back.

With Rust and FFmpeg installed:

```sh
cargo run --release --manifest-path encoder/Cargo.toml -- input.mp4 -o movie.mup
```

Copy the movie and `MUPIC.PRG` to a FAT32 USB stick. See [hardware setup](docs/hardware.md), [building](docs/building.md), and [acceleration](docs/acceleration.md). The encoder can make small, measured substitutions to meet the player's cycle budget; the web preview shows the resulting file.

The demo's two sample movies are stored compressed in **Git LFS** and published as lossless streaming blocks. The browser expands only the blocks it needs before emulation, so movie night does not start with a giant download.

Standing on some very clever shoulders: **Aleksi Eeben** invented [UPIC](https://csdb.dk/release/?id=263980), **Xander Mol** maintains a corrected [UPIC viewer](https://github.com/xahmol/mandelbrot-upic), **Joel Yliluoma / Bisqwit** devised the [positional dithering](https://bisqwit.iki.fi/story/howto/dither/jy/), **Björn Ottosson** created [OKLab](https://bjoernottosson.github.io/posts/oklab/), and **Gideon Zweijtzer** created the Ultimate hardware and [UltimateAudio](https://github.com/GideonZ/1541ultimate/blob/master/doc/ultimate_audio_v0.2.pdf). Thank you for making sixteen colors do so much.

GPL-3.0. Third-party components retain their licenses; see [credits](CREDITS.md). The Big Buck Bunny demo is © 2008 Blender Foundation, [CC BY 3.0](https://peach.blender.org/about/), converted to MUpic with its credits roll preserved. This is an experimental project: emulator qualification is not a physical-hardware benchmark.
