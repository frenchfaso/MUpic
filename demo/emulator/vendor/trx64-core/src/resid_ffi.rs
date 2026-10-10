//! Rust reSID adapter for the browser build. No C/C++ compiler, FFI, or raw handles.
//!
//! The public name is retained for callers of the original core. The engine is a
//! thread-owned `resid::Sid` and is `Send`; immutable analog tables are shared.
//! VICE 3.10 digital pipelines, exact precomputed analog/DAC tables and its FIR
//! schedule are enabled. The pinned filter oracle verifies the analog stage;
//! that is not a claim of complete PCM equivalence for every SID tune.
//!
//! Snapshot blobs use a versioned Rust format and include the sample-ring state.
//! They are deliberately incompatible with the former opaque C++ memory dump.

use resid::{ChipModel, SamplingMethod, Sid, SidState};
use std::sync::atomic::{AtomicU64, Ordering};

pub static RESID_CONSTRUCT_COUNT: AtomicU64 = AtomicU64::new(0);
pub fn resid_construct_count() -> u64 {
    RESID_CONSTRUCT_COUNT.load(Ordering::Relaxed)
}

/// reSID `sampling_method` (siddefs.h): FAST=0, INTERPOLATE=1, RESAMPLE=2,
/// RESAMPLE_FASTMEM=3.
pub const SAMPLE_FAST: i32 = 0;
pub const SAMPLE_INTERPOLATE: i32 = 1;
pub const SAMPLE_RESAMPLE: i32 = 2;
pub const SAMPLE_RESAMPLE_FASTMEM: i32 = 3;

/// reSID `chip_model`: 0 = MOS6581, 1 = MOS8580.
pub const MODEL_6581: i32 = 0;
pub const MODEL_8580: i32 = 1;

/// The default model's Φ2 clock (`c64-pal`, Hz) — what `ResidConfig::default()` samples
/// at. A machine's own is `Machine::timing().cpu_hz` (Spec 863): build its engine with
/// [`ResidConfig::for_model`].
const DEFAULT_CLOCK_FREQ: f64 = 985248.0;
/// The `c64-pal` Φ2 clock, as a public name: hosts that build on this crate and are PAL
/// by construction (UE2's bridge) name it. Nothing in TRX64 reads it — the machine's
/// clock is `Machine::timing().cpu_hz`.
pub const PAL_CLOCK_FREQ: f64 = DEFAULT_CLOCK_FREQ;
/// Default sample rate — matches TS `DEFAULT_SAMPLE_RATE`.
pub const DEFAULT_SAMPLE_RATE: f64 = 44100.0;

/// Max samples per inner `resid_clock` call (the emit loop re-issues until the
/// cycle delta is consumed). Mirrors TS `MAX_SAMPLES_PER_CALL`.
const MAX_SAMPLES_PER_CALL: usize = 4096;

/// Configuration for the reSID audio engine. Defaults match the TS `ResidWasm`
/// engine (`resid-wasm-engine.ts` configure()): 6581, filter OFF (BUG-049
/// follow-on — the 6581 filter sounded wrong in A/B), RESAMPLE @ 44.1k/PAL.
#[derive(Clone, Copy, Debug)]
pub struct ResidConfig {
    pub model: i32,
    pub clock_freq: f64,
    pub sample_rate: f64,
    pub sampling_method: i32,
    /// SID analog filter on/off. Default OFF (matches `ResidWasm`).
    pub filter: bool,
    /// 6581 filter DC bias in volts (VICE `SidResidFilterBias`/1000). 6581=0.5.
    pub filter_bias: f64,
    /// Output RC stage (VICE enables it with the filter; reSID defaults on).
    pub external_filter: bool,
    /// Per-voice enable bitmask (VICE inits to 0x07 = all three voices).
    pub voice_mask: i32,
    /// Resampler passband Hz; <=0 → reSID default. VICE: sample*90/200.
    pub passband: f64,
    /// Output gain (VICE `SidResidGain`/100 = 0.97).
    pub gain: f64,
}

