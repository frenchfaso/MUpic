//! Shared audio quantization and exact cumulative PAL sampler timing.
const PAL_PHI2_HZ: u64 = 985248;
const PAL_FRAME_PHI2: u64 = 312 * 63;
const SAMPLER_CLOCK_HZ: u64 = 6250000;

pub const PCM8_AUDIO_OFFSET: usize = 221184;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioFormat {
    Pcm16,
    Pcm8,
}

/// The container and sampler must use the same sample width and rational clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioConfig {
    format: AudioFormat,
    source_rate: u32,
    divider: u16,
}
impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            format: AudioFormat::Pcm16,
            source_rate: crate::AUDIO_RATE,
            divider: crate::AUDIO_DIVIDER,
        }
    }
}
impl AudioConfig {
    pub fn pcm8(source_rate: u32) -> Result<Self, String> {
        if !(8000..=48000).contains(&source_rate) {
            return Err("PCM8 preserves source rates from 8000 to 48000 Hz; this source is outside that range".into());
        }
        // Hardware PCM8 uses a half-cycle rather than PCM16's full-cycle tail.
        let divider = (SAMPLER_CLOCK_HZ as f64 / f64::from(source_rate) - 1.5).round() as u16;
        Ok(Self {
            format: AudioFormat::Pcm8,
            source_rate,
            divider,
        })
    }
    pub fn format(self) -> AudioFormat {
        self.format
    }
    pub fn source_rate(self) -> u32 {
        self.source_rate
    }
    pub fn divider(self) -> u16 {
        self.divider
    }
    pub fn audio_offset(self) -> usize {
        if self.format == AudioFormat::Pcm8 {
            PCM8_AUDIO_OFFSET
        } else {
            crate::AUDIO_OFFSET
        }
    }
    pub fn bytes_per_frame(self) -> usize {
        if self.format == AudioFormat::Pcm8 {
            2
        } else {
            4
        }
    }
    /// Numerator/denominator in samples per second, without rounding to Hz.
    pub fn rate_ratio(self) -> (u32, u32) {
        if self.format == AudioFormat::Pcm8 {
            (12_500_000, u32::from(self.divider) * 2 + 3)
        } else {
            (6_250_000, u32::from(self.divider) + 2)
        }
    }
    pub fn rate(self) -> f64 {
        let (numerator, denominator) = self.rate_ratio();
        f64::from(numerator) / f64::from(denominator)
    }
    pub fn sample_at_tick(self, tick: usize) -> usize {
        let (rate, divisor) = self.rate_ratio();
        round_ratio(
            tick as u128 * u128::from(rate) * u128::from(PAL_FRAME_PHI2),
            u128::from(divisor) * u128::from(PAL_PHI2_HZ),
        )
    }
}

fn round_ratio(numerator: u128, denominator: u128) -> usize {
    let whole = numerator / denominator;
    let remainder = numerator % denominator;
    (whole
        + u128::from(
            remainder * 2 > denominator
                || (remainder * 2 == denominator && !whole.is_multiple_of(2)),
        )) as usize
}

#[cfg(test)]
pub(super) fn sample_at_tick(tick: usize, divider: u16) -> usize {
    // Exact rational arithmetic with ties-to-even rounding, matching the
    // cumulative Python prototype without accumulating per-block drift.
    let numerator = tick as u128 * u128::from(SAMPLER_CLOCK_HZ) * u128::from(PAL_FRAME_PHI2);
    let denominator = (u128::from(divider) + 2) * u128::from(PAL_PHI2_HZ);
    round_ratio(numerator, denominator)
}

/// Stereo Float32LE at the configured hardware rate becomes signed PCM8.
/// Shibata feedback and seeded TPDF match the HQ audio lab. Normalize the clip
/// to -6 dBFS before quantization; exceptional clipping retries with guaranteed
/// headroom rather than truncating peaks or introducing wraparound distortion.
pub fn prepare_pcm8_hq(raw_f32_interleaved: &[u8], source_rate: u32) -> Result<Vec<u8>, String> {
    AudioConfig::pcm8(source_rate)?;
    if !raw_f32_interleaved.len().is_multiple_of(8) {
        return Err("PCM8 input must be stereo Float32LE (eight bytes per sample frame)".into());
    }
    let mut peak = 0.0_f64;
    for value in raw_f32_interleaved.chunks_exact(4) {
        let sample = f32::from_le_bytes(value.try_into().unwrap());
        if !sample.is_finite() {
            return Err("PCM8 input contains a non-finite audio sample".into());
        }
        peak = peak.max(f64::from(sample).abs());
    }
    let mut output = vec![0; raw_f32_interleaved.len() / 4];
    if peak == 0.0 {
        return Ok(output);
    }
    let coefficients = shibata(source_rate);
    let gain = 10.0_f64.powf(-6.0 / 20.0) / peak;
    if !quantize(raw_f32_interleaved, coefficients, gain, &mut output) {
        let feedback_bound = 1.5 * coefficients.iter().map(|v| v.abs()).sum::<f64>();
        let safe_gain = ((125.0 - feedback_bound) / 128.0 / peak).min(gain);
        if safe_gain <= 0.0 || !quantize(raw_f32_interleaved, coefficients, safe_gain, &mut output)
        {
            return Err("PCM8 noise shaper could not preserve unclipped peaks".into());
        }
    }
    Ok(output)
}

