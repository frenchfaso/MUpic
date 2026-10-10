# Rust reSID in the WebRust fork

Base: binaryfields/resid-rs 1.1.1, Sebastian Jastrzebski, GPL-3.0-or-later.
The local additions translate VICE 3.10 reSID 1.0-pre2 (Dag Lem,
GPL-2.0-or-later, redistributed here under GPL-3.0-or-later), source commit
`e635822a93`. Copyright notices remain in the Rust files. `LICENSE` is retained.
The source and packed/raw table SHA-256 values are recorded in
`src/data/vice/manifest.json`. No C++ runtime or C++ oracle generator is shipped.
Tables may be regenerated separately from the pinned upstream VICE source.

Implemented modern paths: both 6581/8580 DACs and analog filter,
filter bias, voice/EXT IN masks, external RC output filter, waveform/noise/test
and ADSR pipelines, Fast/Interpolate/FIR resamplers, cycle-stamped writes,
readback bus aging, and versioned Rust snapshots including FIR cadence/history.
Digital shadow clocking avoids analog/FIR work while preserving OSC3/ENV3.
WASM SIMD128 convolution uses signed dot products with wrapping 32-bit sums.

The default analog table path expands each used model once into immutable
shared tables (10,620,930 bytes/model) to favor speed. `vice-packed-tables`
uses exact bitplane lookups instead, retaining 3,544,010 packed bytes for both
models plus VCR; this avoids the expanded resident tables at a throughput cost.
Neither path approximates table values. FIR defaults to the interpolated
polyphase `Resample` method; `Interpolate` is available as a lower-cost audio
quality setting, and `ResampleFast` trades FIR table memory for one convolution.

Validation scope: the 73,728-output pinned C++ analog-stage sweep covers both
models, four cutoffs, six filter/output modes and three voice masks. The longer
70,384-sample saw/noise/release fixture covers the default 6581 unfiltered
signal path and compares against native C++ and C++ WASM PCM references.
This fixture and the analog-stage sweep do not prove full-song equivalence,
all combined waveforms, all write edge cases, or audible quality on hardware.
Native timing results are diagnostic; browser realtime performance must be
measured separately. Refer to `reports/sid-validation.json` for measured values.
