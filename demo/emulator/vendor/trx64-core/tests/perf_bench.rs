//! perf_bench.rs — TRX64 Rust-core throughput benchmark (vs the c64re TS core).
//!
//! This measures CORE EMULATION THROUGHPUT only: the time to execute a FIXED
//! C64-cycle budget of steady-state emulation, with ROM-load / disk-mount /
//! key-injection / rendering all EXCLUDED from the timed region. It is the Rust
//! half of the cross-core comparison; the TS half lives in
//! `bench/c64re_bench.ts` and runs the IDENTICAL workload on the c64re
//! IntegratedSession. See `docs/perf-compare.md` for the methodology + results.
//!
//! These tests are `#[ignore]` so a normal `cargo test` never runs them. They
//! ONLY produce a defensible number when built `--release` (debug is ~10x slower
//! and meaningless).
//!
//! Run (release, median of K runs printed):
//!   rtk cargo test -p trx64-core --release --test perf_bench -- --ignored --nocapture
//!
//! Single workload:
//!   cargo test -p trx64-core --release --test perf_bench bench_pure_headless -- --ignored --nocapture
//!   cargo test -p trx64-core --release --test perf_bench bench_disk_workload -- --ignored --nocapture

use std::path::Path;
use std::time::Instant;
use trx64_core::drive::{DiskImage, DiskKind};
use trx64_core::{Machine, NullSink};

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../C64ReverseEngineeringMCP/resources/roms");
const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../C64ReverseEngineeringMCP/samples");

/// C64 PAL master clock (MHz). 985248.444 Hz → 0.985248 MHz.
const PAL_HZ: f64 = 985_248.444;

/// Pure-headless steady-state cycle budget: 100M C64 cycles (~101.5s of PAL
/// wall-clock emulated per run). Large enough to dwarf any fixed overhead and
/// to amortize cache effects. Override with TRX64_PURE_BUDGET to run the SAME
/// reduced budget the TS bench uses (the TS core is ~200x slower, so the
/// cross-core ratio is computed on a smaller identical budget — see
/// docs/perf-compare.md).
const PURE_BUDGET_DEFAULT: u64 = 100_000_000;
const DISK_BUDGET_DEFAULT: u64 = 35_000_000;

/// Number of timed runs per workload; we report min / median / max.
const K_RUNS: usize = 7;

fn env_budget(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default)
}

fn roms_present() -> bool {
    let d = Path::new(ROM_DIR);
    d.join("kernal-901227-03.bin").exists()
        && d.join("basic-901226-01.bin").exists()
        && d.join("chargen-901225-01.bin").exists()
}

fn inject_keys(m: &mut Machine, s: &[u8]) {
    for (i, b) in s.iter().enumerate() {
        m.poke(0x0277 + i as u16, &[*b]);
    }
    m.poke(0x00c6, &[s.len() as u8]);
}

/// Median of an odd-or-even slice (lower-middle for even N).
fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

struct RunStats {
    budget_cycles: u64,
    secs: Vec<f64>,
}

impl RunStats {
    fn report(&self, label: &str) {
        let mut s = self.secs.clone();
        let min = s.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = s.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let med = median(&mut s);
        let mhz = |secs: f64| (self.budget_cycles as f64) / secs / 1_000_000.0;
        let rt = |secs: f64| (self.budget_cycles as f64) / secs / PAL_HZ;
        eprintln!("\n========== {label} ==========");
        eprintln!(
            "  budget = {} C64 cycles, K = {} timed runs",
            self.budget_cycles,
            self.secs.len()
        );
        eprintln!("  wall-clock  min/median/max : {min:.4} / {med:.4} / {max:.4} s");
        eprintln!(
            "  emulated MHz (median)          : {:.3} MHz  ({:.4} / {:.4} / {:.4} min/med/max)",
            mhz(med),
            mhz(max),
            mhz(med),
            mhz(min)
        );
        eprintln!(
            "  real-time multiple (median)    : {:.1}x  (median {:.3} MHz / {:.6} MHz PAL)",
            rt(med),
            mhz(med),
            PAL_HZ / 1_000_000.0
        );
        eprintln!(
            "  RAW (machine-parseable): {label} budget={} k={} min_s={:.6} med_s={:.6} max_s={:.6} med_mhz={:.4} med_rtx={:.3}",
            self.budget_cycles,
            self.secs.len(),
            min,
            med,
            max,
            mhz(med),
            rt(med)
        );
    }
}

