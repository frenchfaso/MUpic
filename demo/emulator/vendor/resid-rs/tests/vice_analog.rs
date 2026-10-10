#![cfg(feature = "vice-analog")]
use resid::filter::Filter;
use resid::{ChipModel, SamplingMethod, Sid};

#[test]
fn analog_stage_matches_pinned_cpp_oracle() {
    let bytes = include_bytes!("data/vice_filter_reference.s16le");
    let mut cursor = 0;
    for model in [ChipModel::Mos6581, ChipModel::Mos8580] {
        for cutoff in [0_u16, 512, 1800, 2047] {
            for mode in [0x0f, 0x1f, 0x2f, 0x4f, 0x7f, 0x8f] {
                for mask in [0, 1, 7] {
                    let mut f = Filter::new(model);
                    f.set_voice_mask(mask);
                    f.adjust_filter_bias(0.5);
                    f.set_fc_lo((cutoff & 7) as u8);
                    f.set_fc_hi((cutoff >> 3) as u8);
                    f.set_res_filt(0xb7);
                    f.set_mode_vol(mode);
                    for cycle in 0..512_i32 {
                        let v1 = ((cycle * 4093) & 0xfffff) - 524288;
                        let v2 = if cycle & 64 != 0 { 400000 } else { -400000 };
                        let v3 = ((cycle * 137) & 0x7ffff) - 262144;
                        f.clock(v1, v2, v3, 0);
                        let expected =
                            i16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as i32;
                        assert_eq!(
                            f.output(),
                            expected,
                            "model={} cutoff={cutoff} mode={mode:02x} mask={mask} cycle={cycle}",
                            if matches!(model, ChipModel::Mos6581) {
                                0
                            } else {
                                1
                            }
                        );
                        cursor += 2;
                    }
                }
            }
        }
    }
    assert_eq!(cursor, bytes.len());
}

fn sid() -> Sid {
    let mut sid = Sid::new(ChipModel::Mos8580);
    assert!(sid.set_sampling_parameters_with_options(
        SamplingMethod::Resample,
        985248.0,
        48000.0,
        21600.0,
        0.97
    ));
    sid.adjust_filter_bias(0.5);
    sid.set_voice_mask(7);
    for (register, value) in [
        (0, 0xb1),
        (1, 0x19),
        (2, 0),
        (3, 8),
        (5, 0x04),
        (6, 0xf7),
        (4, 0x41),
        (0x15, 7),
        (0x16, 100),
        (0x17, 0xc1),
        (0x18, 0x1f),
    ] {
        sid.write(register, value);
    }
    sid
}

#[test]
fn sampling_is_independent_of_cycle_batching() {
    let mut whole = sid();
    let mut split = whole.clone();
    let mut expected = [0_i16; 5000];
    let (length, remaining) = whole.sample(80000, &mut expected, 1);
    assert_eq!(remaining, 0);
    let mut actual = Vec::new();
    let mut cycles = 80000;
    let mut buffer = [0_i16; 97];
    while cycles != 0 {
        let delta = cycles.min(137);
        let (length, remaining) = split.sample(delta, &mut buffer, 1);
        assert_eq!(remaining, 0);
        actual.extend_from_slice(&buffer[..length]);
        cycles -= delta;
    }
    assert_eq!(&expected[..length], actual);
    assert!(actual.iter().any(|s| *s != 0));
}

#[test]
fn stride_preserves_other_channels_and_returns_unconsumed_cycles() {
    let mut sid = sid();
    let mut buffer = [123_i16; 7];
    let (count, remaining) = sid.sample(20000, &mut buffer, 2);
    assert_eq!(count, 4);
    assert!(remaining > 0);
    assert_eq!([buffer[1], buffer[3], buffer[5]], [123; 3]);
}

#[test]
fn invalid_sampling_configuration_leaves_state_intact() {
    let mut sid = sid();
    let mut control = sid.clone();
    assert!(!sid.set_sampling_parameters_with_options(
        SamplingMethod::Resample,
        0.0,
        48000.0,
        21600.0,
        0.97
    ));
    assert!(!sid.set_sampling_parameters_with_options(
        SamplingMethod::Resample,
        985248.0,
        48000.0,
        24000.0,
        0.97
    ));
    let mut a = [0_i16; 5000];
    let mut b = a;
    assert_eq!(
        sid.sample(80000, &mut a, 1),
        control.sample(80000, &mut b, 1)
    );
    assert_eq!(a, b);
}

#[test]
fn all_sampling_modes_preserve_batching_and_stride() {
    for method in [
        SamplingMethod::Fast,
        SamplingMethod::Interpolate,
        SamplingMethod::Resample,
        SamplingMethod::ResampleFast,
    ] {
        let mut whole = sid();
        assert!(
            whole.set_sampling_parameters_with_options(method, 985248.0, 48000.0, 21600.0, 0.97)
        );
        let mut split = whole.clone();
        let mut expected = [0_i16; 5000];
        let (length, remaining) = whole.sample(20000, &mut expected, 1);
        assert_eq!(remaining, 0);
        let mut actual = Vec::new();
        let mut remaining = 20000;
        let mut buffer = [123_i16; 7];
        while remaining > 0 {
            let delta = remaining.min(137);
            let mut unconsumed = delta;
            while unconsumed > 0 {
                let (count, rest) = split.sample(unconsumed, &mut buffer, 2);
                assert!(rest < unconsumed || count != 0);
                assert!(count <= 4, "{:?}", method);
                assert_eq!([buffer[1], buffer[3], buffer[5]], [123; 3]);
                actual.extend((0..count).map(|i| buffer[i * 2]));
                unconsumed = rest;
            }
            remaining -= delta;
        }
        assert_eq!(&expected[..length], actual, "{method:?}");
    }
}

