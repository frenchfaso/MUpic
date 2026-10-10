# Rust SID replacement provenance

This WebRust fork does not build or ship the original C++ runtime. The adapter
in `src/resid_ffi.rs` owns the local `../../resid-rs` Rust SID. C++ source and
its shim were removed from this independent copy after reference validation.

The replacement retains the GPL-3.0-or-later `resid-rs` license and the Dag Lem
copyright notices of its VICE-derived runtime/table translations. The previous
reSID source was VICE 3.10, reSID 1.0-pre2, from VICE-Team/svn-mirror commit
`e635822a93`, imported 2026-05-22. Exact source/table fingerprints are retained
in `resid-rs/src/data/vice/manifest.json`; `resid-rs/PROVENANCE.md` documents the
verified scope and remaining limits.

The committed C++ native/WASM PCM binaries are test reference data only.
The new Rust snapshot format starts with `RSR1`; it includes sampler history
and is incompatible with native C++ structure memory snapshots.
