//! Host-only, bounded-loss rate control for the existing DICT4/SKIP decoder.
//! Palette conversion and cadence are unchanged. Every result is checked by
//! the caller's authoritative record cost model against the requested budget.
use std::collections::HashMap;

const FRAME: usize = 46080;
const COLS: usize = 180;
const TOP: usize = 5;
const ROWS: usize = 192;
const ACTIVE: usize = 190;

fn read_word(frame: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(frame[at..at + 4].try_into().unwrap())
}
fn grey(colour: usize) -> bool {
    matches!(colour, 0 | 1 | 11 | 12 | 15)
}
fn oklab(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|v| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}
fn word_error(a: u32, b: u32, distance: &[Vec<f64>], padding: bool) -> f64 {
    if padding && b >> 16 != 0 {
        return f64::INFINITY;
    }
    let aa = a.to_le_bytes();
    let bb = b.to_le_bytes();
    (0..4)
        .map(|k| distance[aa[k] as usize][bb[k] as usize])
        .sum()
}
#[derive(Clone)]
struct Change {
    at: usize,
    value: u32,
    error: f64,
    skip: bool,
}

/// `cost` must return the maximum of the three actual record estimates for one
/// transition. It captures the prefetch setting so there is one cost authority.
pub(super) fn convert<F>(
    frames: &[u8],
    palette: &[[u8; 3]; 16],
    budget: usize,
    cost: F,
) -> Result<(Vec<u8>, String), String>
where
    F: Fn(&[u8], &[u8]) -> usize,
{
    if frames.is_empty() || !frames.len().is_multiple_of(FRAME) || budget == 0 {
        return Err(
            "DICT4 rate control requires nonempty whole packed frames and positive budget".into(),
        );
    }
    Prepared::new(palette).convert(frames, budget, cost)
}

