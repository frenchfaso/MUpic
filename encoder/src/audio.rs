//! Exact cumulative sampler timing for the PAL player.
const PAL_PHI2_HZ: u64 = 985248;
const PAL_FRAME_PHI2: u64 = 312 * 63;
const SAMPLER_CLOCK_HZ: u64 = 6250000;

pub(super) fn sample_at_tick(tick: usize, divider: u16) -> usize {
    // Exact rational arithmetic with ties-to-even rounding, matching the
    // cumulative Python prototype without accumulating per-block drift.
    let numerator = tick as u128 * u128::from(SAMPLER_CLOCK_HZ) * u128::from(PAL_FRAME_PHI2);
    let denominator = (u128::from(divider) + 2) * u128::from(PAL_PHI2_HZ);
    let whole = numerator / denominator;
    let remainder = numerator % denominator;
    (whole
        + u128::from(
            remainder * 2 > denominator
                || (remainder * 2 == denominator && !whole.is_multiple_of(2)),
        )) as usize
}