// ── Workload 1: pure headless steady-state main-machine throughput ──────────
//
// Boot to BASIC (UNtimed), then time a FIXED 100M-cycle steady-state run of the
// main machine: CPU + VIC + CIA1/CIA2 + SID, no drive activity. This isolates
// the C64-core throughput (the realistic "sitting at the READY prompt with the
// cursor blinking + the IRQ running" loop, which is the cleanest apples-to-apples
// steady-state path that BOTH cores execute identically).
#[test]
#[ignore = "perf benchmark; run --release with --ignored --nocapture"]
fn bench_pure_headless() {
    if !roms_present() {
        eprintln!("skip bench_pure_headless: ROMs absent at {ROM_DIR}");
        return;
    }
    let pure_budget = env_budget("TRX64_PURE_BUDGET", PURE_BUDGET_DEFAULT);
    let mut secs = Vec::with_capacity(K_RUNS);

    for run in 0..K_RUNS {
        // ── SETUP (UNTIMED): construct + boot to BASIC READY ──────────────
        let mut m = Machine::new();
        m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
        let mut sink = NullSink;
        // Boot well past READY so the timed region is pure steady state.
        m.run_for_full(3_000_000, &mut sink, |_, _, _, _, _, _, _| {});

        // ── TIMED REGION: a FIXED 100M-cycle steady-state run ─────────────
        // Drive the run in fixed chunks with a generous per-chunk instruction
        // cap that can NEVER trip before the cycle budget (so cycles, not
        // instructions, bound the loop — matching the cross-core contract).
        let start_clk = m.c64_core.clk;
        let chunk = 1_000_000u64;
        // worst case ~1 cycle/instr (impossible on 6502, min is 2) → cap = chunk
        // is already unreachable; use chunk*2 for absolute safety.
        let inst_cap = chunk * 2;
        let t0 = Instant::now();
        let mut done = 0u64;
        while done < pure_budget {
            m.run_for_full_capped(chunk, inst_cap, &mut sink, |_, _, _, _, _, _, _| {});
            done += chunk;
        }
        let elapsed = t0.elapsed().as_secs_f64();
        let executed = m.c64_core.clk.wrapping_sub(start_clk);
        // Sanity: we executed at least the budget (chunks are exact multiples).
        assert!(
            executed >= pure_budget,
            "executed {executed} < budget {pure_budget}"
        );
        secs.push(elapsed);
        eprintln!(
            "  [pure run {}/{}] {:.4}s  executed={} cyc  final_pc=${:04X}",
            run + 1,
            K_RUNS,
            elapsed,
            executed,
            m.c64_core.reg_pc
        );
    }

    RunStats {
        budget_cycles: pure_budget,
        secs,
    }
    .report("WORKLOAD 1 — pure headless (CPU+VIC+CIA+SID, no drive)");
}

