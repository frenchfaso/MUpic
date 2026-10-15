# MUpic (Motion UPIC)

**16 colors is all you need!**

Your videos, with a Commodore twist. MUpic brings moving pictures and stereo sound to the **C64 Ultimate**, with up to **360×200 pixels in 16 colors**, at **16.7 fps PAL** or **15 fps NTSC**.

**[▶ Watch the demo](https://frenchfaso.github.io/MUpic/)** · **[✦ Convert a video](https://frenchfaso.github.io/MUpic/convert.html)** · **[↓ Get the player](releases/MUPIC.PRG)**

## Make your own

1. Open the **[web encoder](https://frenchfaso.github.io/MUpic/convert.html)** and choose a video.
2. Match **System** (PAL/NTSC) and **HDMI** (SD/HD) to your Ultimate, then pick your audio.
3. Press **Encode**, watch the preview, and download your `.mup` file.

Everything happens in your browser. Your video stays on your device, and its proportions are preserved automatically—no cropping or stretching.

Choose **PCM16** for full-resolution audio, or **PCM8 HQ** for lighter audio with carefully shaped dithering. Both keep the source sample rate. Volume is adjusted automatically; silent videos work too.

Start with a short clip! Movies can now run beyond the old 18–22 minute limit: only the **FAT32 limit of 4 GiB per file** remains (about **109 minutes PAL** or **91 minutes NTSC**). Use the latest player for longer movies; your existing MUP files still work. Long conversions also need enough free memory and storage.

## Showtime

Copy your `.mup` files and **[MUPIC.PRG](releases/MUPIC.PRG)** to a **FAT32 USB stick**, then launch the player from the Ultimate file browser.

For first-time setup, use **64 MHz**, at least **2 MB REU**, REU preload off, command interface on, and **UltimateAudio at `$DF20–DFFF`**. Set sampler volume to **0 dB**, with left/right panning. Match PAL/NTSC and HDMI to the video; the player shows the required HDMI setting.

The **[example configuration](releases/ultimate-pal.cfg)** uses PAL + SD, matching the two demo movies.

**↑ ↓** select · **Return** play · **Space** pause · **RUN/STOP** back

No Ultimate handy? The **[browser demo](https://frenchfaso.github.io/MUpic/)** includes an emulator and two movies ready to play.

## Prefer the terminal?

With Rust and FFmpeg installed:

```sh
cargo build --locked --release --manifest-path encoder/Cargo.toml
encoder/target/release/mupic-encoder input.mp4 -o movie.mup
```

Defaults: **PAL · HD · PCM16**. Run with `--help` for audio, video profiles and trimming. The CLI and browser use the same encoder.

For tinkering, see [encoder](encoder/), [player](player/), [build tools](tools/) and [tests](tests/).

## Made possible thanks to…

The people behind Ultimate, UPIC, the emulator and the other projects that make this little adventure possible. Meet them in the **[credits](CREDITS.md)**.

Original code under **[GPL-3.0-only](LICENSE)**; third-party components keep their own licenses. Playback profiles are emulator-tested; verification on a physical Ultimate is still pending.