impl Default for ResidConfig {
    fn default() -> Self {
        let sample_rate = DEFAULT_SAMPLE_RATE;
        Self {
            model: MODEL_6581,
            clock_freq: DEFAULT_CLOCK_FREQ,
            sample_rate,
            sampling_method: SAMPLE_RESAMPLE,
            filter: false,
            filter_bias: 0.5,
            external_filter: true,
            voice_mask: 0x07,
            passband: (sample_rate * 90.0) / 200.0,
            gain: 0.97,
        }
    }
}

impl ResidConfig {
    /// The default engine, sampling at a model's clock with its SID (Spec 863 D3: the
    /// clock is the row's `cpu_hz`, the chip its `sid`).
    pub fn for_model(model: &crate::model::C64Model) -> Self {
        Self {
            model: model.resid_model(),
            clock_freq: model.timing.cpu_hz as f64,
            ..Self::default()
        }
    }
}
/// One independently owned Rust SID engine; movable to its audio worker.
pub struct Resid {
    cfg: ResidConfig,
    cycle_acc: f64,
    sid: Sid,
}

impl Resid {
    pub fn new(cfg: ResidConfig) -> Self {
        RESID_CONSTRUCT_COUNT.fetch_add(1, Ordering::Relaxed);
        let sid = Self::configured_sid(&cfg);
        Self {
            cfg,
            cycle_acc: 0.0,
            sid,
        }
    }

    pub fn new_default() -> Self {
        Self::new(ResidConfig::default())
    }

    fn sampling(method: i32) -> SamplingMethod {
        match method {
            SAMPLE_FAST => SamplingMethod::Fast,
            SAMPLE_INTERPOLATE => SamplingMethod::Interpolate,
            SAMPLE_RESAMPLE => SamplingMethod::Resample,
            SAMPLE_RESAMPLE_FASTMEM => SamplingMethod::ResampleFast,
            _ => panic!("invalid reSID sampling method: {method}"),
        }
    }

    fn configured_sid(cfg: &ResidConfig) -> Sid {
        let model = if cfg.model == MODEL_8580 {
            ChipModel::Mos8580
        } else {
            ChipModel::Mos6581
        };
        let mut sid = Sid::new(model);
        sid.set_voice_mask(cfg.voice_mask as u8);
        sid.enable_filter(cfg.filter);
        sid.adjust_filter_bias(cfg.filter_bias);
        sid.enable_external_filter(cfg.external_filter);
        let passband = if cfg.passband <= 0.0 {
            -1.0
        } else {
            cfg.passband
        };
        assert!(
            sid.set_sampling_parameters_with_options(
                Self::sampling(cfg.sampling_method),
                cfg.clock_freq,
                cfg.sample_rate,
                passband,
                cfg.gain
            ),
            "invalid reSID sampling configuration"
        );
        sid
    }

    pub fn set_clock_freq(&mut self, clock_freq: f64) {
        if clock_freq == self.cfg.clock_freq {
            return;
        }
        let passband = if self.cfg.passband <= 0.0 {
            -1.0
        } else {
            self.cfg.passband
        };
        assert!(
            self.sid.set_sampling_parameters_with_options(
                Self::sampling(self.cfg.sampling_method),
                clock_freq,
                self.cfg.sample_rate,
                passband,
                self.cfg.gain
            ),
            "invalid SID clock"
        );
        self.cfg.clock_freq = clock_freq;
    }

    /// Reset every digital/analog pipeline and sample-ring slot using the same configuration.
    pub fn reset(&mut self) {
        self.sid.reset();
        self.cycle_acc = 0.0;
    }

    #[inline]
    pub fn write(&mut self, reg: u8, value: u8) {
        self.sid.write(reg & 0x1f, value);
    }
    #[inline]
    pub fn read(&self, reg: u8) -> u8 {
        self.sid.read(reg & 0x1f)
    }
    #[inline]
    pub fn output(&self) -> i16 {
        self.sid.output()
    }
    pub fn clock_silent(&mut self, cycles: u32) {
        self.sid.clock_delta(cycles);
    }