// ── Workload 1b: cpuhistory-ring overhead (ring ON vs OFF) ──────────────────
//
// reverse-debug Phase 1a perf gate. The always-on CPU-history ring pushes one
// record per retired instruction on the ~1 MHz hot path; this must be negligible.
// Runs the SAME pure-headless steady-state workload TWICE on identical machines —
// once with the ring ENABLED (the shipped default) and once with it DISABLED (the
// `TRX64_CPUHISTORY=0` kill-switch, via `set_enabled(false)`) — and reports the
// cycles/sec delta. K medians each; the delta is the cost of `CpuHistoryRing::push`.
#[test]
#[ignore = "perf benchmark; run --release with --ignored --nocapture"]
fn bench_cpuhistory_ring_overhead() {
    if !roms_present() {
        eprintln!("skip bench_cpuhistory_ring_overhead: ROMs absent at {ROM_DIR}");
        return;
    }
    // Default 2M cycles/run (the task's "free-run ~2M cycles" measure); override with
    // TRX64_PURE_BUDGET to match the larger pure bench.
    let budget = env_budget("TRX64_PURE_BUDGET", 2_000_000);

    let run_once = |ring_on: bool| -> f64 {
        let mut m = Machine::new();
        m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
        // Toggle BOTH always-on rings AFTER construction (the env is read at new());
        // explicit so the bench is deterministic regardless of the ambient
        // TRX64_CPUHISTORY. reverse-debug Phase 1b: this now measures the FULL hot-path
        // cost = the CPU-history ring (Phase 1a) + the full-delta undo ring (Phase 1b,
        // per-instruction begin/commit + per-write record_write). The kill-switch gates
        // both together in production, so the bench gates both together too.
        m.cpu_history.set_enabled(ring_on);
        m.delta_ring.set_enabled(ring_on);
        let mut sink = NullSink;
        m.run_for_full(3_000_000, &mut sink, |_, _, _, _, _, _, _| {});
        let chunk = 500_000u64;
        let t0 = Instant::now();
        let mut done = 0u64;
        while done < budget {
            m.run_for_full_capped(chunk, chunk * 2, &mut sink, |_, _, _, _, _, _, _| {});
            done += chunk;
        }
        let secs = t0.elapsed().as_secs_f64();
        // Sanity: the ON run actually recorded into BOTH rings; OFF recorded nothing.
        if ring_on {
            assert!(m.cpu_history.len() > 0, "cpu-history ring ON but recorded nothing");
            assert!(m.delta_ring.len() > 0, "delta ring ON but recorded nothing");
        } else {
            assert_eq!(m.cpu_history.len(), 0, "cpu-history ring OFF but recorded something");
            assert_eq!(m.delta_ring.len(), 0, "delta ring OFF but recorded something");
        }
        secs
    };

    let mut on = Vec::with_capacity(K_RUNS);
    let mut off = Vec::with_capacity(K_RUNS);
    for _ in 0..K_RUNS {
        // Interleave ON/OFF so thermal/scheduler drift hits both equally.
        on.push(run_once(true));
        off.push(run_once(false));
    }
    let med_on = median(&mut on.clone());
    let med_off = median(&mut off.clone());
    let mhz = |s: f64| (budget as f64) / s / 1_000_000.0;
    let delta_pct = (med_on - med_off) / med_off * 100.0;
    eprintln!("\n========== WORKLOAD 1b — cpuhistory-ring overhead (ON vs OFF) ==========");
    eprintln!("  budget = {budget} C64 cycles/run, K = {K_RUNS} timed runs each");
    eprintln!("  ring OFF (kill-switch)  median : {med_off:.4} s  ({:.3} MHz)", mhz(med_off));
    eprintln!("  ring ON  (shipped)      median : {med_on:.4} s  ({:.3} MHz)", mhz(med_on));
    eprintln!("  DELTA (ON vs OFF)              : {delta_pct:+.2}%  (negative = ON faster = within noise)");
    eprintln!(
        "  RAW: cpuhistory_overhead budget={budget} k={K_RUNS} off_med_s={med_off:.6} on_med_s={med_on:.6} delta_pct={delta_pct:.3}"
    );
}

