# Encoder acceleration

The encoder keeps the author preset's exact colour decisions. Its fixed Bayer
pattern uses the source RGB and canvas coordinates, never the frame number.
Acceleration does not introduce a cheaper colour metric or temporal dithering.

## Native CPU

Distinct full 8-bit RGB values are planned once per batch and memoized. A scoped
worker pool drains dynamic groups of 128 colours, balancing expensive searches
without changing deterministic tie-breaking. `--threads auto` uses the host's
available CPU threads; `--threads 1` is a useful reproducible baseline.

The exact CIEDE2000 search starts with a nearby leaf rather than an arbitrary
candidate. Its conservative branch bounds then refine the result. Input that
matches a hardware colour retains that pure colour. Neutral guard applies to
the seed and final search alike.

Candidate and query chroma are computed once and reused, removing two `hypot`
calls from each examined plan. Each initial leaf is checked once, and those
already evaluated plans are excluded from the remaining exact search. These
changes retain the original floating-point arithmetic and deterministic ties.

## Browser CPU

The portable WASM module never spawns an OS thread. Independent Web Workers
provide parallelism, with one Rust worker inside each instance. This works on
GitHub Pages without `SharedArrayBuffer` or custom cross-origin isolation
headers. Each worker retains a bounded RGB cache; clearing memoized lookups
changes performance, not pixels. Frame order and compression state remain
serial and deterministic after parallel colour conversion.

## Native and browser GPU

`encoder/shaders/author-seed.wgsl` is shared by native **wgpu** and browser
**WebGPU**. It walks the candidate KD tree using an approximate version of the
CPU's inexpensive CIEDE2000 rectangle bounds, returning an initial leaf hint.
Rust always runs the exact
double-precision CIEDE2000 refinement before choosing a colour plan.

Each opaque hint also identifies its tree leaf. The CPU verifies membership,
checks every palette plan in that leaf exactly, and skips it during subsequent
refinement. This avoids redundant work without trusting a GPU distance as a
proof that another branch can be discarded.

GPU float rounding therefore affects search effort, not the output pixels.
Invalid or unsuitable hints are ignored, including non-neutral candidates
when neutral guard applies. The GPU processes distinct RGB values rather than
every repeated pixel. Native submissions contain at most 65,536 unique colours;
browser submissions de-duplicate each frame before readback. Candidate trees
stay on the device throughout the job.

The native `gpu` feature supports the backends available through wgpu on each
platform, including Metal on macOS, Vulkan and DirectX 12. GPU access is optional:
initialization or dispatch failure retains the exact CPU path. The browser
converter also falls back when WebGPU is absent or its device is lost.

```sh
cargo run --release --manifest-path encoder/Cargo.toml --features gpu -- \
  input.mp4 -o output.mup --threads auto --gpu on
```

GPU hints are an optimization, not a promise that every clip will encode faster.
Small batches can spend more time on upload and readback than they save in
search. Encoder setup, video decoding, audio conversion and rate control also
remain separate costs.
The native `auto` setting keeps the CPU path as its default; `on` explicitly
enables GPU hints, while `off` disables them.

## Reproducing the measurements

The benchmark consumes already resized **360×200 RGB24** video. It compares
three fresh-cache planning runs for one CPU thread, all available CPU threads,
and GPU hints with the same CPU pool. Every run hashes the resulting hardware
pixels and asserts exact equality before the timing results are accepted.
Candidate construction and GPU initialization are measured separately from
planning; these are not end-to-end video-encoding times.

```sh
cargo run --release --manifest-path encoder/Cargo.toml --features gpu \
  --example benchmark_author -- source.rgb 12 83
```

This selects 12 real frames at frame indices 0, 83, …, 913. See
[`acceleration-benchmark.json`](acceleration-benchmark.json) for the input and
hardware-pixel SHA-256 hashes, individual measurements and host context.

On the tested Apple M3, 12 sampled video frames contained 79,381 distinct RGB
values. All nine runs produced identical hardware pixels:

| Planning path | Median time |
| --- | ---: |
| One CPU thread | 15.281 s |
| Eight CPU threads | 3.008 s |
| GPU hints + eight CPU threads | 3.158 s |

The CPU pool was about **5.08×** faster than one thread. On this native workload,
GPU hints took about **5.0% longer** than the CPU pool alone, so the native
default remains CPU first. Earlier weighted-Lab GPU hints were also slower;
the final shader follows the CPU's tree-walk heuristic instead. Browser results
depend on its worker count and GPU implementation and should be measured
separately. Background desktop work and an earlier emulator workload are
recorded in the report; these figures are not whole-encoder throughput claims.

The implementation follows the current
[wgpu API](https://docs.rs/wgpu/30.0.1/wgpu/) and
[WebGPU specification](https://www.w3.org/TR/webgpu/).