#[test]
fn sampled_bus_fades_and_readable_registers_refresh_it() {
    for (model, ttl) in [(ChipModel::Mos6581, 0x1d00), (ChipModel::Mos8580, 0xa2000)] {
        let mut sid = Sid::new(model);
        sid.write(0, 0x55);
        assert_eq!(sid.read(1), 0x55);
        let mut buffer = [0_i16; 5000];
        let mut remaining = ttl - 1;
        while remaining > 0 {
            remaining = sid.sample(remaining, &mut buffer, 1).1;
        }
        assert_eq!(sid.read(1), 0x55);
        sid.sample(1, &mut buffer, 1);
        assert_eq!(sid.read(1), 0);
        assert_eq!(sid.read(0x19), 0xff);
        assert_eq!(sid.read(0), 0xff);
    }
}

#[test]
fn digital_shadow_matches_full_oscillator_and_envelope_readback() {
    let mut audio = sid();
    let mut shadow = audio.clone();
    for (reg, value, cycles) in [
        (0x12, 0x11, 1),
        (0x13, 0x00, 89),
        (0x14, 0xf0, 300),
        (0x0e, 0xff, 1789),
        (0x0f, 0x91, 20),
        (0x12, 0x89, 4100),
        (0x12, 0x80, 20000),
    ] {
        audio.write(reg, value);
        shadow.write(reg, value);
        audio.clock_delta(cycles);
        shadow.clock_readback(cycles);
        assert_eq!(audio.read(0x1b), shadow.read(0x1b));
        assert_eq!(audio.read(0x1c), shadow.read(0x1c));
    }
}

#[test]
fn complete_state_restore_replays_fir_pcm() {
    let mut sid = sid();
    let mut buffer = [0_i16; 5000];
    sid.sample(18000, &mut buffer, 1);
    let state = sid.read_state();
    let mut expected = buffer;
    let result = sid.sample(40000, &mut expected, 1);
    sid.write_state(&state);
    let mut actual = buffer;
    assert_eq!(sid.sample(40000, &mut actual, 1), result);
    assert_eq!(actual, expected);
}

#[test]
fn complete_saw_noise_release_matches_cpp_references_within_eight_lsb() {
    let mut sid = Sid::new(ChipModel::Mos6581);
    sid.set_voice_mask(7);
    sid.enable_filter(false);
    sid.adjust_filter_bias(0.5);
    sid.enable_external_filter(true);
    assert!(sid.set_sampling_parameters_with_options(
        SamplingMethod::Resample,
        985248.0,
        44100.0,
        19845.0,
        0.97
    ));
    for (reg, value) in [
        (0, (7493 & 255) as u8),
        (1, (7493 >> 8) as u8),
        (5, 9),
        (6, 0xf0),
        (0x18, 15),
        (4, 0x21),
    ] {
        sid.write(reg, value);
    }
    let mut pcm = Vec::new();
    let mut buffer = [0_i16; 4096];
    for frame in 0..80 {
        if frame == 40 {
            sid.write(4, 0x81);
        }
        if frame == 60 {
            sid.write(4, 0x80);
        }
        let (count, remaining) = sid.sample(19656, &mut buffer, 1);
        assert_eq!(remaining, 0);
        pcm.extend_from_slice(&buffer[..count]);
    }
    for (label, reference) in [
        (
            "C++ native",
            include_bytes!("data/trx64_native.s16le").as_slice(),
        ),
        (
            "C++ WASM",
            include_bytes!("data/c64re_wasm.s16le").as_slice(),
        ),
    ] {
        assert_eq!(pcm.len() * 2, reference.len());
        let mut maximum = 0;
        let mut squared = 0_f64;
        let mut differing = 0;
        for (&sample, pair) in pcm.iter().zip(reference.chunks_exact(2)) {
            let delta = (sample as i32 - i16::from_le_bytes([pair[0], pair[1]]) as i32).abs();
            maximum = maximum.max(delta);
            squared += f64::from(delta * delta);
            differing += usize::from(delta != 0);
        }
        let rms = (squared / pcm.len() as f64).sqrt();
        println!(
            "{label}: samples={} max_lsb={maximum} rms_lsb={rms:.6} differing={differing}",
            pcm.len()
        );
        assert!(maximum <= 8, "{}: {} LSB", label, maximum);
    }
}

#[test]
fn digital_shadow_sample_counts_match_real_sampler_at_boundaries() {
    for model in [ChipModel::Mos6581, ChipModel::Mos8580] {
        for method in [
            SamplingMethod::Fast,
            SamplingMethod::Interpolate,
            SamplingMethod::Resample,
            SamplingMethod::ResampleFast,
        ] {
            let mut full = Sid::new(model);
            assert!(
                full.set_sampling_parameters_with_options(method, 985248.0, 48000.0, 21600.0, 0.97)
            );
            let mut shadow = full.clone();
            let mut buffer = [0_i16; 5000];
            for cycles in [0, 1, 20, 1, 22, 1, 123, 19656, 77777, 1, 1, 1, 441, 2048] {
                let (count, rest) = full.sample(cycles, &mut buffer, 1);
                assert_eq!(rest, 0);
                assert_eq!(
                    shadow.clock_readback_sampled(cycles),
                    count,
                    "{:?} cycles={}",
                    method,
                    cycles
                );
                assert_eq!(shadow.read(0x1b), full.read(0x1b));
                assert_eq!(shadow.read(0x1c), full.read(0x1c));
            }
        }
    }
}
