//! Self-contained regression gates for the opt-in, emulator-only 77× PHI2 table.
//! No external ROM files and no skipped tests. This does not validate real C77 hardware.

use trx64_core::vic::{SpeedProfile, SubCycleColour, U64SpeedTable, VicII};
use trx64_core::{Machine, NullSink};

const SHORT_RUN: u64 = 2048; // A 16-bit work counter cannot wrap at 77× in this interval.

fn machine(index: u8) -> Machine {
    let mut m = Machine::new();
    m.set_machine_profile(SpeedProfile::U64);
    m.set_u64_speed_table(U64SpeedTable::Experimental77);
    m.set_u64_turbo(0x01, index);
    m.write_full(0x0001, 0x37);
    m
}

fn run(m: &mut Machine, code: &[u8], instructions: u64) {
    m.poke(0xc100, code);
    m.c64_core.reg_pc = 0xc100;
    m.run_for_full_capped(
        20_000,
        instructions,
        &mut NullSink,
        |_, _, _, _, _, _, _| {},
    );
}

fn count_work(m: &mut Machine, cycles: u64) -> u32 {
    // INC $FB / BNE +2 / INC $FC / JMP $C000.
    m.poke(
        0xc000,
        &[0xe6, 0xfb, 0xd0, 0x02, 0xe6, 0xfc, 0x4c, 0x00, 0xc0],
    );
    m.poke(0x00fb, &[0, 0]);
    m.c64_core.reg_pc = 0xc000;
    m.run_for_full_capped(cycles, u64::MAX, &mut NullSink, |_, _, _, _, _, _, _| {});
    u32::from(m.read_full(0x00fb)) | (u32::from(m.read_full(0x00fc)) << 8)
}

#[test]
fn existing_tables_and_default_are_unchanged() {
    let u64 = [1, 2, 3, 4, 5, 6, 8, 10, 12, 14, 16, 20, 24, 32, 40, 48];
    let elite2 = [1, 2, 3, 4, 6, 8, 10, 12, 14, 16, 20, 24, 32, 40, 48, 64];
    assert_eq!(U64SpeedTable::default(), U64SpeedTable::U64II);
    for i in 0..16 {
        assert_eq!(U64SpeedTable::U64.mhz(i), u64[usize::from(i)]);
        assert_eq!(U64SpeedTable::U64II.mhz(i), elite2[usize::from(i)]);
        assert_eq!(U64SpeedTable::Experimental77.mhz(i), elite2[usize::from(i)]);
    }
    for i in [16, 17, 0x7f, 0xff] {
        assert_eq!(U64SpeedTable::U64.mhz(i), 48);
        assert_eq!(U64SpeedTable::U64II.mhz(i), 64);
        assert_eq!(U64SpeedTable::Experimental77.mhz(i), 77);
    }
    assert_eq!(
        U64SpeedTable::parse("experimental-77"),
        Some(U64SpeedTable::Experimental77)
    );
    assert_eq!(
        U64SpeedTable::parse("c77"),
        None,
        "do not advertise an official C77 table"
    );
}

#[test]
fn cpu_work_scales_from_delivered_63_to_experimental_77_per_phi2() {
    let a = count_work(&mut machine(15), SHORT_RUN);
    let b = count_work(&mut machine(16), SHORT_RUN);
    let measured = f64::from(b) / f64::from(a);
    let expected = 77.0 / 63.0;
    eprintln!("64× label (63 slots): {a} loops, experimental 77 slots: {b} loops, ratio {measured:.6} (expected {expected:.6})");
    assert!(
        a > 10_000 && b < 65_536,
        "counter has enough work without wrapping"
    );
    assert!(
        (measured - expected).abs() < 0.002,
        "CPU throughput scales by the divider"
    );
}

