# Movie night on the Ultimate

Use a C64 Ultimate / Ultimate 64 Elite II with PAL and 64 MHz turbo:

- HDMI scan resolution: **SD (480p/576p)**. Preserve the output's 4:3 shape on either a 4:3 or widescreen display.
- Turbo control: the machine's Ultimate turbo registers; CPU speed: 64 MHz.
- REU enabled, 16 MB; REU preload disabled.
- Command interface enabled; UltimateAudio mapped at `$DF20–DFFF`.
- Sampler L/R mixer volume: 0 dB; pan L left and R right.

The reference settings are in `releases/ultimate-pal.cfg`. Copy `releases/MUPIC.PRG` and your `.mup` files onto a FAT32 USB stick and run the PRG from the Ultimate file browser. No external `initial.reu` is needed.

Arrow keys select; Return opens and starts immediately. Left returns to the parent directory. Space toggles pause; RUN/STOP returns to the list. EOF returns to the browser. The list shows duration and MiB and supports up to 96 entries per directory.

The current codec9 player displays a centered 360×200 canvas within the SD 384×288 raster. Video is fitted without stretching; a 16:9 input uses 356×200, with two black pixels on either side. Content advances every three PAL refreshes (16.708 fps). The renderer remains at 50.125 Hz; updates are progressive across those three refreshes.

The `.mup` contains its header, first picture, video records and stereo signed 16-bit PCM at approximately 22.1 kHz. Two 256 KiB REU slots buffer disk reads; UltimateAudio reads its samples autonomously.

The cycle model, native emulator and browser validate the implementation. A physical Ultimate and uninterrupted audio across every host/network configuration still require testing.
