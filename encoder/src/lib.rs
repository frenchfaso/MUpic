//! Shared native SD codec9 encoder for the CLI and browser workers.
mod audio;
pub mod author_dither;
mod core;
mod dict4_rate;
pub mod lut;
mod sd;
mod sd_timing;
mod wasm;

pub use audio::{prepare_pcm8_hq, AudioConfig, AudioFormat, PCM8_AUDIO_OFFSET};
pub use core::*;