// ── Workload 2: full-system disk workload (scramble_infinity.d64) ───────────
//
// The realistic "running a game" path: boot + mount the D64 + LOAD"*",8,1 + RUN,
// then run to ~the title. This exercises the cross-domain drive + IEC + GCR +
// the 1:1 viacore — the expensive parts. We time ONLY the post-mount cycle run
// (mount + key-inject are part of setup but cheap; the disk LOAD itself is the
// realistic workload and IS timed). Render is excluded from the timed region.
#[test]
#[ignore = "perf benchmark; run --release with --ignored --nocapture"]
fn bench_disk_workload() {
    if !roms_present() {
        eprintln!("skip bench_disk_workload: ROMs absent at {ROM_DIR}");
        return;
    }
    let disk_path = format!("{SAMPLES}/scramble_infinity.d64");
    let disk_bytes = match std::fs::read(&disk_path) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("skip bench_disk_workload: sample absent ({disk_path})");
            return;
        }
    };

    // Fixed cross-domain cycle budget: 35M C64 cycles after RUN. Per the
    // seven_game_gate notes, scramble's standard-KERNAL serial load needs ~30M
    // cycles to bring in the BASIC stub before the fastloader installs, so 35M
    // lands us in/around the title — a representative drive-heavy slice.
    // Override with TRX64_DISK_BUDGET for the reduced cross-core ratio budget.
    let disk_budget = env_budget("TRX64_DISK_BUDGET", DISK_BUDGET_DEFAULT);
    let mut secs = Vec::with_capacity(K_RUNS);

    for run in 0..K_RUNS {
        // ── SETUP (UNTIMED): construct + boot + mount + settle + inject ───
        let mut m = Machine::new();
        m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
        let mut sink = NullSink;
        m.run_for_full(2_500_000, &mut sink, |_, _, _, _, _, _, _| {});
        m.drive8.attach_disk(DiskImage {
            kind: DiskKind::D64,
            bytes: disk_bytes.clone(),
            backing_path: Some(disk_path.clone()),
            read_only: false,
        });
        m.run_for_full(800_000, &mut sink, |_, _, _, _, _, _, _| {});
        inject_keys(&mut m, b"LOAD\"*\",8,1\r");
        // Let the LOAD command be parsed + the load begin (still setup — we want
        // the timed region to be the steady cross-domain load/run, not the
        // editor parsing a line). Small, fixed, and identical on both cores.
        m.run_for_full(500_000, &mut sink, |_, _, _, _, _, _, _| {});
        inject_keys(&mut m, b"RUN\r");

        // ── TIMED REGION: the cross-domain LOAD + RUN cycle run ───────────
        let start_clk = m.c64_core.clk;
        let chunk = 500_000u64;
        let inst_cap = chunk * 2;
        let t0 = Instant::now();
        let mut done = 0u64;
        while done < disk_budget {
            m.run_for_full_capped(chunk, inst_cap, &mut sink, |_, _, _, _, _, _, _| {});
            done += chunk;
        }
        let elapsed = t0.elapsed().as_secs_f64();
        let executed = m.c64_core.clk.wrapping_sub(start_clk);
        assert!(executed >= disk_budget);
        secs.push(elapsed);
        eprintln!(
            "  [disk run {}/{}] {:.4}s  executed={} cyc  sync_found={} head={}  final_pc=${:04X}",
            run + 1,
            K_RUNS,
            elapsed,
            executed,
            m.drive8.rotation.sync_found(),
            m.drive8.rotation.gcr_head_offset,
            m.c64_core.reg_pc
        );
    }

    RunStats {
        budget_cycles: disk_budget,
        secs,
    }
    .report("WORKLOAD 2 — full-system disk (boot+mount+LOAD\"*\",8,1+RUN, scramble_infinity.d64)");
}