/// Reuse the palette metric across transitions in the bounded-memory encoder.
/// The original multi-frame path and this path share exactly the same projection.
pub(super) struct Prepared {
    palette: [[u8; 3]; 16],
    colour_distance: [[f64; 16]; 16],
    distance: Vec<Vec<f64>>,
    top: usize,
    rows: usize,
    active: usize,
}
impl Prepared {
    pub(super) fn new(palette: &[[u8; 3]; 16]) -> Self {
        Self::with_geometry(palette, TOP, ROWS, ACTIVE)
    }
    pub(super) fn with_geometry(
        palette: &[[u8; 3]; 16],
        top: usize,
        rows: usize,
        active: usize,
    ) -> Self {
        assert!(rows % 4 == 0 && active <= rows && top + rows <= 256);
        let labs = palette.map(oklab);
        let mut colour_distance = [[0.0; 16]; 16];
        for a in 0..16 {
            for b in 0..16 {
                colour_distance[a][b] = (0..3).map(|k| (labs[a][k] - labs[b][k]).powi(2)).sum();
            }
        }
        let mut distance = vec![vec![0.0; 256]; 256];
        for (a, row) in distance.iter_mut().enumerate() {
            for (b, d) in row.iter_mut().enumerate() {
                // Preserve the existing neutral guard per pixel, including mixed words.
                *d = if (grey(a & 15) && !grey(b & 15)) || (grey(a >> 4) && !grey(b >> 4)) {
                    f64::INFINITY
                } else {
                    colour_distance[a & 15][b & 15] + colour_distance[a >> 4][b >> 4]
                };
            }
        }
        Self {
            palette: *palette,
            colour_distance,
            distance,
            top,
            rows,
            active,
        }
    }
    pub(super) fn convert<F>(
        &self,
        frames: &[u8],
        budget: usize,
        cost: F,
    ) -> Result<(Vec<u8>, String), String>
    where
        F: Fn(&[u8], &[u8]) -> usize,
    {
        if frames.is_empty() || !frames.len().is_multiple_of(FRAME) || budget == 0 {
            return Err("DICT4 projection requires whole frames and positive budget".into());
        }
        let (top, word_rows, active, pixels) =
            (self.top, self.rows, self.active, COLS * self.active * 2);
        let palette = &self.palette;
        let colour_distance = &self.colour_distance;
        let distance = &self.distance;
        let mut result = Vec::with_capacity(frames.len());
        result.extend_from_slice(&frames[..FRAME]);
        let mut rows = Vec::new();
        let mut total_changed = 0usize;
        let mut total_sse = 0.0;
        let mut total_oklab = 0.0;
        let mut worst_changed = 0usize;
        let mut adapted = 0usize;
        let mut peak = 0usize;
        let mut total_skips = 0usize;
        for (index, target) in frames.chunks_exact(FRAME).enumerate().skip(1) {
            let before = &result[result.len() - FRAME..];
            let original_cost = cost(before, target);
            let mut after = target.to_vec();
            let mut final_cost = original_cost;
            let mut selected = 0usize;
            let mut skip_count = 0usize;
            let mut threshold = 0.0;
            let mut evaluations = 1usize;
            if original_cost > budget {
                let mut frequency: HashMap<u32, (usize, usize)> = HashMap::new();
                for c in 0..COLS {
                    for y in (top..top + word_rows).step_by(4) {
                        let at = c * 256 + y;
                        let value = read_word(target, at);
                        if value != read_word(before, at) {
                            let order = frequency.len();
                            frequency.entry(value).or_insert((0, order)).0 += 1;
                        }
                    }
                }
                let mut entries = frequency.into_iter().collect::<Vec<_>>();
                entries.sort_by_key(|(_, (n, order))| (std::cmp::Reverse(*n), *order));
                entries.truncate(254);
                let dictionary = entries.iter().map(|(value, _)| *value).collect::<Vec<_>>();
                let mut nearest: HashMap<(u32, bool), (u32, f64)> = HashMap::new();
                let mut changes = Vec::new();
                for c in 0..COLS {
                    for y in (top..top + word_rows).step_by(4) {
                        let at = c * 256 + y;
                        let wanted = read_word(target, at);
                        let old = read_word(before, at);
                        if wanted == old {
                            continue;
                        }
                        let padding = y + 4 > top + active;
                        let &mut (mut value, mut error) =
                            nearest.entry((wanted, padding)).or_insert_with(|| {
                                let mut best = (wanted, f64::INFINITY);
                                for &candidate in &dictionary {
                                    let e = word_error(wanted, candidate, distance, padding);
                                    if e < best.1 {
                                        best = (candidate, e);
                                    }
                                    if e == 0.0 {
                                        break;
                                    }
                                }
                                best
                            });
                        // Exact dictionary words already have the cheapest lossless code.
                        if error == 0.0 {
                            continue;
                        }
                        let old_error = word_error(wanted, old, distance, padding);
                        let skip = old_error <= error;
                        if skip {
                            value = old;
                            error = old_error;
                        }
                        if error.is_finite() && value != wanted {
                            changes.push(Change {
                                at,
                                value,
                                error,
                                skip,
                            });
                        }
                    }
                }
                changes.sort_by(|a, b| a.error.total_cmp(&b.error).then(a.at.cmp(&b.at)));
                let render = |count: usize| {
                    let mut output = target.to_vec();
                    for change in &changes[..count] {
                        output[change.at..change.at + 4]
                            .copy_from_slice(&change.value.to_le_bytes());
                    }
                    output
                };
                let all = render(changes.len());
                let all_cost = cost(before, &all);
                evaluations += 1;
                if all_cost > budget || all == before {
                    return Err(format!("DICT4 rate control frame {index}: admissible word replacements reach {all_cost} slots, budget {budget}; refusing a whole-frame hold or unchecked output"));
                }
                let mut low = 0usize;
                let mut high = changes.len();
                after = all;
                final_cost = all_cost;
                // Cost can have small discontinuities when RAW/dictionary choices
                // change. Retain an explicitly verified feasible upper endpoint.
                while high > low + 1 {
                    let middle = (low + high) / 2;
                    let candidate = render(middle);
                    let candidate_cost = cost(before, &candidate);
                    evaluations += 1;
                    if candidate_cost <= budget && candidate != before {
                        high = middle;
                        after = candidate;
                        final_cost = candidate_cost;
                    } else {
                        low = middle;
                    }
                }
                selected = high;
                threshold = changes[high - 1].error;
                skip_count = changes[..high].iter().filter(|v| v.skip).count();
                adapted += 1;
            }
            // Pixel metrics compare against the exact author conversion, never the
            // previous reconstruction: accumulated temporal error stays visible.
            let mut changed = 0usize;
            let mut sse = 0.0;
            let mut ok = 0.0;
            let mut max_ok: f64 = 0.0;
            for c in 0..COLS {
                for y in top..top + active {
                    let at = c * 256 + y;
                    for shift in [0, 4] {
                        let a = ((target[at] >> shift) & 15) as usize;
                        let b = ((after[at] >> shift) & 15) as usize;
                        changed += usize::from(a != b);
                        sse += (0..3)
                            .map(|k| (f64::from(palette[a][k]) - f64::from(palette[b][k])).powi(2))
                            .sum::<f64>();
                        let e = colour_distance[a][b].sqrt();
                        ok += e;
                        max_ok = max_ok.max(e);
                    }
                }
            }
            total_changed += changed;
            total_sse += sse;
            total_oklab += ok;
            worst_changed = worst_changed.max(changed);
            peak = peak.max(final_cost);
            total_skips += skip_count;
            let psnr = if sse == 0.0 {
                "null".into()
            } else {
                (10.0 * (255.0_f64.powi(2) / (sse / (pixels * 3) as f64)).log10()).to_string()
            };
            rows.push(format!("{{\"frame\":{index},\"exact_cost_before_rate_control\":{original_cost},\"final_estimated_peak_slots\":{final_cost},\"selected_word_replacements\":{selected},\"temporal_skip_words\":{skip_count},\"changed_pixels\":{changed},\"changed_pixel_percent\":{},\"rgb_mse\":{},\"rgb_psnr_db\":{psnr},\"mean_oklab_delta\":{},\"maximum_pixel_oklab_delta\":{max_ok},\"maximum_selected_word_squared_oklab_error\":{threshold},\"cost_evaluations\":{evaluations}}}", changed as f64 * 100.0 / pixels as f64, sse / (pixels * 3) as f64, ok / pixels as f64));
            result.extend_from_slice(&after);
            if index % 32 == 0 {
                eprintln!("DICT4 rate control: {index}/{} frames, {adapted} adapted, peak {peak}/{budget} slots", frames.len() / FRAME - 1);
            }
        }
        let frame_count = frames.len() / FRAME;
        let denom = frame_count * pixels;
        let psnr = if total_sse == 0.0 {
            "null".into()
        } else {
            (10.0 * (255.0_f64.powi(2) / (total_sse / (denom * 3) as f64)).log10()).to_string()
        };
        let report = format!("{{\"algorithm\":\"DICT4 host-only palette-word projection or temporal SKIP, ordered by per-pixel squared OKLab error; exact record-cost callback\",\"lossy\":{},\"reference\":\"current exact author conversion, including accumulated temporal error\",\"neutral_guard\":\"each reference grayscale pixel stays grayscale\",\"black_padding_preserved\":true,\"whole_frame_holds_allowed\":false,\"source_frames\":{frame_count},\"adapted_frames\":{adapted},\"tick_budget_cpu_slots\":{budget},\"maximum_estimated_tick_cpu_slots\":{peak},\"total_changed_pixels\":{total_changed},\"changed_pixel_percent\":{},\"worst_frame_changed_pixel_percent\":{},\"rgb_mse\":{},\"rgb_psnr_db\":{psnr},\"mean_oklab_delta\":{},\"temporal_skip_words\":{total_skips},\"frames\":[{}]}}",total_changed>0,total_changed as f64*100.0/denom as f64,worst_changed as f64*100.0/pixels as f64,total_sse/(denom*3) as f64,total_oklab/denom as f64,rows.join(","));
        Ok((result, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn palette() -> [[u8; 3]; 16] {
        std::array::from_fn(|i| [i as u8 * 16; 3])
    }
    #[test]
    fn already_fitting_frames_are_byte_identical() {
        let frames = vec![0; FRAME * 3];
        let (actual, report) = convert(&frames, &palette(), 100, |_, _| 99).unwrap();
        assert_eq!(actual, frames);
        assert!(report.contains("\"adapted_frames\":0"));
    }
    #[test]
    fn refuses_unreachable_budget_instead_of_dropping_frame() {
        let mut frames = vec![0; FRAME * 2];
        frames[FRAME + TOP] = 0x11;
        assert!(convert(&frames, &palette(), 100, |_, _| 101)
            .unwrap_err()
            .contains("refusing"));
    }
    #[test]
    fn successful_projection_obeys_callback_and_keeps_bars() {
        let mut frames = vec![0; FRAME * 2];
        for c in 0..COLS {
            for y in (TOP..TOP + 188).step_by(4) {
                let k = (c * 47 + (y - TOP) / 4) as u32;
                let value = 0x2222_2222u32.wrapping_add(k.wrapping_mul(0x0101_0101));
                let at = FRAME + c * 256 + y;
                frames[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        let callback = |before: &[u8], after: &[u8]| {
            let mut words = std::collections::HashSet::new();
            for c in 0..COLS {
                for y in (TOP..TOP + ROWS).step_by(4) {
                    let at = c * 256 + y;
                    if read_word(before, at) != read_word(after, at) {
                        words.insert(read_word(after, at));
                    }
                }
            }
            if words.len() <= 254 {
                100
            } else {
                200
            }
        };
        let (actual, report) = convert(&frames, &palette(), 150, callback).unwrap();
        assert_eq!(&actual[..FRAME], &frames[..FRAME]);
        assert!(actual[FRAME..] != frames[FRAME..]);
        assert!(actual[FRAME..].iter().any(|&v| v != 0));
        assert_eq!(callback(&actual[..FRAME], &actual[FRAME..]), 100);
        assert!(report.contains("\"adapted_frames\":1"));
        for c in 0..COLS {
            for y in (0..TOP).chain(TOP + ACTIVE..256) {
                assert_eq!(actual[FRAME + c * 256 + y], 0);
            }
        }
    }
    #[test]
    fn full_height_metrics_cover_row_zero_and_preserve_zero_edge_columns() {
        let mut frames = vec![0; FRAME * 2];
        for c in 1..COLS - 1 {
            for row in 0..50 {
                let k = (c * 50 + row) as u32;
                let at = FRAME + c * 256 + row * 4;
                let value = 0x2222_2222u32.wrapping_add(k.wrapping_mul(0x0101_0101));
                frames[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        let cost = |a: &[u8], b: &[u8]| {
            let mut words = std::collections::HashSet::new();
            for c in 0..COLS {
                for row in 0..50 {
                    let at = c * 256 + row * 4;
                    if read_word(a, at) != read_word(b, at) {
                        words.insert(read_word(b, at));
                    }
                }
            }
            if words.len() <= 254 {
                100
            } else {
                200
            }
        };
        let metric = Prepared::with_geometry(&palette(), 0, 200, 200);
        let (actual, report) = metric.convert(&frames, 150, cost).unwrap();
        let mut changed = 0;
        for c in 0..COLS {
            for y in 0..200 {
                for shift in [0, 4] {
                    changed += usize::from(
                        (actual[FRAME + c * 256 + y] >> shift) & 15
                            != (frames[FRAME + c * 256 + y] >> shift) & 15,
                    );
                }
            }
        }
        assert!(report.contains(&format!("\"total_changed_pixels\":{changed},")));
        assert!(actual[FRAME..FRAME + 256].iter().all(|&v| v == 0));
        assert!(actual[FRAME + (COLS - 1) * 256..FRAME * 2]
            .iter()
            .all(|&v| v == 0));
        assert!(
            (1..COLS - 1).any(|c| actual[FRAME + c * 256..FRAME + c * 256 + 4]
                != frames[FRAME + c * 256..FRAME + c * 256 + 4])
        );
    }
    #[test]
    fn neutral_guard_and_black_padding_are_hard_constraints() {
        let mut distance = vec![vec![1.0; 256]; 256];
        distance[0x11][0x22] = f64::INFINITY;
        assert!(!word_error(0x11111111, 0x22222222, &distance, false).is_finite());
        assert!(!word_error(0x00001111, 0x11111111, &distance, true).is_finite());
        assert!(word_error(0x00001111, 0x00002222, &distance, true).is_infinite());
        assert!(word_error(0x00001111, 0x00003333, &distance, true).is_finite());
    }
}
