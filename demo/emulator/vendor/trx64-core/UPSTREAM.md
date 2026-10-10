# Vendored TRX64 core

Source: https://github.com/Jondalar/TRX64, tag `v0.11.0`, commit
`ab01fbc56304dd37179bcb469100f187684472ab`.
Only the `crates/trx64-core` crate is vendored; `LICENSE` is copied from upstream.
The crate retains GPL-3.0-or-later and its bundled reSID license/provenance.
Cargo workspace metadata is made explicit for this standalone copy.

Local extension: `U64SpeedTable::Experimental77` retains every U64 Elite II speed
index (0–15) and appends an **emulator-only** index 16 with 77 CPU cycles per PHI2
cycle. This is a nominal 77 MHz label, not an exact 77,000,000 Hz oscillator:
PAL gives 75,864,096 Hz and NTSC gives 78,750,210 Hz with the current base clocks.
The stock table remains the default. The experimental index delivers 77 CPU
cycles per PHI2; its precise real hardware counterpart is unknown. VIC/CIA/SID
clocks and REU DMA continue to use the base clock.

Local hardware timing correction (2026-10-06): nominal speed labels and indices
remain unchanged, but `U64SpeedTable::delivered_slots()` applies the measured
CPU budget at the top two real hardware indices. U64 index 14/15 delivers 39/47
CPU cycles per PHI2 instead of labels 40/48; U64II/C64U index 14/15 delivers
47/63 instead of labels 48/64. All lower entries retain their nominal budget.
The experimental profile inherits this correction at indices 0–15 only; its
unverified index 16 continues to deliver 77. This correction is used when
adopting CPU speed, restoring a checkpoint, and resolving subcycle border
colors. The public `Machine::turbo_divider()` API retains the nominal label.

Evidence: measurements with four independent instruction mixes and Gideon's
confirmation that a VIC time slot is reserved even with the display blanked:
- https://github.com/GideonZ/1541ultimate/issues/874
- https://github.com/GideonZ/1541ultimate/issues/874#issuecomment-5583392633
- https://raw.githubusercontent.com/xahmol/ultimate-uci-oscar64/main/docs/UPIC_MANUAL.md

This is a **uniform delivered-throughput model**, not a reproduction of the
precise reserved-slot phase in the U64 gateware (that CPU arbiter is not in the
public firmware sources). External cartridge accesses and physical SID sockets
have additional 1 MHz/BA constraints; this change neither adds nor changes
those bus behaviors. Do not infer complete hardware cycle accuracy from the
new per-line UPic pitch or from an emulated benchmark.

As of 2026-10-04 Commodore77 firmware and its turbo register specification have
not been published. The extra index is a preparation tool, **not a claim of
Commodore77 firmware or hardware compatibility**. In particular, software must
not assume `$D031=$10` or `$90` is valid on real hardware.

Official references checked on 2026-10-04:
- https://commodore.net/downloads/
- https://commodore.net/commodore-jacks-into-night-city-with-the-commodore-77-an-all-new-cyberpunk-2077-collaboration/
- https://1541u-documentation.readthedocs.io/en/latest/config/turbo_mode.html

Self-contained regression tests: `tests/experimental77_gate.rs` and
`tests/u64_delivered_slots_gate.rs`. The delivered-slot gate checks nominal
labels separately from CPU budgets, four instruction mixes with both turbo
fast paths, exact CIA/VIC elapsed time, and checkpoint continuation with a
nonzero CPU subphase. The experimental 77-slot budget is checked unchanged.
The boot portions of `cia_alarm_check_gate`, `turbo_fastpath_gate`, and
`u64_boot_speed_gate` accept `TRX64_ROM_DIR`; when unset, they retain the
upstream fixture path and missing-ROM skip behavior. A qualified boot run
must supply actual ROMs and check that no boot test printed a skip.
The CIA boot golden also requires the 1541 DOS ROM; its IEC line activity
changes the observable digest even though the C64 ROMs alone can boot.