// ── Per-game gate throughput: each of the 7 gate games, end-to-end ──────────
//
// For each game: boot + mount (D64 or G64) + LOAD"*",8,1 + RUN (UNtimed setup),
// then TIME a fixed post-RUN cycle run. Reports per-game cycles → MHz so the
// report can quote "game X emulated N cycles in M ms on TRX64". One run each
// (these are throughput datapoints, not a precision median — the gate proper is
// seven_game_gate.rs). Drive-heavy G64 games stress the GCR + viacore the most.
#[test]
#[ignore = "perf benchmark; run --release with --ignored --nocapture"]
fn bench_per_game_gate() {
    if !roms_present() {
        eprintln!("skip bench_per_game_gate: ROMs absent at {ROM_DIR}");
        return;
    }
    // Fixed post-RUN timed budget per game (override with TRX64_GAME_BUDGET).
    let game_budget = env_budget("TRX64_GAME_BUDGET", 30_000_000);

    // (file, kind, name) — the gate roster (california excluded, see gate notes).
    let games: &[(&str, DiskKind, &str)] = &[
        ("scramble_infinity.d64", DiskKind::D64, "scramble"),
        ("POLARBEAR.d64", DiskKind::D64, "polarbear"),
        ("motm.g64", DiskKind::G64, "motm"),
        ("green_beret[ocean_1986](!).g64", DiskKind::G64, "greenberet"),
        ("impossible_mission_ii[epyx_1987](!).g64", DiskKind::G64, "impossible2"),
        ("last_ninja_remix_s1[system3_1991].g64", DiskKind::G64, "lastninja"),
        (
            "maniac_mansion_s1[activision_1987](german)(manual)(!).g64",
            DiskKind::G64,
            "maniac",
        ),
    ];

    eprintln!("\n========== PER-GAME GATE THROUGHPUT (TRX64 release) ==========");
    eprintln!("  post-RUN timed budget = {game_budget} C64 cycles/game");
    eprintln!(
        "  {:<12} {:>6} {:>12} {:>10} {:>8} {:>10}",
        "game", "kind", "cycles", "wall_s", "MHz", "real-time"
    );

    for (file, kind, name) in games {
        let path = format!("{SAMPLES}/{file}");
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("  {name:<12}  (sample absent — skipped)");
                continue;
            }
        };
        let mut m = Machine::new();
        m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
        let mut sink = NullSink;
        m.run_for_full(2_500_000, &mut sink, |_, _, _, _, _, _, _| {});
        m.drive8.attach_disk(DiskImage {
            kind: kind.clone(),
            bytes,
            backing_path: Some(path.clone()),
            read_only: false,
        });
        m.run_for_full(800_000, &mut sink, |_, _, _, _, _, _, _| {});
        inject_keys(&mut m, b"LOAD\"*\",8,1\r");
        m.run_for_full(500_000, &mut sink, |_, _, _, _, _, _, _| {});
        inject_keys(&mut m, b"RUN\r");

        let start_clk = m.c64_core.clk;
        let chunk = 500_000u64;
        let t0 = Instant::now();
        let mut done = 0u64;
        while done < game_budget {
            m.run_for_full_capped(chunk, chunk * 2, &mut sink, |_, _, _, _, _, _, _| {});
            done += chunk;
        }
        let secs = t0.elapsed().as_secs_f64();
        let executed = m.c64_core.clk.wrapping_sub(start_clk);
        let mhz = executed as f64 / secs / 1_000_000.0;
        let rt = executed as f64 / secs / PAL_HZ;
        eprintln!(
            "  {:<12} {:>6} {:>12} {:>10.4} {:>8.3} {:>9.1}x",
            name,
            format!("{kind:?}"),
            executed,
            secs,
            mhz,
            rt
        );
    }
}