#[test]
fn cia_and_raster_keep_the_base_phi2_clock() {
    let sample = |index| {
        let mut m = machine(index);
        m.write_full(0xdc04, 0xff);
        m.write_full(0xdc05, 0xff);
        m.write_full(0xdc0e, 0x01);
        let origin = m.c64_core.clk;
        count_work(&mut m, SHORT_RUN);
        let elapsed = m.c64_core.clk - origin;
        let timer = u64::from(m.cia1.peek(0xdc04)) | (u64::from(m.cia1.peek(0xdc05)) << 8);
        (
            elapsed,
            timer + elapsed,
            m.vic.raster_line,
            m.vic.raster_cycle,
            m.timing().cpu_hz,
        )
    };
    let slow = sample(0);
    let normal = sample(15);
    let fast = sample(16);
    eprintln!("base clock samples: 1×={slow:?}, 64×={normal:?}, 77×={fast:?}");
    for other in [normal, fast] {
        assert!(slow.0.abs_diff(other.0) < 8);
        assert_eq!(slow.1, other.1, "CIA timer counts PHI2, not CPU cycles");
        assert!(
            slow.2.abs_diff(other.2) <= 1,
            "raster line remains on the base clock"
        );
        assert_eq!(slow.4, other.4, "the crystal frequency does not change");
    }
    assert_eq!(normal, fast, "64× and 77× end on the same PHI2 boundary");
}

#[test]
fn d031_selects_77_and_can_return_to_64() {
    let mut m = machine(15);
    count_work(&mut m, 32);
    assert_eq!(m.c64_core.turbo_div, 63);
    // These register values are intentionally emulator-only, not a C77 hardware API.
    run(&mut m, &[0xa9, 0x90, 0x8d, 0x31, 0xd0, 0x4c, 0x05, 0xc1], 3);
    count_work(&mut m, 32);
    assert_eq!(m.read_full(0xd031), 0x90);
    assert_eq!(m.vic.u64_speed(), (16, true));
    assert_eq!(m.c64_core.turbo_div, 77);
    run(&mut m, &[0xa9, 0x8f, 0x8d, 0x31, 0xd0, 0x4c, 0x05, 0xc1], 3);
    count_work(&mut m, 32);
    assert_eq!(m.read_full(0xd031), 0x8f);
    assert_eq!(
        m.c64_core.turbo_div, 63,
        "nominal 64× scanout uses the hardware's 63 delivered slots"
    );

    let mut stock = machine(15);
    stock.set_u64_speed_table(U64SpeedTable::U64II);
    run(
        &mut stock,
        &[0xa9, 0x90, 0x8d, 0x31, 0xd0, 0x4c, 0x05, 0xc1],
        3,
    );
    count_work(&mut stock, 32);
    assert_eq!(
        stock.c64_core.turbo_div, 63,
        "the extension is unreachable on the stock table"
    );
}

#[test]
fn divider_changes_still_wait_for_phi2_edges_and_resync_phase() {
    let mut m = machine(15);
    count_work(&mut m, 32);
    m.c64_core.turbo_phase = 37;
    m.vic.write_reg(0x31, 0x90);
    m.sync_turbo_from_vic();
    assert_eq!(m.c64_core.turbo_phase, 0);
    assert_eq!(
        m.c64_core.turbo_div, 63,
        "the current cycle retains its old divider"
    );
    assert_eq!(m.c64_core.pending_turbo_div, 77);
    count_work(&mut m, 32);
    assert_eq!(m.c64_core.turbo_div, 77);
    m.c64_core.turbo_phase = 41;
    m.vic.write_reg(0x31, 0x8f);
    m.sync_turbo_from_vic();
    assert_eq!(m.c64_core.turbo_phase, 0);
    assert_eq!(m.c64_core.turbo_div, 77);
    assert_eq!(m.c64_core.pending_turbo_div, 63);
}

#[test]
fn warm_reset_preserves_opt_in_table_but_resets_cpu_register() {
    let mut m = machine(16);
    run(&mut m, &[0xa9, 0x90, 0x8d, 0x31, 0xd0], 2);
    m.warm_reset();
    assert_eq!(m.vic.u64_speed_table, U64SpeedTable::Experimental77);
    assert_eq!(m.vic.u64_speed_prefer, 16);
    assert_eq!(m.vic.u64_regs_en, 0x01);
    assert!(!m.vic.u64_d031_written);
    assert_eq!(m.speed_profile(), SpeedProfile::U64);
    assert_eq!(
        m.turbo_divider(),
        1,
        "normal post-reset hold requests the base clock"
    );
}

