//! Spec 856 D1 — the turbo fast path may change the speed and nothing else.
//!
//! Written and green on the loop that synchronises at every instruction BEFORE the fast
//! path existed, so these pin today's behaviour instead of describing the new one.
//!
//! Two things can go wrong when instructions that do not advance `clk` stop paying for the
//! boundary sync:
//!
//!   - an IRQ acknowledge that lands inside one PHI2 cycle never reaches `IntStatus` — below
//!     the divider `clk_inc` returns early, so the boundary restamp is the only path it has
//!     (§3). The handler returns to a line that is still high and takes the same IRQ again;
//!   - something the fast path skipped was not idempotent after all, and state drifts.
//!
//! These retained regressions count CIA and VIC handler entries against the events
//! that caused them at stock and turbo clocks.

use trx64_core::vic::SpeedProfile;
use trx64_core::{Machine, NullSink};

/// PAL: 63 cycles × 312 lines.
const FRAME: u64 = 19_656;
/// `$D031` speed bytes on the U64-II table. Bit 7 = badline timing.
const MHZ_1: u8 = 0x80;
const MHZ_64: u8 = 0x8f;
const MHZ_64_NO_BADLINE: u8 = 0x0f;

fn machine(prefer: u8, fast: bool) -> Machine {
    let mut m = Machine::new();
    m.set_machine_profile(SpeedProfile::U64);
    m.set_u64_turbo(0x00, prefer);
    m.turbo_fast_path = fast;
    m
}

fn start(m: &mut Machine, main: &[u8], handler: &[u8]) {
    m.poke(0xc000, main);
    m.poke(0xc100, handler);
    m.write_full(0x0001, 0x35); // BASIC + KERNAL out: the IRQ vector is RAM
    m.c64_core.reg_pc = 0xc000;
}

fn entries(m: &mut Machine) -> u64 {
    u64::from(m.read_full(0x0400)) | (u64::from(m.read_full(0x0401)) << 8)
}

// ── CIA1 timer A, acknowledged by reading $DC0D ──────────────────────────────────────────

/// SEI / vector → $C100 / mask all, clear pending / latch $0FFF / enable TA / start / CLI /
/// `JMP *` — the main loop never touches IO, so at 64 MHz it is exactly what the fast path
/// runs back to back.
const CIA_MAIN: [u8; 43] = [
    0x78, // C000 SEI
    0xa9, 0x00, 0x8d, 0xfe, 0xff, // LDA #$00 / STA $FFFE
    0xa9, 0xc1, 0x8d, 0xff, 0xff, // LDA #$C1 / STA $FFFF
    0xa9, 0x7f, 0x8d, 0x0d, 0xdc, // LDA #$7F / STA $DC0D
    0xad, 0x0d, 0xdc, // LDA $DC0D
    0xa9, 0xff, 0x8d, 0x04, 0xdc, // LDA #$FF / STA $DC04
    0xa9, 0x0f, 0x8d, 0x05, 0xdc, // LDA #$0F / STA $DC05
    0xa9, 0x81, 0x8d, 0x0d, 0xdc, // LDA #$81 / STA $DC0D
    0xa9, 0x11, 0x8d, 0x0e, 0xdc, // LDA #$11 / STA $DC0E
    0x58, // C027 CLI
    0x4c, 0x28, 0xc0, // C028 JMP $C028
];
/// INC $0400 / BNE +3 / INC $0401 / LDA $DC0D / RTI — 23 CPU cycles with the IRQ entry,
/// so at 64 MHz the whole handler, acknowledge included, fits inside one PHI2 cycle.
const CIA_HANDLER: [u8; 12] = [0xee, 0x00, 0x04, 0xd0, 0x03, 0xee, 0x01, 0x04, 0xad, 0x0d, 0xdc, 0x40];
const CIA_PERIOD: u64 = 0x1000;

#[test]
fn a_cia_acknowledge_inside_one_phi2_cycle_is_taken_once_per_underflow() {
    for (prefer, fast) in [(MHZ_1, false), (MHZ_64, false), (MHZ_64, true), (MHZ_64_NO_BADLINE, true)] {
        let mut m = machine(prefer, fast);
        start(&mut m, &CIA_MAIN, &CIA_HANDLER);
        let clk0 = m.c64_core.clk;
        m.run_for_full(50 * FRAME, &mut NullSink, |_, _, _, _, _, _, _| {});
        let underflows = (m.c64_core.clk - clk0) / CIA_PERIOD;
        let taken = entries(&mut m);
        assert!(underflows > 200, "the run covered enough underflows to mean something: {underflows}");
        assert!(
            taken.abs_diff(underflows) <= 2,
            "speed ${prefer:02x} fast={fast}: {taken} handler entries for {underflows} underflows"
        );
    }
}

// ── VIC raster, acknowledged by writing $D019 ─────────────────────────────────────────────

const RASTER_MAIN: [u8; 38] = [
    0x78, // C000 SEI
    0xa9, 0x00, 0x8d, 0xfe, 0xff, // LDA #$00 / STA $FFFE
    0xa9, 0xc1, 0x8d, 0xff, 0xff, // LDA #$C1 / STA $FFFF
    0xa9, 0x80, 0x8d, 0x12, 0xd0, // LDA #$80 / STA $D012
    0xad, 0x11, 0xd0, 0x29, 0x7f, 0x8d, 0x11, 0xd0, // LDA $D011 / AND #$7F / STA $D011
    0xa9, 0xff, 0x8d, 0x19, 0xd0, // LDA #$FF / STA $D019
    0xa9, 0x01, 0x8d, 0x1a, 0xd0, // LDA #$01 / STA $D01A
    0x58, // C022 CLI
    0x4c, 0x23, 0xc0, // C023 JMP $C023
];
/// INC $0400 / BNE +3 / INC $0401 / LDA #$FF / STA $D019 / RTI
const RASTER_HANDLER: [u8; 14] =
    [0xee, 0x00, 0x04, 0xd0, 0x03, 0xee, 0x01, 0x04, 0xa9, 0xff, 0x8d, 0x19, 0xd0, 0x40];

#[test]
fn a_raster_acknowledge_inside_one_phi2_cycle_is_taken_once_per_frame() {
    for (prefer, fast) in [(MHZ_1, false), (MHZ_64, false), (MHZ_64, true), (MHZ_64_NO_BADLINE, true)] {
        let mut m = machine(prefer, fast);
        start(&mut m, &RASTER_MAIN, &RASTER_HANDLER);
        let clk0 = m.c64_core.clk;
        m.run_for_full(50 * FRAME, &mut NullSink, |_, _, _, _, _, _, _| {});
        let frames = (m.c64_core.clk - clk0) / FRAME;
        let taken = entries(&mut m);
        assert!(
            taken.abs_diff(frames) <= 1,
            "speed ${prefer:02x} fast={fast}: {taken} handler entries in {frames} frames"
        );
    }
}