// ── Workload 7: turbo scaling (Spec 856 D0) ──────────────────────────────────
//
// What a faster CPU costs in wall time. Spec 851's turbo is CPU-only: VIC, CIAs, SID and
// the drive stay on PHI2, so at N MHz one emulated second holds N× the 6502 work. The
// question D0 answers is how much MORE than that we pay, and where.
//
// Two workloads, both from a bare machine with no interrupts armed, run in PAL frames the
// way a host emulator paces itself (`run_for_full(FRAME)`):
//   - RAM: `INC $FB / BNE / INC $FC / JMP` — never touches IO, the best case for any
//     boundary saving;
//   - IO:  `LDA $D012 / STA $FB / JMP` — one VIC read per iteration, the worst case.
// Each at 1 / 16 / 48 / 64 MHz (U64-II table, badline timing on), reverse rings on and off.
// Reported per configuration: the real-time multiple and the CPU MHz actually emulated.
//
//   cargo test -p trx64-core --release --test perf_bench bench_turbo_scaling -- --ignored --nocapture
#[test]
#[ignore = "perf benchmark; run --release with --ignored --nocapture"]
fn bench_turbo_scaling() {
    use trx64_core::vic::SpeedProfile;
    const FRAME: u64 = 19_656;
    const RAM_LOOP: [u8; 9] = [0xe6, 0xfb, 0xd0, 0x02, 0xe6, 0xfc, 0x4c, 0x00, 0xc0];
    const IO_LOOP: [u8; 8] = [0xad, 0x12, 0xd0, 0x85, 0xfb, 0x4c, 0x00, 0xc0];
    let frames = env_budget("TRX64_TURBO_FRAMES", 25);
    let k = env_budget("TRX64_TURBO_K", 5) as usize;
    // (label, $D031 speed byte on the U64-II table, MHz) — bit 7 = badline timing.
    let speeds: [(&str, u8, u64); 4] = [("1", 0x80, 1), ("16", 0x89, 16), ("48", 0x8e, 48), ("64", 0x8f, 64)];

    let run_once = |code: &[u8], prefer: u8, rings: bool, fast: Option<bool>| -> f64 {
        let mut m = Machine::new();
        m.set_machine_profile(SpeedProfile::U64);
        m.set_u64_turbo(0x00, prefer);
        m.cpu_history.set_enabled(rings);
        m.delta_ring.set_enabled(rings);
        if let Some(on) = fast {
            m.turbo_fast_path = on;
        }
        m.poke(0xc000, code);
        m.write_full(0x0001, 0x37);
        m.c64_core.reg_pc = 0xc000;
        let mut sink = NullSink;
        // One untimed frame so the speed has been read at a boundary and caches are warm.
        m.run_for_full(FRAME, &mut sink, |_, _, _, _, _, _, _| {});
        let t0 = Instant::now();
        for _ in 0..frames {
            m.run_for_full(FRAME, &mut sink, |_, _, _, _, _, _, _| {});
        }
        t0.elapsed().as_secs_f64()
    };

    let emulated_secs = (frames * FRAME) as f64 / PAL_HZ;
    eprintln!("\n========== WORKLOAD 7 — turbo scaling (Spec 856 D0) ==========");
    eprintln!("  {frames} PAL frames per run ({emulated_secs:.3} s emulated), K = {k}, median reported");
    eprintln!("  {:<4} {:>4} {:<5} {:<5} {:>9} {:>11}", "load", "MHz", "rings", "fast", "rt-x", "CPU MHz");
    for (load, code) in [("RAM", &RAM_LOOP[..]), ("IO", &IO_LOOP[..])] {
        for (label, prefer, mhz) in speeds {
            for rings in [true, false] {
                for fast in [false, true] {
                    let mut v: Vec<f64> = (0..k).map(|_| run_once(code, prefer, rings, Some(fast))).collect();
                    let med = median(&mut v);
                    let rtx = emulated_secs / med;
                    let cpu_mhz = rtx * PAL_HZ * mhz as f64 / 1_000_000.0;
                    eprintln!(
                        "  {load:<4} {label:>4} {:<5} {:<5} {rtx:>8.2}x {cpu_mhz:>11.2}",
                        if rings { "on" } else { "off" },
                        if fast { "on" } else { "off" }
                    );
                    eprintln!(
                        "  RAW: turbo load={load} mhz={label} rings={rings} fast={fast} frames={frames} k={k} med_s={med:.6} rtx={rtx:.4} cpu_mhz={cpu_mhz:.3}"
                    );
                }
            }
        }
    }
}

