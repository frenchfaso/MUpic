# MUpic encoder

**16 colors is all you need!** Feed a video to the Rust CLI, or drop it onto the browser converter. Both use the same SD codec and exact color search.

```sh
# FFmpeg and ffprobe must be on PATH (macOS: brew install ffmpeg).
cargo build --release --manifest-path encoder/Cargo.toml
encoder/target/release/mupic-encoder movie.mp4 -o MOVIE.MUP

# Optional native WebGPU compute hints (Metal, Vulkan or Direct3D via wgpu).
cargo build --release --manifest-path encoder/Cargo.toml --features gpu
encoder/target/release/mupic-encoder movie.mp4 -o MOVIE.MUP --gpu on
```

`--threads auto` uses the available CPU workers. `--gpu on` requests compute search hints and falls back to the CPU if an adapter is unavailable or fails. `auto` currently prefers the measured CPU baseline; on the tested M3, GPU hints added a little overhead after exact refinement. Compute shaders supply search hints, followed by the authoritative exact CPU search, so acceleration does not change the palette or dithering decisions.

Useful options: `--start 10 --duration 30`, `--gain-db 6`, `--title "MY MOVIE"`, and `--force`. Run `--help` for the complete list. The default preserves the full video and its audio, with no gain. Videos with no audio track get silence.

The output is MUP9 with the native SD codec9 profile: 360×200 canvas, up to 356×200 active picture, square pixels, PAL/3 (about 16.708 fps), and stereo PCM16 at nominal 22,085 Hz. Source aspect ratio is preserved with black bars. The fixed Bayer pattern uses Yliluoma's author preset and neutral guard. The qualified dictionary/SKIP/RAW planner applies only the perceptual substitutions needed to meet 425,000 estimated CPU/DMA slots per tick, and refuses an infeasible transition rather than producing unchecked output.

Video decoding is streamed from FFmpeg in batches of eight frames. The encoder retains the current frame and cache block; the decoded PCM is retained for the conversion. The pure RGB lookup cache is capped at 1,048,576 keys on the host and 262,144 keys per browser worker; clearing it changes only the amount of work, never the pixels. Finished cache blocks are written immediately to a private temporary file, and the header is filled after the final SHA-256 is known. The completed file is published only on success, preserving any previous output if conversion fails. The current player supports at most 21,845 source frames (about 21.8 minutes).

## Browser build

```sh
rustup target add wasm32-unknown-unknown
python3 tools/build_encoder_wasm.py
# Optional: verify a rebuild and the shared shader exactly match shipped assets.
python3 tools/build_encoder_wasm.py --check
```

The module imports nothing. Each browser worker owns an independent instance, so parallel conversion works on static hosting without shared memory or cross-origin isolation. Quantizers work in parallel; the sequential stateful encoder streams complete 256 KiB blocks. The browser reconstructs the emitted MUP for its preview rather than emulating the C64.

## Raw WebAssembly ABI

All status functions return **0 on success, 1 on error**. Read `mup_error_ptr/len` after a failure. Input buffers come from `mup_alloc(bytes)` and are released with `mup_free(pointer, bytes)`. Inputs are copied when needed; views must be refreshed after memory growth, and outputs must be copied before the next mutation.

- `mup_quantize(rgb_ptr, rgb_len)`: one 360×200 RGB24 frame; `mup_result_ptr/len` returns 36,000 row-major bytes, with even X in the low nibble.
- `mup_gpu_search_ptr/len`: the optional compute search table, length in bytes. `mup_quantize_seeded(rgb_ptr, rgb_len, seed_ptr, seed_len)` accepts one u32 hint per pixel and still performs exact refinement.
- `mup_begin(frame_count, pcm_ptr, pcm_len, title_ptr, title_len)`: PCM is interleaved stereo S16LE at nominal 22,085 Hz; the title is at most 63 ASCII characters. All input data is copied.
- `mup_encode_packed(ptr, len)`: submit the next quantized frame. `mup_preview_ptr/len` exposes its actual post-rate-control reconstruction.
- `mup_output_ptr/len`: copy completed cache blocks, then call `mup_output_clear()`.
- `mup_finish()`: emits the remaining blocks and places the startup prefix in `mup_result_ptr/len`. The final file is the prefix followed by all drained blocks in order.

`mup_peak_slots`, `mup_adapted_frames` and `mup_received_frames` expose progress. The two outer pixels on each side must be black; the quantizer enforces them.

```sh
cargo test --manifest-path encoder/Cargo.toml
node encoder/tests/wasm-abi.cjs site/assets/encoder.wasm
```
