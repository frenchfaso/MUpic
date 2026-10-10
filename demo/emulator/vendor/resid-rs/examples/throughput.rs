//! Native diagnostic only; browser realtime must be measured in the actual browser.
use resid::{ChipModel, SamplingMethod, Sid};
use std::time::Instant;
fn main() {
    for (model, model_name) in [(ChipModel::Mos6581, "6581"), (ChipModel::Mos8580, "8580")] {
        for method in [SamplingMethod::Interpolate, SamplingMethod::Resample] {
            let mut sid = Sid::new(model);
            sid.adjust_filter_bias(0.5);
            sid.set_voice_mask(7);
            assert!(
                sid.set_sampling_parameters_with_options(method, 985248.0, 48000.0, 21600.0, 0.97)
            );
            for voice in 0..3 {
                for (offset, value) in [
                    (0, 0xb1),
                    (1, 0x19 + voice * 7),
                    (2, 0),
                    (3, 8),
                    (5, 4),
                    (6, 0xf7),
                    (4, [0x21, 0x41, 0x11][voice as usize]),
                ] {
                    sid.write(voice * 7 + offset, value);
                }
            }
            for (reg, val) in [(0x15, 7), (0x16, 100), (0x17, 0xc7), (0x18, 0x1f)] {
                sid.write(reg, val);
            }
            let mut buffer = [0_i16; 4096];
            sid.sample(19656, &mut buffer, 1);
            let start = Instant::now();
            let mut samples = 0;
            for _ in 0..100 {
                let (count, rest) = sid.sample(19656, &mut buffer, 1);
                assert_eq!(rest, 0);
                samples += count;
                std::hint::black_box(&buffer[..count]);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let emulated = 1965600.0 / 985248.0;
            println!("model={model_name} method={method:?} emulated_seconds={emulated:.6} wall_seconds={elapsed:.6} realtime_percent={:.2} samples={samples}", emulated / elapsed * 100.0);
        }
    }
}