fn quantize(input: &[u8], coefficients: &[f64], gain: f64, output: &mut [u8]) -> bool {
    let taps = coefficients.len();
    for channel in 0..2 {
        let mut seed = 0x81235_u32 + channel as u32 * 0x78131;
        // The audio lab consumes this seed value before its sample loop.
        random(&mut seed);
        let mut errors = vec![0.0_f64; taps * 2];
        let mut position = 0;
        for frame in 0..input.len() / 8 {
            let at = frame * 8 + channel * 4;
            let value = f32::from_le_bytes(input[at..at + 4].try_into().unwrap());
            let mut shaped = f64::from(value) * gain * 128.0;
            for (tap, coefficient) in coefficients.iter().enumerate() {
                shaped -= coefficient * errors[position + tap];
            }
            let noise = random(&mut seed) + random(&mut seed) - 1.0;
            let quantized = (shaped + noise).round_ties_even();
            if !(-128.0..=127.0).contains(&quantized) {
                return false;
            }
            position = if position == 0 {
                taps - 1
            } else {
                position - 1
            };
            errors[position] = quantized - shaped;
            errors[position + taps] = quantized - shaped;
            output[frame * 2 + channel] = quantized as i8 as u8;
        }
    }
    true
}

fn random(seed: &mut u32) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    f64::from(*seed) / 4_294_967_296.0
}

fn shibata(source_rate: u32) -> &'static [f64] {
    SHIBATA
        .iter()
        .min_by_key(|(rate, _)| rate.abs_diff(source_rate))
        .unwrap()
        .1
}

