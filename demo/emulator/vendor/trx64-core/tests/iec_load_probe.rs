//! Byte-exact IEC directory-load regression for the pinned upstream disk corpus.

use std::path::Path;
use trx64_core::drive::{DiskImage, DiskKind};
use trx64_core::{Machine, NullSink};

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../C64ReverseEngineeringMCP/resources/roms");
const SAMPLE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../C64ReverseEngineeringMCP/samples/scramble_infinity.d64");

fn roms_present() -> bool {
    let d = Path::new(ROM_DIR);
    d.join("kernal-901227-03.bin").exists()
        && (d.join("dos1541-325302-01+901229-05.bin").exists() || d.join("1541.bin").exists())
}

/// Byte-exact directory-load verification mirroring the corpus
/// `disk/disk-load-dir.json` sequence (mount-before-boot, 2M+2M boot, type
/// LOAD"$",8 via the keyboard matrix with the corpus hold/gap cycles, 4×2M run),
/// then compares the first 32 directory bytes + vartab ($2D/$2E) + ST ($90) to the golden values
/// extracted from `disk-load-dir.golden.json`.
#[test]
#[ignore = "byte-exact directory-load check; run explicitly with --ignored"]
fn iec_load_dir_byteexact() {
    if !roms_present() {
        eprintln!("skip: ROMs absent");
        return;
    }
    let d64 = match std::fs::read(SAMPLE) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("skip: sample disk absent");
            return;
        }
    };
    let mut m = Machine::new();
    m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
    // Corpus order: mount at cycle 0 (before any run).
    m.drive8.attach_disk(DiskImage {
        kind: DiskKind::D64,
        bytes: d64.clone(),
        backing_path: Some(SAMPLE.to_string()),
        read_only: false,
    });
    let mut sink = NullSink;
    // boot-1 + boot-2 (2M + 2M).
    m.run_for_full(2_000_000, &mut sink, |_, _, _, _, _, _, _| {});
    m.run_for_full(2_000_000, &mut sink, |_, _, _, _, _, _, _| {});
    // type-load-dir: LOAD"$",8\r via the keyboard matrix, corpus hold/gap = 80000.
    m.keyboard
        .type_text(m.cpu6510.clk, "LOAD\"$\",8\r", 80_000, 80_000);
    // load-1..load-4 (4 × 2M).
    for _ in 0..4 {
        m.run_for_full(2_000_000, &mut sink, |_, _, _, _, _, _, _| {});
    }

    // Golden values from disk-load-dir.golden.json.
    let golden_st: u8 = 0x40;
    let golden_vartab: [u8; 2] = [127, 10]; // $0A7F
    let golden_dir: [u8; 32] = [
        31, 8, 0, 0, 18, 34, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 32, 34,
        32, 32, 32, 32, 32, 32, 0, 63, 8,
    ];
    let st = m.read_full(0x0090);
    let vartab = [m.read_full(0x002D), m.read_full(0x002E)];
    let dir: Vec<u8> = (0x0801..0x0801 + 640).map(|a| m.read_full(a)).collect();
    let first_mismatch = dir
        .iter()
        .take(32)
        .zip(golden_dir.iter())
        .position(|(a, b)| a != b);
    assert_eq!(st, golden_st, "status $90 mismatch");
    assert_eq!(vartab, golden_vartab, "vartab $2D/$2E mismatch");
    assert!(
        first_mismatch.is_none(),
        "directory first-32 byte mismatch at idx {:?}",
        first_mismatch
    );
}