#[test]
fn subcycle_border_mapping_supports_77_and_delivered_63() {
    let mut previous = 0;
    for phase in 0..77 {
        let pixel = SubCycleColour::pixel_for(phase, 77);
        assert_eq!(pixel, ((phase * 8) / 77) as usize);
        assert!(pixel >= previous && pixel < 8);
        previous = pixel;
    }
    assert_eq!(previous, 7);
    for phase in 0..63 {
        assert_eq!(SubCycleColour::pixel_for(phase, 63), ((phase * 8) / 63) as usize);
    }
}

#[test]
fn reu_16mib_dma_keeps_base_cycle_cost_at_64_and_77() {
    let transfer = |index| {
        let mut m = machine(index);
        assert!(m.attach_reu(16 * 1024));
        count_work(&mut m, 32); // Adopt the requested divider before measuring.
        let data: Vec<u8> = (0..1024).map(|i| ((i * 37 + 13) & 0xff) as u8).collect();
        let reu_addr = 0xfffc00; // Last 1 KiB of the full 16 MiB expansion.
        m.reu_mut().unwrap().write_ram(reu_addr, &data);
        for (address, value) in [
            (0xdf02, 0x00),
            (0xdf03, 0x20),
            (0xdf04, 0x00),
            (0xdf05, 0xfc),
            (0xdf06, 0xff),
            (0xdf07, 0x00),
            (0xdf08, 0x04),
            (0xdf0a, 0x00),
        ] {
            m.write_full(address, value);
        }
        let before = m.c64_core.clk;
        run(&mut m, &[0xa9, 0x91, 0x8d, 0x01, 0xdf, 0x4c, 0x05, 0xc1], 3);
        let elapsed = m.c64_core.clk - before;
        let copied: Vec<u8> = (0..1024).map(|i| m.read_full(0x2000 + i)).collect();
        assert_eq!(
            copied, data,
            "every byte from the top REU bank was transferred"
        );
        assert!(
            (1024..=1026).contains(&elapsed),
            "one PHI2 cycle per transferred byte: {elapsed}"
        );
        elapsed
    };
    let normal = transfer(15);
    let fast = transfer(16);
    eprintln!("1024-byte REU DMA: 64×={normal} PHI2 cycles, 77×={fast} PHI2 cycles");
    assert_eq!(normal, fast, "CPU turbo does not overclock REU DMA");
}

#[test]
fn resync_pair_keeps_zero_phi2_cost_at_both_dividers() {
    for (index, register, divider) in [(15, 0x8f, 63), (16, 0x90, 77)] {
        let mut m = machine(index);
        count_work(&mut m, 32);
        assert_eq!(m.c64_core.turbo_div, divider);
        m.c64_core.turbo_phase = 0;
        m.poke(
            0xc100,
            &[
                0xa9, 0x80, 0xa2, register, 0x8d, 0x31, 0xd0, 0x8e, 0x31, 0xd0,
            ],
        );
        m.c64_core.reg_pc = 0xc100;
        let before = m.clk;
        m.run_for_full_capped(4096, 4, &mut NullSink, |_, _, _, _, _, _, _| {});
        assert_eq!(
            m.clk - before,
            0,
            "resync pair at {divider}× remains within one PHI2"
        );
        m.sync_turbo_from_vic(); // Observe the final write at the next instruction boundary.
        assert_eq!(m.c64_core.turbo_phase, 0);
        assert_eq!(m.c64_core.turbo_div, divider);
        assert_eq!(m.c64_core.pending_turbo_div, divider);
    }
}

#[test]
fn border_writes_at_77_reach_each_of_the_eight_vic_pixels() {
    let mut vic = VicII::new();
    vic.speed_profile = SpeedProfile::U64;
    vic.turbo_div = 77;
    for pixel in 0..8u32 {
        vic.turbo_phase = (pixel * 77).div_ceil(8);
        vic.write_reg(0x20, pixel as u8 + 1);
    }
    assert_eq!(
        vic.subcycle_colour
            .expect("border writes arm subcycle slots")
            .slots,
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(vic.colour_register(0x20), 8);
}