// Shibata coefficient data: Copyright (c) 2008-2009 robs@users.sourceforge.net.
// From FFmpeg libswresample/noise_shaping_data.c; LGPL 2.1 or later.
// https://github.com/FFmpeg/FFmpeg/blob/master/libswresample/noise_shaping_data.c
const SHIBATA: &[(u32, &[f64])] = &[
    (
        8000,
        &[
            -1.202863335609436,
            -0.9410309791564941,
            -0.6787855625152588,
            -0.5765001773834229,
            -0.5000447630882263,
            -0.44349345564842224,
            -0.37833768129348755,
            -0.3402872383594513,
            -0.2941308915615082,
            -0.24994957447052002,
            -0.21715600788593292,
            -0.18792112171649933,
            -0.15268312394618988,
            -0.12135542929172516,
            -0.09961062669754028,
            -0.07527361065149307,
            -0.048787496984004974,
            -0.0425863191485405,
            -0.02899129129946232,
            -0.011869125068187714,
        ],
    ),
    (
        11025,
        &[
            -0.9264228343963623,
            -0.9869598746299744,
            -0.631156325340271,
            -0.5196693539619446,
            -0.3973887264728546,
            -0.35679301619529724,
            -0.2972072660923004,
            -0.26310476660728455,
            -0.21719355881214142,
            -0.18561814725399017,
            -0.15404847264289856,
            -0.12687471508979797,
            -0.10339745879173279,
            -0.0836886316537857,
            -0.05875682458281517,
            -0.04689367115497589,
            -0.027950936928391457,
            -0.020740609616041183,
            -0.009366452693939209,
            -0.00602601608261466,
        ],
    ),
    (
        16000,
        &[
            -0.3725113272666931,
            -0.8142357468605042,
            -0.5501095652580261,
            -0.474057674407959,
            -0.32624706625938416,
            -0.3161766529083252,
            -0.2286367267370224,
            -0.22916607558727264,
            -0.1956561654806137,
            -0.18160104751586914,
            -0.15423151850700378,
            -0.14104481041431427,
            -0.11844276636838913,
            -0.0975830927491188,
            -0.07649359852075577,
            -0.06810691952705383,
            -0.04188165441155434,
            -0.03692242503166199,
            -0.019364040344953537,
            -0.014994367957115173,
        ],
    ),
    (
        22050,
        &[
            0.056581053882837296,
            -0.5695690512657166,
            -0.40727734565734863,
            -0.33870288729667664,
            -0.29810553789138794,
            -0.19039161503314972,
            -0.16510021686553955,
            -0.1346815973520279,
            -0.09663376957178116,
            -0.0810491293668747,
            -0.06495305895805359,
            -0.054459091275930405,
            -0.04337870702147484,
            -0.03660014271736145,
            -0.026256965473294258,
            -0.01878620684146881,
            -0.013387725688517094,
            -0.009098323062062263,
            -0.0026585909072309732,
            -0.0004208330065011978,
        ],
    ),
    (
        32000,
        &[
            0.8211855292320251,
            -1.0063692331314087,
            0.6234196424484253,
            -1.0447187423706055,
            0.6453251242637634,
            -0.8761513233184814,
            0.5221975445747375,
            -0.6743426322937012,
            0.44954317808151245,
            -0.5255749821662903,
            0.34567299485206604,
            -0.39618203043937683,
            0.26791760325431824,
            -0.28936097025871277,
            0.1883765310049057,
            -0.19097308814525604,
            0.10431359708309174,
            -0.10633844882249832,
            0.04683221876621246,
            -0.03965331241488457,
        ],
    ),
    (
        44100,
        &[
            2.6773197650909424,
            -4.830892562866211,
            6.570110321044922,
            -7.4572014808654785,
            6.726327419281006,
            -4.848165035247803,
            2.0412089824676514,
            0.7006359100341797,
            -2.95375657081604,
            4.080038547515869,
            -4.184521675109863,
            3.331181287765503,
            -2.117992639541626,
            0.879302978515625,
            -0.031759146600961685,
            -0.4238278865814209,
            0.4788210391998291,
            -0.35490813851356506,
            0.1749683916568756,
            -0.06090816855430603,
        ],
    ),
    (
        48000,
        &[
            2.87207293510437,
            -5.041323184967041,
            6.244299411773682,
            -5.848398685455322,
            3.706754207611084,
            -1.0495119094848633,
            -1.1830236911773682,
            2.1126792430877686,
            -1.9094531536102295,
            0.9991308450698853,
            -0.17090806365013123,
            -0.32615602016448975,
            0.39127644896507263,
            -0.2687646150588989,
            0.0976761057972908,
            -0.023473845794796944,
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hq_matches_audio_lab_known_vectors_and_signed_hardware_bytes() {
        // Exact binary fractions avoid host decoder/resampler differences.
        let input: Vec<u8> = (0..32)
            .flat_map(|i| [(i % 7) as f32 / 8.0 - 0.375, (i % 11) as f32 / 8.0 - 0.625])
            .flat_map(f32::to_le_bytes)
            .collect();
        let cases = [
            (22050, "d9c0e7ccf2da00e60df31900270dd91ae627f334ff410dc019cd26dad9e6e6f4f300ff0d0d1a1a262634da3fe6bff3cd00d80ce61af22600da0ce719f3260033"),
            (44100, "d9c0e8ccefdb07e404f622fd2110da17eb28ea340b3d03c51fc727ded3e5f1f1e7060a0407241b1c283ed735e8cbf3c4ffdd0dea1ce82111e2f9dd2cfb19fc3a"),
            (48000, "d9c0e8ccf0db04e409f61cfd2610d917e628f534fd3d10c517c728ded9e5e6f1f405ff070e1e18252833d940e5c2f6cafdda0eea1ceb210be002e221f3240531"),
        ];
        for (rate, expected) in cases {
            let actual = prepare_pcm8_hq(&input, rate).unwrap();
            assert_eq!(
                actual
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                expected
            );
        }
    }

    #[test]
    fn automatic_gain_preserves_headroom_and_silence_without_wrapping() {
        let input: Vec<u8> = (0..10_000)
            .flat_map(|i| {
                [
                    if i % 3 == 0 { 4.0_f32 } else { -4.0_f32 },
                    if i % 7 == 0 { -2.0_f32 } else { 2.0_f32 },
                ]
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        for rate in [8000, 11025, 16000, 22050, 32000, 44100, 48000] {
            let pcm = prepare_pcm8_hq(&input, rate).unwrap();
            assert_eq!(pcm.len(), 20_000);
            for frame in pcm.chunks_exact(2) {
                assert!((frame[0] as i8).abs() < 120);
                assert!((frame[1] as i8).abs() < 110);
            }
        }
        assert_eq!(prepare_pcm8_hq(&vec![0; 800], 48000).unwrap(), vec![0; 200]);
        assert!(prepare_pcm8_hq(&[0; 4], 48000).is_err());
        let bad: Vec<_> = [f32::NAN, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        assert!(prepare_pcm8_hq(&bad, 48000).is_err());
        assert!(AudioConfig::pcm8(96000).is_err());
        assert!(AudioConfig::pcm8(7999).is_err());
    }

    #[test]
    fn sampler_counts_use_fractional_clock_and_cumulative_rounding() {
        assert_eq!(round_ratio(5, 2), 2);
        assert_eq!(round_ratio(7, 2), 4);
        assert_eq!(AudioConfig::pcm8(48000).unwrap().divider(), 129);
        assert_eq!(AudioConfig::pcm8(44100).unwrap().divider(), 140);
        let legacy = AudioConfig::default();
        for tick in [0, 1, 20, 333, 65535] {
            assert_eq!(
                legacy.sample_at_tick(tick),
                sample_at_tick(tick, crate::AUDIO_DIVIDER)
            );
        }
        for rate in [8000, 11025, 16000, 22050, 32000, 44100, 48000] {
            let audio = AudioConfig::pcm8(rate).unwrap();
            let mut count = 0;
            for start in (0..65535).step_by(20) {
                let end = (start + 20).min(65535);
                let length = audio.sample_at_tick(end) - audio.sample_at_tick(start);
                count += length;
                assert!(length * 2 <= crate::CACHE_BYTES - audio.audio_offset());
            }
            assert_eq!(count, audio.sample_at_tick(65535));
        }
    }
}