// ── Workload 8: the CIA alarm check (Spec 857 D0) ────────────────────────────────────────
//
// Check off against check on, alternated run by run in one binary — a hot-path change inside
// the machine's ~2 % drift needs an alternating A/B, never a single run.
//   - booted, 1 MHz: READY prompt with the KERNAL's CIA1 Timer A IRQ and cursor blink running;
//   - RAM loop, 64 MHz: `INC $FB / BNE / INC $FC / JMP`, no IRQ armed.
//
//   cargo test -p trx64-core --release --test perf_bench bench_cia_alarm_check -- --ignored --nocapture
#[test]
#[ignore = "perf benchmark; run --release with --ignored --nocapture"]
fn bench_cia_alarm_check() {
    use trx64_core::vic::SpeedProfile;
    if !roms_present() {
        eprintln!("skip bench_cia_alarm_check: ROMs absent at {ROM_DIR}");
        return;
    }
    const FRAME: u64 = 19_656;
    let k = env_budget("TRX64_CIA857_K", 9) as usize;
    let booted_budget = env_budget("TRX64_CIA857_BUDGET", 5_000_000);
    let turbo_frames = env_budget("TRX64_CIA857_FRAMES", 25);

    let booted = |check: bool| -> f64 {
        let mut m = Machine::new();
        m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
        m.cia_alarm_check = check;
        let mut sink = NullSink;
        m.run_for_full(3_000_000, &mut sink, |_, _, _, _, _, _, _| {});
        let t0 = Instant::now();
        let mut done = 0;
        while done < booted_budget {
            m.run_for_full(FRAME, &mut sink, |_, _, _, _, _, _, _| {});
            done += FRAME;
        }
        t0.elapsed().as_secs_f64()
    };
    let turbo = |check: bool| -> f64 {
        let mut m = Machine::new();
        m.set_machine_profile(SpeedProfile::U64);
        m.set_u64_turbo(0x00, 0x8f);
        m.cia_alarm_check = check;
        m.poke(0xc000, &[0xe6, 0xfb, 0xd0, 0x02, 0xe6, 0xfc, 0x4c, 0x00, 0xc0]);
        m.write_full(0x0001, 0x37);
        m.c64_core.reg_pc = 0xc000;
        let mut sink = NullSink;
        m.run_for_full(FRAME, &mut sink, |_, _, _, _, _, _, _| {});
        let t0 = Instant::now();
        for _ in 0..turbo_frames {
            m.run_for_full(FRAME, &mut sink, |_, _, _, _, _, _, _| {});
        }
        t0.elapsed().as_secs_f64()
    };

    eprintln!("\n========== WORKLOAD 8 — CIA alarm check (Spec 857 D0), K = {k}, alternated ==========");
    for (label, run, emulated) in [
        ("booted @1 MHz", &booted as &dyn Fn(bool) -> f64, booted_budget as f64 / PAL_HZ),
        ("RAM loop @64 MHz", &turbo as &dyn Fn(bool) -> f64, (turbo_frames * FRAME) as f64 / PAL_HZ),
    ] {
        let (mut off, mut on) = (Vec::new(), Vec::new());
        for i in 0..k {
            // Alternate which side goes first, so a warm-up or thermal trend hits both.
            if i % 2 == 0 {
                off.push(run(false));
                on.push(run(true));
            } else {
                on.push(run(true));
                off.push(run(false));
            }
        }
        let wins = off.iter().zip(&on).filter(|(a, b)| b < a).count();
        let (mo, mn) = (median(&mut off), median(&mut on));
        eprintln!(
            "  {label:<18} off {:>7.2}x  on {:>7.2}x  real time  ({:+.1} % wall; on faster in {wins}/{k} pairs)",
            emulated / mo,
            emulated / mn,
            (mn - mo) / mo * 100.0
        );
        eprintln!("  RAW: cia857 load=\"{label}\" k={k} off_med_s={mo:.6} on_med_s={mn:.6} on_faster_pairs={wins}");
    }
}
