//! Internal turbo throughput follows measured U64 hardware, not its MHz labels.
//! These fixtures need no ROMs or guest program: independent opcode mixes also
//! check that changing delivered CPU slots does not change CIA or VIC time.


use trx64_core::vic::{SpeedProfile, SubCycleColour, U64SpeedTable};
use trx64_core::{Machine, NullSink};

fn machine(table: U64SpeedTable, index: u8, fast: bool) -> Machine {
    let mut m = Machine::new();
    m.set_machine_profile(SpeedProfile::U64);
    m.set_u64_speed_table(table);
    m.set_u64_turbo(0x01, index | 0x80);
    m.turbo_fast_path = fast;
    m.write_full(0x0001, 0x35);
    m.write_full(0xd011, 0);
    m.write_full(0xd015, 0);
    m.c64_core.reg_p |= 0x04; // No interrupt may alter the measured instruction mix.
    m.poke(0xc100, &[0xea, 0xea]);
    m.c64_core.reg_pc = 0xc100;
    m.run_for_full_capped(16, 2, &mut NullSink, |_, _, _, _, _, _, _| {});
    m
}

#[test]
fn only_the_top_two_real_indices_lose_a_slot_and_labels_are_unchanged() {
    let u64 = [1, 2, 3, 4, 5, 6, 8, 10, 12, 14, 16, 20, 24, 32, 40, 48];
    let u64ii = [1, 2, 3, 4, 6, 8, 10, 12, 14, 16, 20, 24, 32, 40, 48, 64];
    let delivered_u64 = [1, 2, 3, 4, 5, 6, 8, 10, 12, 14, 16, 20, 24, 32, 39, 47];
    let delivered_u64ii = [1, 2, 3, 4, 6, 8, 10, 12, 14, 16, 20, 24, 32, 40, 47, 63];
    for index in 0..16 {
        let i = usize::from(index);
        assert_eq!(U64SpeedTable::U64.mhz(index), u64[i]);
        assert_eq!(U64SpeedTable::U64II.mhz(index), u64ii[i]);
        assert_eq!(U64SpeedTable::U64.delivered_slots(index), delivered_u64[i]);
        assert_eq!(U64SpeedTable::U64II.delivered_slots(index), delivered_u64ii[i]);
        assert_eq!(U64SpeedTable::Experimental77.delivered_slots(index), delivered_u64ii[i]);
    }
    assert_eq!(U64SpeedTable::U64.delivered_slots(0xff), 47);
    assert_eq!(U64SpeedTable::U64II.delivered_slots(0xff), 63);
    assert_eq!(U64SpeedTable::Experimental77.delivered_slots(16), 77);
    assert_eq!(U64SpeedTable::Experimental77.delivered_slots(0xff), 77);
}

#[test]
fn independent_instruction_mixes_deliver_the_measured_rate() {
    // One instruction then JMP $C000: each pair has a separately known 6510
    // cycle cost. Opcode-only, zero-page, far reads and far writes distinguish
    // a delivered-clock change from an accidental memory-access penalty.
    let mixes: [(&str, &[u8], u64); 4] = [
        ("opcode", &[0xea], 5),
        ("zero-page", &[0xa5, 0x02], 6),
        ("absolute-read", &[0xad, 0x00, 0xce], 7),
        ("absolute-write", &[0x8d, 0x00, 0xce], 7),
    ];
    const PAIRS: u64 = 4096;
    for (table, index, slots) in [
        (U64SpeedTable::U64, 13, 32),
        (U64SpeedTable::U64, 14, 39),
        (U64SpeedTable::U64, 15, 47),
        (U64SpeedTable::U64II, 13, 40),
        (U64SpeedTable::U64II, 14, 47),
        (U64SpeedTable::U64II, 15, 63),
        (U64SpeedTable::Experimental77, 16, 77),
    ] {
        for fast in [false, true] {
            for (name, instruction, pair_cycles) in mixes {
                let mut m = machine(table, index, fast);
                assert_eq!(m.c64_core.turbo_div, slots);
                assert_eq!(m.turbo_divider(), u64::from(table.mhz(index)), "public API is nominal");
                let mut code = instruction.to_vec();
                code.extend_from_slice(&[0x4c, 0x00, 0xc0]);
                m.poke(0xc000, &code);
                m.c64_core.reg_pc = 0xc000;
                m.write_full(0xdc04, 0xff);
                m.write_full(0xdc05, 0xff);
                m.write_full(0xdc0e, 0x01);
                // Settle the CIA load/start pipeline before measuring a delta.
                // In particular, the experimental divider can begin after the
                // warm-up has already advanced PHI2; assuming $FFFF at that
                // point would fold the timer's initial load delay into elapsed.
                m.run_for_full_capped(100_000, 256, &mut NullSink, |_, _, _, _, _, _, _| {});
                let timer_before = u64::from(m.read_full_live(0xdc04))
                    | (u64::from(m.read_full_live(0xdc05)) << 8);
                let clk = m.c64_core.clk;
                let phase = u64::from(m.c64_core.turbo_phase);
                let raster_before = u64::from(m.vic.raster_line) * 63 + u64::from(m.vic.raster_cycle);
                m.run_for_full_capped(100_000, PAIRS * 2, &mut NullSink, |_, _, _, _, _, _, _| {});
                let elapsed = m.c64_core.clk - clk;
                let expected = (phase + PAIRS * pair_cycles) / u64::from(slots);
                assert_eq!(elapsed, expected, "{table:?} index={index} {name} fast={fast}");
                // Host register reads catch the lazy timer up to this exact
                // endpoint without executing another guest instruction.
                let timer = u64::from(m.read_full_live(0xdc04))
                    | (u64::from(m.read_full_live(0xdc05)) << 8);
                assert_eq!(
                    timer_before - timer, elapsed,
                    "CIA counts base PHI2: table={table:?} index={index} mix={name} \
                     fast={fast} timer_before={timer_before:#06x} timer={timer:#06x} \
                     elapsed={elapsed} start_clk={clk} \
                     end_clk={} cia_checked_clk={} cia_tick_clk={}",
                    m.c64_core.clk, m.cia1.checked_clk, m.cia1.clk
                );
                let raster_clocks = u64::from(m.vic.raster_line) * 63 + u64::from(m.vic.raster_cycle);
                assert_eq!(raster_clocks - raster_before, elapsed, "VIC keeps the base clock");
            }
        }
    }
}



#[test]
fn two_phi2_cycles_have_exactly_sixteen_subcycle_dots_at_64x() {
    // The v1.3 scanout uses 126 delivered CPU cycles per 16 dots. This
    // exercises all physical dot slots without imposing an unknown VIC slot
    // position: one evenly interpolated sample per dot is the bounded model.
    for dot in 0..16u32 {
        let total_phase = (dot * 63).div_ceil(8);
        let cycle = total_phase / 63;
        let pixel = SubCycleColour::pixel_for(total_phase % 63, 63);
        assert_eq!(cycle * 8 + pixel as u32, dot);
    }
    assert_eq!(126 / 63, 2);
}