    /// Digital units only, for a permanently silent CPU-side OSC3/ENV3 shadow.
    /// This shadow cannot subsequently supply audio: analog state is not advanced.
    pub fn clock_readback(&mut self, cycles: u32) {
        self.sid.clock_readback(cycles);
    }

    /// Digital shadow and exact configured 16.16 sample cadence, without producing PCM.
    pub fn clock_readback_sampled(&mut self, cycles: u32) -> usize {
        self.sid.clock_readback_sampled(cycles)
    }

    /// Advance the sample clock only when digital state was already advanced with clock_readback.
    pub fn clock_sample_count(&mut self, cycles: u32) -> usize {
        self.sid.clock_sample_count(cycles)
    }

    pub fn emit(&mut self, cycles: u32) -> Vec<i16> {
        let mut out = Vec::new();
        self.emit_into(cycles, &mut out);
        out
    }

    /// Append PCM to a caller-owned buffer, retaining its allocation across drains.
    pub fn emit_into(&mut self, mut cycles: u32, out: &mut Vec<i16>) -> usize {
        let before = out.len();
        let mut buffer = [0_i16; MAX_SAMPLES_PER_CALL];
        while cycles > 0 {
            let (count, remaining) = self.sid.sample(cycles, &mut buffer, 1);
            out.extend_from_slice(&buffer[..count]);
            assert!(
                remaining < cycles || count != 0,
                "SID sampler made no progress"
            );
            cycles = remaining;
        }
        out.len() - before
    }

    /// Encoded length of the current versioned snapshot (variable JSON length).
    pub fn state_size(&self) -> usize {
        self.capture_state().len()
    }

    pub fn capture_state(&self) -> Vec<u8> {
        let state = self.sid.read_state();
        let mut bytes = b"RSR1".to_vec();
        serde_json::to_writer(&mut bytes, &state).expect("SID snapshot serialization");
        bytes
    }

    /// Restore this fork's Rust snapshot, including FIR cadence/history.
    pub fn restore_state(&mut self, bytes: &[u8]) {
        assert_eq!(
            bytes.get(..4),
            Some(b"RSR1".as_slice()),
            "unsupported SID snapshot format"
        );
        let state: SidState = serde_json::from_slice(&bytes[4..]).expect("invalid SID snapshot");
        self.sid.write_state(&state);
    }

    pub fn config(&self) -> &ResidConfig {
        &self.cfg
    }
    pub fn cycle_accumulator(&self) -> f64 {
        self.cycle_acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sounding() -> Resid {
        let mut sid = Resid::new(ResidConfig {
            filter: true,
            ..ResidConfig::default()
        });
        for (reg, value) in [
            (0, 0x45),
            (1, 0x1d),
            (5, 0x09),
            (6, 0xf0),
            (0x18, 0x0f),
            (4, 0x21),
        ] {
            sid.write(reg, value);
        }
        sid
    }

    #[test]
    fn owned_sid_is_send() {
        fn send<T: Send>() {}
        send::<Resid>();
    }

    #[test]
    fn snapshot_replays_identical_pcm_and_reset_is_pristine() {
        let mut sid = sounding();
        sid.emit(40000);
        let saved = sid.capture_state();
        let expected = sid.emit(12000);
        sid.restore_state(&saved);
        assert_eq!(sid.capture_state(), saved);
        assert_eq!(sid.emit(12000), expected);
        sid.reset();
        let mut fresh = Resid::new(*sid.config());
        assert_eq!(sid.emit(2000), fresh.emit(2000));
    }

    #[test]
    fn emit_into_preserves_pcm_prefix() {
        let mut sid = sounding();
        let mut whole = sounding();
        let mut out = vec![123, 456];
        sid.emit_into(8000, &mut out);
        sid.emit_into(8000, &mut out);
        assert_eq!(&out[2..], whole.emit(16000));
    }
}
