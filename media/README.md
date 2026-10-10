# Compressed demo media

Only compressed copies of the complete PAL/SD sample movies are published. The `.MUP.gz` files in Git LFS are deployment inputs; uncompressed `.mup` files are excluded from Git and Pages.

The Pages workflow verifies and expands these inputs in its temporary build directory, builds a FAT32 disk and publishes individually compressed 256 KiB disk blocks. The browser decompresses each requested block and verifies its original SHA-256 before supplying sectors to the emulator. Neither full movie is downloaded at startup.

Big Buck Bunny is © 2008 Blender Foundation, [CC BY 3.0](https://peach.blender.org/about/); its full credits roll is preserved. The second sample was converted from [the supplied Cyberpunk video](https://youtu.be/VmyOU3jLQWU); the original video remains the property of its authors. Both adaptations resize the picture, convert it to the C64 palette and resample the audio.

For a local demo build, install [Git LFS](https://git-lfs.com/) and mtools, run `git lfs pull`, then `python3 tools/build_demo.py`. Temporary expanded files remain inside the ignored `build` directory.
