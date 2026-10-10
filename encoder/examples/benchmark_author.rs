//! Reproducible author-planning benchmark on real, already resized RGB video.
//! cargo run --release --features gpu --example benchmark_author -- source.rgb 12 83
use mupic_encoder::author_dither::AuthorDither;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    time::Instant,
};
const PALETTE: [[u8; 3]; 16] = [
    [0, 0, 0],
    [247, 247, 247],
    [141, 47, 52],
    [106, 212, 205],
    [152, 53, 164],
    [76, 180, 66],
    [44, 41, 177],
    [239, 239, 93],
    [152, 78, 32],
    [91, 56, 0],
    [209, 103, 109],
    [74, 74, 74],
    [123, 123, 123],
    [159, 239, 147],
    [109, 106, 239],
    [178, 178, 178],
];
const FRAME: usize = 360 * 200 * 3;
fn output(dither: &mut AuthorDither, rgb: &[u8]) -> String {
    let mut hash = Sha256::new();
    for frame in rgb.chunks_exact(FRAME) {
        let plans = dither.frame_plans(frame);
        let pixels: Vec<_> = plans
            .iter()
            .enumerate()
            .map(|(i, &p)| dither.colour(p, i % 360, i / 360))
            .collect();
        hash.update(pixels);
    }
    format!("{:x}", hash.finalize())
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let source = args.get(1).expect("source.rgb path required");
    let frames: usize = args.get(2).map_or(12, |s| s.parse().unwrap());
    let stride: usize = args.get(3).map_or(83, |s| s.parse().unwrap());
    let mut file = File::open(source).unwrap();
    let mut rgb = Vec::with_capacity(frames * FRAME);
    for index in 0..frames {
        file.seek(SeekFrom::Start((index * stride * FRAME) as u64))
            .unwrap();
        let begin = rgb.len();
        rgb.resize(begin + FRAME, 0);
        file.read_exact(&mut rgb[begin..]).unwrap();
    }
    let mut data = AuthorDither::new(&PALETTE, true).unwrap();
    let unique = data.missing_rgb_keys(&rgb).len();
    let cpu_threads = std::thread::available_parallelism().map_or(1, usize::from);
    #[cfg(feature = "gpu")]
    let gpu = {
        let begin = Instant::now();
        let planner = match mupic_encoder::gpu::GpuPlanner::new(&data) {
            Ok(planner) => Some(planner),
            Err(error) => {
                eprintln!("GPU unavailable: {error}");
                None
            }
        };
        if let Some(p) = &planner {
            eprintln!(
                "GPU {} initialized in {:.3}s",
                p.adapter_name,
                begin.elapsed().as_secs_f64()
            );
        }
        planner
    };
    let mut reference = String::new();
    println!("{{\"frames\":{frames},\"source_sha256\":\"{:x}\",\"unique_rgb\":{unique},\"threads\":{cpu_threads},\"results\":[",Sha256::digest(&rgb));
    let mut first = true;
    for mode in 0..3 {
        if args.get(4).is_some_and(|filter| filter == "parallel") && mode == 0 {
            continue;
        }
        if args.get(4).is_some_and(|filter| filter == "gpu") && mode != 2 {
            continue;
        }
        #[cfg(not(feature = "gpu"))]
        if mode == 2 {
            continue;
        }
        #[cfg(feature = "gpu")]
        if mode == 2 && gpu.is_none() {
            continue;
        }
        for repetition in 0..3 {
            data = AuthorDither::new(&PALETTE, true).unwrap();
            data.set_threads(if mode == 0 { 1 } else { cpu_threads });
            let begin = Instant::now();
            #[cfg(feature = "gpu")]
            if mode == 2 {
                gpu.as_ref().unwrap().preplan(&mut data, &rgb).unwrap();
            } else {
                data.preplan_rgb(&rgb);
            }
            #[cfg(not(feature = "gpu"))]
            data.preplan_rgb(&rgb);
            let seconds = begin.elapsed().as_secs_f64();
            let hash = output(&mut data, &rgb);
            if reference.is_empty() {
                reference = hash.clone();
            }
            assert_eq!(hash, reference, "Hardware output changed!");
            if !first {
                println!(",");
            }
            first = false;
            print!("{{\"mode\":\"{}\",\"run\":{repetition},\"planning_seconds\":{seconds},\"hardware_pixels_sha256\":\"{hash}\"}}",["cpu1","cpuN","gpu+cpuN"][mode]);
        }
    }
    println!("]}}");
}
