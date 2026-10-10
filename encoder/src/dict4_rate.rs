//! Bounded perceptual word projection for the native SD dictionary/SKIP decoder.
//! Every admitted transition is checked by the authoritative packet cost model.
use std::collections::HashMap;

const FRAME: usize = 46080;
const COLS: usize = 180;
const ROWS: usize = 200;

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
fn word_error(a: u32, b: u32, distance: &[Vec<f64>]) -> f64 {
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
}

/// Reuse the palette metric across transitions in the streaming encoder.
pub(super) struct Prepared {
    distance: Vec<Vec<f64>>,
}
impl Prepared {
    pub(super) fn new(palette: &[[u8; 3]; 16]) -> Self {
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
        Self { distance }
    }
    pub(super) fn project<F>(
        &self,
        before: &[u8],
        target: &[u8],
        budget: usize,
        cost: F,
    ) -> Result<Vec<u8>, String>
    where
        F: Fn(&[u8], &[u8]) -> usize,
    {
        if before.len() != FRAME || target.len() != FRAME || budget == 0 {
            return Err("SD projection requires two whole frames and positive budget".into());
        }
        if cost(before, target) <= budget {
            return Ok(target.to_vec());
        }
        let mut frequency: HashMap<u32, (usize, usize)> = HashMap::new();
        for c in 0..COLS {
            for y in (0..ROWS).step_by(4) {
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
        let mut nearest: HashMap<u32, (u32, f64)> = HashMap::new();
        let mut changes = Vec::new();
        for c in 0..COLS {
            for y in (0..ROWS).step_by(4) {
                let at = c * 256 + y;
                let wanted = read_word(target, at);
                let old = read_word(before, at);
                if wanted == old {
                    continue;
                }
                let &mut (mut value, mut error) = nearest.entry(wanted).or_insert_with(|| {
                    let mut best = (wanted, f64::INFINITY);
                    for &candidate in &dictionary {
                        let e = word_error(wanted, candidate, &self.distance);
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
                let old_error = word_error(wanted, old, &self.distance);
                let skip = old_error <= error;
                if skip {
                    value = old;
                    error = old_error;
                }
                if error.is_finite() && value != wanted {
                    changes.push(Change { at, value, error });
                }
            }
        }
        changes.sort_by(|a, b| a.error.total_cmp(&b.error).then(a.at.cmp(&b.at)));
        let render = |count: usize| {
            let mut output = target.to_vec();
            for change in &changes[..count] {
                output[change.at..change.at + 4].copy_from_slice(&change.value.to_le_bytes());
            }
            output
        };
        let all = render(changes.len());
        let all_cost = cost(before, &all);
        if all_cost > budget || all == before {
            return Err(format!("SD rate control: admissible word replacements reach {all_cost} slots, budget {budget}; refusing a whole-frame hold or unchecked output"));
        }
        let mut low = 0usize;
        let mut high = changes.len();
        let mut after = all;
        // Cost can have small discontinuities when RAW/dictionary choices
        // change. Retain an explicitly verified feasible upper endpoint.
        while high > low + 1 {
            let middle = (low + high) / 2;
            let candidate = render(middle);
            let candidate_cost = cost(before, &candidate);
            if candidate_cost <= budget && candidate != before {
                high = middle;
                after = candidate;
            } else {
                low = middle;
            }
        }
        Ok(after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sd, PALETTE, RATE_BUDGET};
    use sha2::{Digest, Sha256};

    #[test]
    fn already_fitting_transition_is_byte_identical() {
        let before = vec![0; FRAME];
        let mut target = before.clone();
        target[256] = 0x22;
        let metric = Prepared::new(&PALETTE);
        assert_eq!(
            metric.project(&before, &target, 100, |_, _| 99).unwrap(),
            target
        );
    }

    #[test]
    fn invalid_bounds_and_unreachable_budget_are_rejected() {
        let metric = Prepared::new(&PALETTE);
        let before = vec![0; FRAME];
        let mut target = before.clone();
        target[256] = 0x11;
        assert!(metric
            .project(&before, &target, 100, |_, _| 101)
            .unwrap_err()
            .contains("refusing"));
        assert!(metric
            .project(&before, &target[..FRAME - 1], 100, |_, _| 99)
            .is_err());
        assert!(metric.project(&before, &target, 0, |_, _| 0).is_err());
    }

    #[test]
    fn projection_matches_qualified_fixture_and_preserves_bars_and_neutral_pixels() {
        let before = vec![0; FRAME];
        let mut target = before.clone();
        for c in 1..COLS - 1 {
            for row in 0..ROWS / 4 {
                let k = (c * 50 + row) as u32;
                let at = c * 256 + row * 4;
                let value = 0x2222_2222u32.wrapping_add(k.wrapping_mul(0x0101_0101));
                target[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        let actual = Prepared::new(&PALETTE)
            .project(&before, &target, RATE_BUDGET, sd::peak)
            .unwrap();
        // Recorded from the original codec9 rate-control path before refactoring.
        assert_eq!(
            format!("{:x}", Sha256::digest(&actual)),
            "b4f5f109946e3d2c0138b8795b6469af17e443a0fd1db45e829e76ec8b93d017"
        );
        assert_eq!(sd::peak(&before, &actual), 424806);
        assert!(actual != target && actual != before);
        assert!(actual[..256].iter().all(|&v| v == 0));
        assert!(actual[(COLS - 1) * 256..].iter().all(|&v| v == 0));
        for c in 0..COLS {
            assert!(actual[c * 256 + ROWS..(c + 1) * 256]
                .iter()
                .all(|&v| v == 0));
            for y in 0..ROWS {
                for shift in [0, 4] {
                    if grey(((target[c * 256 + y] >> shift) & 15) as usize) {
                        assert!(grey(((actual[c * 256 + y] >> shift) & 15) as usize));
                    }
                }
            }
        }
        assert!((1..COLS - 1).any(|c| actual[c * 256..c * 256 + 4] != target[c * 256..c * 256 + 4]));
    }
}
