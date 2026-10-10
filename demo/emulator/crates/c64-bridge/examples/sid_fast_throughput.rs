//! Native comparison of the browser SID consumer; not a browser realtime claim.
use c64_bridge::{SidEvent, SidRenderer};
use std::{hint::black_box, time::Instant};

const FRAME_CYCLES: u64 = 19_656;
const FRAMES: u64 = 100;
const ROUNDS: usize = 7;

fn setup(receivers: u16, model: u32) -> Vec<SidEvent> {
    let mut events = Vec::new();
    for receiver in 0..receivers {
        let mask = 1 << (receiver + 1);
        events.push(SidEvent::new(SidEvent::ENSURE, 0, mask, model, 0));
        let (left, right) = [(60, 15), (15, 60), (30, 30)][receiver as usize];
        events.push(SidEvent::new(SidEvent::GAINS, 0, mask, left | right << 16, 0));
        for voice in 0..3 {
            for (offset, value) in [(0, 0xb1), (1, 0x19 + voice * 7), (2, 0), (3, 8),
                (5, 4), (6, 0xf7), (4, [0x21, 0x41, 0x11][voice as usize])] {
                events.push(SidEvent::new(SidEvent::WRITE, (voice * 7 + offset) as u8, mask, value, 0));
            }
        }
        for (reg, val) in [(0x15, 7), (0x16, 100), (0x17, 0xc7), (0x18, 0x1f)] {
            events.push(SidEvent::new(SidEvent::WRITE, reg, mask, val, 0));
        }
    }
    events
}

fn measure(fast: bool, receivers: u16, model: u32, rate: u32) -> (f64, f64, usize, i32) {
    let mut renderer = if fast { SidRenderer::new_fast(rate) } else { SidRenderer::new(rate) };
    let mut pcm = Vec::new();
    let start = Instant::now();
    renderer.render_into(&setup(receivers, model), &mut pcm).unwrap();
    let init = start.elapsed().as_secs_f64();
    renderer.render_into(&[SidEvent::new(SidEvent::CLOCK, 0, 0, 0, FRAME_CYCLES)], &mut pcm).unwrap();
    let start = Instant::now();
    let mut samples = 0;
    for frame in 2..=FRAMES + 1 {
        let at = frame * FRAME_CYCLES;
        let events = [SidEvent::new(SidEvent::WRITE, 0x16, 1 << 1, 70 + frame as u32 % 50, at - 997),
            SidEvent::new(SidEvent::CLOCK, 0, 0, 0, at)];
        renderer.render_into(&events, &mut pcm).unwrap();
        samples += pcm.len();
        black_box(&pcm);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let peak = pcm.iter().map(|&sample| i32::from(sample).abs()).max().unwrap_or(0);
    assert!(peak > 0);
    (elapsed, init, samples, peak)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() {
    println!("{{\"kind\":\"native-sid-consumer-comparison\",\"browser_realtime_claim\":false,\"rounds\":{ROUNDS},\"emulated_seconds_per_round\":{},\"cases\":[", FRAME_CYCLES as f64 * FRAMES as f64 / 985_248.0);
    let mut first = true;
    for receivers in [1, 3] {
        for model in [0, 1] {
            for rate in [44_100, 48_000, 96_000] {
                let mut accurate = Vec::new();
                let mut fast = Vec::new();
                let mut accurate_init = Vec::new();
                let mut fast_init = Vec::new();
                let mut sample_count = 0;
                let mut peak = 0;
                for round in 0..ROUNDS {
                    // Alternate order to reduce systematic first-run/thermal bias.
                    for light in if round % 2 == 0 { [false, true] } else { [true, false] } {
                        let (wall, init, samples, measured_peak) = measure(light, receivers, model, rate);
                        if sample_count != 0 { assert_eq!(sample_count, samples); }
                        sample_count = samples;
                        if light { fast.push(wall); fast_init.push(init); peak = measured_peak; }
                        else { accurate.push(wall); accurate_init.push(init); }
                    }
                }
                let accurate_median = median(&mut accurate);
                let fast_median = median(&mut fast);
                if !first { println!(","); }
                first = false;
                print!("{{\"receivers\":{receivers},\"model\":{},\"sample_rate\":{rate},\"stereo_samples\":{sample_count},\"fast_peak\":{peak},\"resample_wall_seconds\":{accurate:?},\"interpolate_wall_seconds\":{fast:?},\"resample_median_seconds\":{accurate_median},\"interpolate_median_seconds\":{fast_median},\"sid_wall_reduction_percent\":{},\"resample_init_median_seconds\":{},\"interpolate_init_median_seconds\":{}}}",
                    if model == 0 { 6581 } else { 8580 }, (1.0 - fast_median / accurate_median) * 100.0,
                    median(&mut accurate_init), median(&mut fast_init));
            }
        }
    }
    println!("]}}");
}
