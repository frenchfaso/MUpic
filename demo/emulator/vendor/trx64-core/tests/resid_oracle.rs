//! Rust SID adapter versus the pinned native C++ and C++ WASM PCM references.
//! The 6581 saw/noise/release sequence covers this signal path only; the separate
//! resid-rs analog-stage oracle covers both filter models. Neither proves full-song parity.
use trx64_core::resid_audio::{SidAudioEngine, SidWriteRecord, WavFormat};
use trx64_core::resid_ffi::ResidConfig;

fn stream() -> Vec<SidWriteRecord> {
    let mut records = Vec::new();
    for (reg, value) in [
        (0, (7493 & 255) as u8),
        (1, (7493 >> 8) as u8),
        (5, 9),
        (6, 0xf0),
        (0x18, 15),
        (4, 0x21),
    ] {
        records.push(SidWriteRecord::write(reg, value));
    }
    for frame in 0..80 {
        if frame == 40 {
            records.push(SidWriteRecord::write(4, 0x81));
        }
        if frame == 60 {
            records.push(SidWriteRecord::write(4, 0x80));
        }
        records.push(SidWriteRecord::boundary(19656));
    }
    records
}
fn run() -> Vec<i16> {
    let mut engine = SidAudioEngine::new(ResidConfig::default());
    let produced = engine.run_stream(&stream());
    assert_eq!(produced, engine.pcm().len());
    engine.take_pcm()
}
#[test]
fn rust_pcm_matches_pinned_cpp_references_within_eight_lsb() {
    let pcm = run();
    for (label, bytes) in [
        (
            "native C++",
            include_bytes!("fixtures/resid/trx64_native.saw_noise_release.pcm.s16le").as_slice(),
        ),
        (
            "WASM C++",
            include_bytes!("fixtures/resid/c64re_wasm.saw_noise_release.pcm.s16le").as_slice(),
        ),
    ] {
        assert_eq!(pcm.len() * 2, bytes.len());
        let mut maximum = 0_i32;
        let mut squared = 0_f64;
        for (&sample, pair) in pcm.iter().zip(bytes.chunks_exact(2)) {
            let delta = (sample as i32 - i16::from_le_bytes([pair[0], pair[1]]) as i32).abs();
            maximum = maximum.max(delta);
            squared += f64::from(delta * delta);
        }
        println!(
            "Rust vs {label}: samples={} max_lsb={maximum} rms_lsb={:.6}",
            pcm.len(),
            (squared / pcm.len() as f64).sqrt()
        );
        assert!(maximum <= 8, "{label}: {maximum} LSB");
    }
}
#[test]
fn independent_rust_instances_are_bit_exact() {
    let a = run();
    let b = run();
    assert!(a.len() > 1000);
    assert_eq!(a, b);
}
#[test]
fn rust_snapshot_roundtrips_and_resumes_pcm() {
    let mut engine = SidAudioEngine::new(ResidConfig::default());
    engine.run_stream(&stream()[..16]);
    engine.take_pcm();
    let saved = engine.capture_state();
    engine.run_stream(&[SidWriteRecord::boundary(90000)]);
    let expected = engine.take_pcm();
    engine.restore_state(&saved);
    assert_eq!(engine.capture_state(), saved);
    engine.run_stream(&[SidWriteRecord::boundary(90000)]);
    assert_eq!(engine.take_pcm(), expected);
}
#[test]
fn wav_export_stereo_payload_and_header_are_wellformed() {
    let mut engine = SidAudioEngine::new(ResidConfig::default());
    engine.run_stream(&stream());
    let wav = engine.export_wav(WavFormat::default());
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(wav.len(), 44 + engine.pcm().len() * 4);
    assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 2);
    for (pair, &sample) in wav[44..].chunks_exact(4).zip(engine.pcm()) {
        assert_eq!(&pair[..2], &sample.to_le_bytes());
        assert_eq!(&pair[..2], &pair[2..]);
    }
}
