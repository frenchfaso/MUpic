//! Native SD codec9: deterministic dictionary/SKIP/RAW planner and reference decoder.
use crate::sd_timing as timing;
use std::collections::HashMap;

const FRAME: usize = 46080;
const COLS: usize = 180;
const ROWS: usize = 200;
const WORDS: usize = ROWS / 4;
const PREFETCH: usize = 13;
#[cfg(test)]
const TICKS: usize = 3;
const REGIONS: [(usize, usize, usize); 2] = [(0, 2048, 0xe000), (2048, 44032, 0x1800)];
const LUT_BYTES: usize = 1024;
fn address(c: usize) -> usize {
    if c < 8 {
        0xe000 + c * 256
    } else {
        0x1800 + (c - 8) * 256
    }
}
fn compact(ram: &[u8]) -> Vec<u8> {
    REGIONS
        .iter()
        .flat_map(|&(_, n, a)| ram[a..a + n].iter().copied())
        .collect()
}
fn word(bytes: &[u8]) -> [u8; 4] {
    bytes.try_into().unwrap()
}
#[derive(Clone, Debug)]
struct Column {
    index: usize,
    raw: bool,
    code: Vec<u8>,
    escaped: Vec<u8>,
    target: Vec<u8>,
    primitive: timing::Primitive,
    suffix_slots: usize,
}
impl Column {
    fn body_len(&self) -> usize {
        if self.raw {
            self.target.len()
        } else {
            self.code.len() + self.escaped.len()
        }
    }
    fn wire(&self) -> usize {
        2 + self.body_len()
    }
    fn append(&self, out: &mut Vec<u8>) {
        out.extend([
            self.index as u8,
            if self.raw {
                255
            } else {
                self.escaped.len() as u8
            },
        ]);
        if self.raw {
            out.extend(&self.target)
        } else {
            out.extend(&self.code);
            out.extend(&self.escaped)
        }
    }
}
struct Plan {
    lut: Vec<u8>,
    columns: Vec<Column>,
}
fn plan(before: &[u8], after: &[u8]) -> Plan {
    let mut freq: HashMap<[u8; 4], (usize, usize)> = HashMap::new();
    for c in 0..COLS {
        for y in (0..ROWS).step_by(4) {
            let at = c * 256 + y;
            if before[at..at + 4] != after[at..at + 4] {
                let order = freq.len();
                let entry = freq.entry(word(&after[at..at + 4])).or_insert((0, order));
                entry.0 += 1;
            }
        }
    }
    let mut entries = freq.into_iter().collect::<Vec<_>>();
    entries.sort_by_key(|(_, (count, order))| (std::cmp::Reverse(*count), *order));
    entries.truncate(254);
    let mut lut = vec![0; LUT_BYTES];
    let mut lookup = HashMap::new();
    for (slot, (value, _)) in entries.iter().enumerate() {
        lookup.insert(*value, slot as u8);
        for k in 0..4 {
            lut[k * 256 + slot] = value[k];
        }
    }
    let mut columns = Vec::new();
    for c in 0..COLS {
        let old = &before[c * 256..c * 256 + ROWS];
        let target = &after[c * 256..c * 256 + ROWS];
        if old == target {
            continue;
        }
        let mut code = Vec::with_capacity(WORDS);
        let mut escaped = Vec::new();
        for at in (0..ROWS).step_by(4) {
            if old[at..at + 4] == target[at..at + 4] {
                code.push(254);
            } else if let Some(&slot) = lookup.get(&word(&target[at..at + 4])) {
                code.push(slot);
            } else {
                code.push(255);
                escaped.push(at as u8);
                escaped.extend(&target[at..at + 4]);
            }
        }
        let primitive = timing::Primitive::new(&code, escaped.len());
        let dictionary_cost = timing::column(&primitive, escaped.len(), false, c, 2, 0, 0);
        let raw_cost = timing::column(&primitive, escaped.len(), true, c, 2, 0, 0);
        let raw = dictionary_cost.slots() >= raw_cost.slots();
        columns.push(Column {
            index: c,
            raw,
            code,
            escaped,
            target: target.to_vec(),
            primitive,
            suffix_slots: if raw {
                raw_cost.slots()
            } else {
                dictionary_cost.slots()
            },
        });
    }
    Plan { lut, columns }
}
#[derive(Clone, Debug)]
struct Cost {
    body: usize,
    slots: usize,
}
// Costs below cover the current packet after scanout. Reading/preparing the
// next header is charged to this tick, where the player actually performs it.
fn cost(columns: &[Column], reset: bool) -> Cost {
    let body = usize::from(reset) * LUT_BYTES + columns.iter().map(Column::wire).sum::<usize>();
    let prefix = body.min(PREFETCH * ROWS) / PREFETCH * PREFETCH;
    let mut slots = timing::RECORD_CPU;
    let mut at = 0;
    if reset {
        slots += timing::lut(prefix).slots();
        at = LUT_BYTES;
    }
    for column in columns {
        slots += timing::column(
            &column.primitive,
            column.escaped.len(),
            column.raw,
            column.index,
            at + 2,
            prefix,
            body - at,
        )
        .slots();
        at += column.wire();
    }
    Cost { body, slots }
}

struct Costs<'a> {
    columns: &'a [Column],
    wire: Vec<usize>,
    suffix: Vec<usize>,
    // Only columns wholly inside the 2600-byte prefetch can enter these sums.
    // The total cache is bounded by 51 entries per starting column.
    buffered: Vec<Vec<isize>>,
    header_residues: Vec<Vec<usize>>,
    long_bodies: Vec<usize>,
    full_prefix: Vec<isize>,
    reset_prefix: isize,
}
impl<'a> Costs<'a> {
    fn new(columns: &'a [Column]) -> Self {
        let mut wire = vec![0; columns.len() + 1];
        let mut suffix = wire.clone();
        let mut long_bodies = wire.clone();
        let mut header_residues = vec![Vec::new(); 256];
        for (index, column) in columns.iter().enumerate() {
            header_residues[wire[index] & 255].push(index);
            wire[index + 1] = wire[index] + column.wire();
            suffix[index + 1] = suffix[index] + column.suffix_slots;
            long_bodies[index + 1] = long_bodies[index] + usize::from(column.body_len() >= 256);
        }
        let buffered = (0..=columns.len())
            .map(|begin| {
                let mut sums = vec![0];
                for (index, column) in columns.iter().enumerate().skip(begin) {
                    let at = wire[index] - wire[begin];
                    if at + column.wire() > PREFETCH * ROWS {
                        break;
                    }
                    // A large feed with low byte 2 removes the three variable
                    // feed comparisons; the physical source phase remains exact.
                    let full = timing::column(
                        &column.primitive,
                        column.escaped.len(),
                        column.raw,
                        column.index,
                        at + 2,
                        at + 1026,
                        0,
                    )
                    .slots();
                    sums.push(sums.last().unwrap() + full as isize - column.suffix_slots as isize);
                }
                sums
            })
            .collect();
        let mut costs = Self {
            columns,
            wire,
            suffix,
            buffered,
            header_residues,
            long_bodies,
            full_prefix: vec![0; columns.len() + 1],
            reset_prefix: 0,
        };
        for begin in 0..=columns.len() {
            costs.full_prefix[begin] =
                costs.correction(begin, columns.len(), false, PREFETCH * ROWS);
        }
        costs.reset_prefix = costs.correction(0, columns.len(), true, PREFETCH * ROWS);
        costs
    }
    fn body(&self, begin: usize, end: usize, reset: bool) -> usize {
        usize::from(reset) * LUT_BYTES + self.wire[end] - self.wire[begin]
    }
    fn first_after(&self, begin: usize, end: usize, limit: Option<usize>) -> usize {
        limit.map_or(begin, |limit| {
            begin + self.wire[begin..end].partition_point(|&at| at <= limit)
        })
    }
    fn residue_count(&self, residue: usize, begin: usize, end: usize) -> usize {
        let positions = &self.header_residues[residue & 255];
        positions.partition_point(|&index| index < end)
            - positions.partition_point(|&index| index < begin)
    }
    fn correction(&self, begin: usize, end: usize, reset: bool, prefix: usize) -> isize {
        let reset_bytes = usize::from(reset) * LUT_BYTES;
        if prefix <= reset_bytes {
            return 0;
        }
        // Reset adds four whole pages, so cached source phases are unchanged.
        debug_assert_eq!(LUT_BYTES % 256, 0);
        let limit = self.wire[begin] + prefix - reset_bytes;
        let full_end = begin + self.wire[begin + 1..end + 1].partition_point(|&at| at <= limit);
        let mut correction = self.buffered[begin][full_end - begin];
        let small_header = self.first_after(begin, full_end, limit.checked_sub(256));
        let short_body = self.first_after(begin, full_end, limit.checked_sub(258));
        let long_body = self.first_after(begin, full_end, limit.checked_sub(514));
        let short_count =
            full_end - short_body - (self.long_bodies[full_end] - self.long_bodies[short_body]);
        let long_count = self.long_bodies[full_end] - self.long_bodies[long_body];
        correction += (8 * (full_end - small_header)
            + 8 * (self.residue_count(limit, begin, full_end)
                + self.residue_count(limit.wrapping_sub(1), begin, full_end))
            + 9 * (short_count + long_count)) as isize;
        // Only one column can cross the prefetch boundary; retain its complete
        // instruction model, including mixed RAM/REU copies and page crossings.
        if full_end < end && self.wire[full_end] < limit {
            let column = &self.columns[full_end];
            let at = self.wire[full_end] - self.wire[begin] + reset_bytes;
            let actual = timing::column(
                &column.primitive,
                column.escaped.len(),
                column.raw,
                column.index,
                at + 2,
                prefix,
                0,
            )
            .slots();
            correction += actual as isize - column.suffix_slots as isize;
        }
        correction
    }
    fn score(&self, begin: usize, end: usize, reset: bool) -> usize {
        let body = self.body(begin, end, reset);
        let prefix = body.min(PREFETCH * ROWS) / PREFETCH * PREFETCH;
        let correction = if prefix == PREFETCH * ROWS {
            if reset {
                self.reset_prefix
            } else {
                self.full_prefix[begin]
            }
        } else {
            self.correction(begin, end, reset, prefix)
        };
        let lut = if reset {
            timing::lut(prefix).slots()
        } else {
            0
        };
        (timing::RECORD_CPU + lut + self.suffix[end] - self.suffix[begin])
            .checked_add_signed(correction)
            .unwrap()
    }
}
fn switching(first_tick: usize, tick: usize) -> bool {
    (first_tick + tick + 1).is_multiple_of(crate::CACHE_TICKS)
}
fn next_cost(body: Option<usize>, switch_cache: bool) -> usize {
    body.map_or_else(
        || timing::max_next_prepare(switch_cache),
        |body| timing::next_prepare(body, switch_cache),
    )
    .slots()
}
fn partition(
    columns: &[Column],
    first_tick: usize,
    following_body: Option<usize>,
) -> Vec<std::ops::Range<usize>> {
    let n = columns.len();
    let costs = Costs::new(columns);
    let first = (0..=n)
        .map(|end| costs.score(0, end, true))
        .collect::<Vec<_>>();
    let last_prepare = next_cost(following_body, switching(first_tick, 2));
    // A real split supplies an upper bound before enumeration. Add one so the
    // ascending search still selects the first equally good split itself.
    let first_guess = n / 3;
    let second_guess = 2 * n / 3;
    let seed = (first[first_guess]
        + next_cost(
            Some(costs.body(first_guess, second_guess, false)),
            switching(first_tick, 0),
        ))
    .max(
        costs.score(first_guess, second_guess, false)
            + next_cost(
                Some(costs.body(second_guess, n, false)),
                switching(first_tick, 1),
            ),
    )
    .max(costs.score(second_guess, n, false) + last_prepare);
    let mut best = seed + 1;
    let mut cuts = (0, 0);
    // With only three ticks, enumerate the two boundaries directly. Lookahead
    // couples neighbouring records, so the old independent-tick DP is invalid.
    // Keep ascending second/first boundaries and strict improvement for ties.
    for second in 0..=n {
        let third = costs.score(second, n, false) + last_prepare;
        if third >= best {
            continue;
        }
        let middle_prepare =
            next_cost(Some(costs.body(second, n, false)), switching(first_tick, 1));
        for (first_cut, &first_decode) in first[..=second].iter().enumerate() {
            let initial = first_decode
                + next_cost(
                    Some(costs.body(first_cut, second, false)),
                    switching(first_tick, 0),
                );
            if initial >= best {
                continue;
            }
            let middle = costs.score(first_cut, second, false) + middle_prepare;
            let peak = initial.max(middle).max(third);
            if peak < best {
                best = peak;
                cuts = (first_cut, second);
            }
        }
    }
    vec![0..cuts.0, cuts.0..cuts.1, cuts.1..n]
}
// Four-refresh NTSC uses the same lookahead-aware minimax objective. Every
// possible ascending split is considered; cached interval scores make the
// cubic boundary search inexpensive and avoid assuming monotonic costs.
fn partition_four(
    columns: &[Column],
    first_tick: usize,
    following_body: Option<usize>,
) -> Vec<std::ops::Range<usize>> {
    let n = columns.len();
    let costs = Costs::new(columns);
    let size = n + 1;
    let mut decode = vec![0; size * size];
    let mut prepare = vec![0; size * size];
    for start in 0..=n {
        for end in start..=n {
            decode[start * size + end] = costs.score(start, end, false);
            prepare[start * size + end] = next_cost(Some(costs.body(start, end, false)), false);
        }
    }
    let first = (0..=n)
        .map(|end| costs.score(0, end, true))
        .collect::<Vec<_>>();
    // Cache switches add a fixed cost, independent of the following length.
    let extra = |tick| {
        if switching(first_tick, tick) {
            next_cost(Some(0), true) - next_cost(Some(0), false)
        } else {
            0
        }
    };
    let tail = next_cost(following_body, switching(first_tick, 3));
    let mut best = usize::MAX;
    let mut cuts = (0, 0, 0);
    for third in 0..=n {
        let fourth = decode[third * size + n] + tail;
        if fourth >= best {
            continue;
        }
        for second in 0..=third {
            let third_cost = decode[second * size + third] + prepare[third * size + n] + extra(2);
            let suffix = fourth.max(third_cost);
            if suffix >= best {
                continue;
            }
            for first_cut in 0..=second {
                let initial = first[first_cut] + prepare[first_cut * size + second] + extra(0);
                if initial >= best {
                    continue;
                }
                let middle =
                    decode[first_cut * size + second] + prepare[second * size + third] + extra(1);
                let peak = initial.max(middle).max(suffix);
                if peak < best {
                    best = peak;
                    cuts = (first_cut, second, third);
                }
            }
        }
    }
    vec![0..cuts.0, cuts.0..cuts.1, cuts.1..cuts.2, cuts.2..n]
}

fn planned_peak(
    columns: &[Column],
    parts: &[std::ops::Range<usize>],
    first_tick: usize,
    reset_first: bool,
    following_body: Option<usize>,
) -> usize {
    let records = parts
        .iter()
        .enumerate()
        .map(|(tick, range)| cost(&columns[range.clone()], reset_first && tick == 0))
        .collect::<Vec<_>>();
    records
        .iter()
        .enumerate()
        .map(|(tick, record)| {
            record.slots
                + next_cost(
                    records
                        .get(tick + 1)
                        .map(|next| next.body)
                        .or(following_body),
                    switching(first_tick, tick),
                )
        })
        .max()
        .unwrap()
}
/// Keep the authoritative dictionary and packet split that admitted a frame.
pub(super) struct Prepared {
    plan: Plan,
    parts: Vec<std::ops::Range<usize>>,
    peak: usize,
    first_tick: usize,
    following_body: Option<usize>,
}
impl Prepared {
    pub(super) fn peak(&self) -> usize {
        self.peak
    }
}
#[cfg(test)]
pub(super) fn prepare_at(
    before: &[u8],
    after: &[u8],
    first_tick: usize,
    following_body: Option<usize>,
) -> Prepared {
    prepare_profile(before, after, first_tick, following_body, TICKS)
}
pub(super) fn prepare_profile(
    before: &[u8],
    after: &[u8],
    first_tick: usize,
    following_body: Option<usize>,
    ticks: usize,
) -> Prepared {
    let plan = plan(before, after);
    let parts = if ticks == 4 {
        partition_four(&plan.columns, first_tick, following_body)
    } else {
        partition(&plan.columns, first_tick, following_body)
    };
    let peak = planned_peak(&plan.columns, &parts, first_tick, true, following_body);
    Prepared {
        plan,
        parts,
        peak,
        first_tick,
        following_body,
    }
}
#[cfg(test)]
pub(super) fn prepare(before: &[u8], after: &[u8]) -> Prepared {
    prepare_at(before, after, 0, None)
}
#[cfg(test)]
pub(super) fn peak(before: &[u8], after: &[u8]) -> usize {
    prepare(before, after).peak()
}
#[derive(Clone)]
struct State {
    lut: Vec<u8>,
    initialized: bool,
    last: Option<usize>,
}
impl State {
    fn new() -> Self {
        Self {
            lut: vec![0; 1024],
            initialized: false,
            last: None,
        }
    }
}
fn decode(ram: &mut [u8], state: &mut State, payload: &[u8]) -> Result<bool, String> {
    if ram.len() != 65536 || payload.len() < 2 {
        return Err("Invalid SD RAM/payload".into());
    }
    let flags = payload[0];
    let count = payload[1] as usize;
    if flags & !3 != 0 || count > COLS {
        return Err("Invalid SD flags/count".into());
    }
    let mut next = state.clone();
    let mut at = 2;
    if flags & 2 != 0 {
        next.lut = payload
            .get(at..at + LUT_BYTES)
            .ok_or("Truncated SD LUT")?
            .to_vec();
        at += LUT_BYTES;
        if (0..4).any(|k| next.lut[k * 256 + 254] != 0 || next.lut[k * 256 + 255] != 0) {
            return Err("Reserved SD LUT entries must be zero".into());
        }
        next.initialized = true;
        next.last = None;
    }
    if count > 0 && !next.initialized {
        return Err("SD columns require a dictionary reset".into());
    }
    let mut writes = Vec::with_capacity(count);
    for _ in 0..count {
        let header = payload
            .get(at..at + 2)
            .ok_or("Truncated SD column header")?;
        at += 2;
        let col = header[0] as usize;
        let length = header[1] as usize;
        if col >= COLS || next.last.is_some_and(|p| col <= p) {
            return Err("SD columns must be ascending and unique".into());
        }
        let dest = address(col);
        let mut output = ram[dest..dest + ROWS].to_vec();
        if length == 255 {
            output = payload
                .get(at..at + ROWS)
                .ok_or("Truncated SD RAW200")?
                .to_vec();
            at += ROWS;
        } else {
            if length > WORDS * 5 || !length.is_multiple_of(5) {
                return Err("Invalid SD escape length".into());
            }
            let code = payload
                .get(at..at + WORDS)
                .ok_or("Truncated SD index plane")?;
            at += WORDS;
            let escaped = payload
                .get(at..at + length)
                .ok_or("Truncated SD escaped words")?;
            at += length;
            let mut literal = 0;
            for (row, &slot) in code.iter().enumerate() {
                let y = row * 4;
                if slot < 254 {
                    for k in 0..4 {
                        output[y + k] = next.lut[k * 256 + slot as usize];
                    }
                } else if slot == 255 {
                    let e = escaped
                        .get(literal..literal + 5)
                        .ok_or("Missing SD escaped word")?;
                    if e[0] as usize != y {
                        return Err("SD escape offset must match its255 index".into());
                    }
                    output[y..y + 4].copy_from_slice(&e[1..]);
                    literal += 5;
                }
            }
            if literal != length {
                return Err("Extra SD escaped words".into());
            }
        }
        writes.push((dest, output));
        next.last = Some(col);
    }
    if at != payload.len() {
        return Err("Trailing SD bytes".into());
    }
    for (dest, bytes) in writes {
        ram[dest..dest + ROWS].copy_from_slice(&bytes);
    }
    *state = next;
    Ok(flags & 1 != 0)
}
#[cfg(test)]
fn group(before: &[u8], after: Option<&[u8]>) -> Result<(Vec<Vec<u8>>, usize), String> {
    if before.len() != FRAME || after.is_some_and(|f| f.len() != FRAME) {
        return Err("SD frame must contain 46080 page-stride bytes".into());
    }
    let prepared = after.map(|target| prepare_at(before, target, 0, None));
    group_from_plan(before, after, prepared, 0, TICKS)
}
#[cfg(test)]
pub(super) fn holds(before: &[u8], first_tick: usize) -> Result<(Vec<Vec<u8>>, usize), String> {
    holds_profile(before, first_tick, TICKS)
}
pub(super) fn holds_profile(
    before: &[u8],
    first_tick: usize,
    ticks: usize,
) -> Result<(Vec<Vec<u8>>, usize), String> {
    if before.len() != FRAME {
        return Err("SD frame must contain 46080 page-stride bytes".into());
    }
    group_from_plan(before, None, None, first_tick, ticks)
}
pub(super) fn group_prepared(
    before: &[u8],
    after: &[u8],
    prepared: Prepared,
) -> Result<(Vec<Vec<u8>>, usize), String> {
    if before.len() != FRAME || after.len() != FRAME {
        return Err("SD frame must contain 46080 page-stride bytes".into());
    }
    let ticks = prepared.parts.len();
    group_from_plan(before, Some(after), Some(prepared), 0, ticks)
}
fn group_from_plan(
    before: &[u8],
    after: Option<&[u8]>,
    prepared: Option<Prepared>,
    holds_tick: usize,
    ticks: usize,
) -> Result<(Vec<Vec<u8>>, usize), String> {
    let mut ram = vec![0xaa; 65536];
    for &(at, count, address) in &REGIONS {
        ram[address..address + count].copy_from_slice(&before[at..at + count]);
    }
    let mut decoder = State::new();
    let expected_peak = prepared.as_ref().map(Prepared::peak);
    let (plan, parts, first_tick, following_body) = prepared.map_or_else(
        || (None, vec![0..0; ticks], holds_tick, Some(0)),
        |prepared| {
            (
                Some(prepared.plan),
                prepared.parts,
                prepared.first_tick,
                prepared.following_body,
            )
        },
    );
    let peak = planned_peak(
        plan.as_ref().map_or(&[][..], |p| &p.columns),
        &parts,
        first_tick,
        after.is_some(),
        following_body,
    );
    let mut packets = Vec::with_capacity(ticks);
    for (tick, range) in parts.into_iter().enumerate() {
        let reset = after.is_some() && tick == 0;
        let complete = after.is_some() && tick + 1 == ticks;
        let columns = plan.as_ref().map_or(&[][..], |p| &p.columns[range]);
        let cost = cost(columns, reset);
        let mut payload = vec![
            u8::from(reset) * 2 + u8::from(complete),
            columns.len() as u8,
        ];
        if reset {
            payload.extend_from_slice(&plan.as_ref().unwrap().lut);
        }
        for column in columns {
            column.append(&mut payload);
        }
        if payload.len() != cost.body + 2 || payload.len() > u16::MAX as usize {
            return Err("SD payload cost/size mismatch".into());
        }
        if decode(&mut ram, &mut decoder, &payload)? != complete {
            return Err("SD decoder completion mismatch".into());
        }
        packets.push(payload);
    }
    if compact(&ram) != after.unwrap_or(before) {
        return Err("SD reference decoder reconstruction mismatch".into());
    }
    if expected_peak.is_some_and(|expected| expected != peak) {
        return Err("SD prepared plan cost mismatch".into());
    }
    Ok((packets, peak))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    // Frozen packet-building path used before the projection reused its plan.
    // It deliberately rebuilds and repartitions the winner independently.
    fn reference_group(before: &[u8], after: &[u8]) -> (Vec<Vec<u8>>, usize) {
        let plan = plan(before, after);
        let parts = partition(&plan.columns, 0, None);
        let mut packets = Vec::new();
        let peak = planned_peak(&plan.columns, &parts, 0, true, None);
        for (tick, range) in parts.into_iter().enumerate() {
            let reset = tick == 0;
            let complete = tick + 1 == TICKS;
            let columns = &plan.columns[range];
            let mut payload = vec![
                u8::from(reset) * 2 + u8::from(complete),
                columns.len() as u8,
            ];
            if reset {
                payload.extend_from_slice(&plan.lut);
            }
            for column in columns {
                column.append(&mut payload);
            }
            packets.push(payload);
        }
        (packets, peak)
    }

    #[test]
    fn prepared_packets_match_rebuilt_winners_and_reject_mispaired_frames() {
        let mut before = vec![0; FRAME];
        for variant in 0..8 {
            let mut after = before.clone();
            for c in 1..COLS - 1 {
                for y in 0..ROWS {
                    if (c + y + variant) % 7 != 0 {
                        after[c * 256 + y] = (c * 13 + y * 31 + variant * 97) as u8;
                    }
                }
            }
            let prepared = prepare(&before, &after);
            let expected = reference_group(&before, &after);
            assert_eq!(prepared.peak(), expected.1);
            assert_eq!(group_prepared(&before, &after, prepared).unwrap(), expected);
            before = after;
        }
        let mut after = before.clone();
        after[256] ^= 0xff;
        let prepared = prepare(&before, &after);
        let mut unrelated = after.clone();
        unrelated[512] ^= 0xff;
        assert_eq!(
            group_prepared(&before, &unrelated, prepared).unwrap_err(),
            "SD reference decoder reconstruction mismatch"
        );
        let mut prepared = prepare(&before, &after);
        prepared.peak += 1;
        assert_eq!(
            group_prepared(&before, &after, prepared).unwrap_err(),
            "SD prepared plan cost mismatch"
        );
    }

    #[test]
    fn packets_match_qualified_codec9_fixtures() {
        // Fixed lossless transitions with the calibrated lookahead planner.
        let fixtures = [
            (
                277830,
                [3730, 3278, 3278],
                [
                    "48bff77fcaca593ac47df84888e609dcaf1ac1f3fe4a3d113472f3ec313a1cff",
                    "18cc960add26686396c6b8bf8d0476af5f8039575bf2f1344f0cbbe2517308df",
                    "43f194650cb1cff16cb4344c1754ea1f4e47ec6f09a3ab911927b2d45b609005",
                ],
            ),
            (
                676320,
                [11992, 12122, 12122],
                [
                    "5ebdf690af5830d81925ce8ddbde46ea90de7581dc6aab8a901a1b2d0d65fc41",
                    "3d7c2f085edd4f4d8d7aeafdfdcbb1d99f0fcef31c46e965d40d38f4736f16cb",
                    "d403d1cebe3ef25a9b617e6c6628aebac307d21da77dfd1dd3eb27590c39cedd",
                ],
            ),
        ];
        for (variant, (peak, lengths, hashes)) in fixtures.into_iter().enumerate() {
            let mut before = vec![0; FRAME];
            let mut after = before.clone();
            for c in 1..COLS - 1 {
                for y in 0..ROWS {
                    before[c * 256 + y] = if variant == 0 {
                        ((c + y / 8) & 15) as u8 * 17
                    } else {
                        0
                    };
                    after[c * 256 + y] = if variant == 0 {
                        ((c + y / 8 + 1) & 15) as u8 * 17
                    } else {
                        ((c * 50 + y / 4 + 1) as u32).to_le_bytes()[y % 4]
                    };
                }
            }
            let (packets, actual_peak) = group(&before, Some(&after)).unwrap();
            assert_eq!(actual_peak, peak);
            assert_eq!(self::peak(&before, &after), peak);
            for ((packet, length), hash) in packets.iter().zip(lengths).zip(hashes) {
                assert_eq!(packet.len(), length);
                assert_eq!(format!("{:x}", Sha256::digest(packet)), hash);
            }
            assert_eq!(
                group(&after, None).unwrap(),
                (vec![vec![0, 0]; TICKS], 1144)
            );
        }
    }

    #[test]
    fn dictionary_ties_follow_first_column_row_and_skip_unchanged_words() {
        let before = vec![0; FRAME];
        let mut after = before.clone();
        let a = [1, 2, 3, 4];
        let b = [9, 8, 7, 6];
        for (col, row, value) in [(0, 0, a), (0, 1, b), (1, 0, b), (1, 1, a)] {
            after[col * 256 + row * 4..col * 256 + row * 4 + 4].copy_from_slice(&value);
        }
        let p = plan(&before, &after);
        assert_eq!(p.columns.len(), 2);
        assert_eq!((0..4).map(|k| p.lut[k * 256]).collect::<Vec<_>>(), a);
        assert_eq!((0..4).map(|k| p.lut[k * 256 + 1]).collect::<Vec<_>>(), b);
        assert_eq!(&p.columns[0].code[..2], &[0, 1]);
        assert!(p.columns[0].code[2..].iter().all(|&v| v == 254));
        assert_eq!(p.lut, plan(&before, &after).lut);
    }

    fn reset_payload(count: u8) -> Vec<u8> {
        let mut packet = vec![2, count];
        packet.extend([0; LUT_BYTES]);
        packet
    }

    #[test]
    fn malformed_dictionary_records_preserve_ram_and_decoder_state() {
        let mut bad = vec![
            vec![4, 0],
            vec![2, 0],
            vec![0, 1],
            vec![0, 0, 5],
            vec![0, 181],
        ];
        let mut packet = reset_payload(0);
        packet[2 + 254] = 1;
        bad.push(packet);
        for header in [[180, 255], [0, 1], [0, 251], [0, 252]] {
            let mut packet = reset_payload(1);
            packet.extend(header);
            bad.push(packet);
        }
        for (length, codes, escaped) in [
            (0, vec![255; WORDS], vec![]),
            (
                5,
                [vec![255], vec![254; WORDS - 1]].concat(),
                vec![4, 1, 2, 3, 4],
            ),
            (5, vec![254; WORDS], vec![0, 1, 2, 3, 4]),
        ] {
            let mut packet = reset_payload(1);
            packet.extend([0, length]);
            packet.extend(codes);
            packet.extend(escaped);
            bad.push(packet);
        }
        // The first column is valid; a truncated second column must undo all writes.
        let mut packet = reset_payload(2);
        packet.extend([0, 255]);
        packet.extend([7; ROWS]);
        packet.extend([1, 255]);
        bad.push(packet);
        for packet in bad {
            let mut ram = vec![0; 65536];
            let mut state = State::new();
            assert!(decode(&mut ram, &mut state, &packet).is_err());
            assert!(!state.initialized);
            assert_eq!(state.last, None);
            assert!(ram.iter().all(|&v| v == 0));
        }
        let mut ram = vec![0; 65536];
        let mut state = State::new();
        let mut packet = reset_payload(1);
        packet.extend([9, 255]);
        packet.extend([7; ROWS]);
        decode(&mut ram, &mut state, &packet).unwrap();
        let before = ram.clone();
        assert!(decode(&mut ram, &mut state, &[0, 1, 9, 255]).is_err());
        assert_eq!(ram, before);
        assert_eq!(state.last, Some(9));
        let mut packet = reset_payload(0);
        packet[0] = 3;
        assert!(decode(&mut ram, &mut state, &packet).unwrap());
        assert_eq!(state.last, None);
        assert!(!decode(&mut ram, &mut state, &[0, 0]).unwrap());
    }

    #[test]
    fn longest_escape_body_covers_first_last_rows_without_touching_shadow_ram() {
        let mut packet = reset_payload(1);
        packet.extend([1, 250]);
        packet.extend([255; WORDS]);
        for row in 0..WORDS {
            packet.push((row * 4) as u8);
            packet.extend([row as u8 + 1; 4]);
        }
        let mut ram = vec![0xaa; 65536];
        let mut state = State::new();
        decode(&mut ram, &mut state, &packet).unwrap();
        assert_eq!(&ram[address(1)..address(1) + 4], &[1; 4]);
        assert_eq!(&ram[address(1) + 196..address(1) + ROWS], &[50; 4]);
        assert!(ram[0xc400..0xe000].iter().all(|&v| v == 0xaa));
        assert!(ram[address(1) + ROWS..address(1) + 256]
            .iter()
            .all(|&v| v == 0xaa));
    }
}

#[cfg(test)]
mod timing_partition_tests {
    use super::*;

    fn fixture(n: usize, variant: usize) -> Vec<Column> {
        let mut rng = 0x7a68_cde1_u32.wrapping_add(variant as u32);
        (0..n)
            .map(|index| {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let raw = match variant {
                    0 => true,
                    1 => false,
                    2 => index % 2 == 0,
                    _ => rng & 1 != 0,
                };
                let escapes = if variant < 3 {
                    0
                } else {
                    (rng as usize >> 8) % 51
                };
                let mut code = vec![254; WORDS];
                code[..escapes].fill(255);
                if variant == 1 {
                    code.fill(0);
                }
                let escaped = vec![0; escapes * 5];
                let primitive = timing::Primitive::new(&code, escaped.len());
                let suffix_slots =
                    timing::column(&primitive, escaped.len(), raw, index, 2, 0, 0).slots();
                Column {
                    index,
                    raw,
                    code,
                    escaped,
                    target: vec![0; ROWS],
                    primitive,
                    suffix_slots,
                }
            })
            .collect()
    }

    #[test]
    fn cached_ranges_match_full_player_cost_across_prefetch_edges() {
        for n in [0, 1, 8, 12, 13, 16, 49, 50, 51, 100, 180] {
            for variant in 0..7 {
                let columns = fixture(n, variant);
                let costs = Costs::new(&columns);
                for end in 0..=n {
                    assert_eq!(
                        costs.score(0, end, true),
                        cost(&columns[..end], true).slots,
                        "reset n={n},variant={variant},end={end}"
                    );
                    for begin in 0..=end {
                        assert_eq!(
                            costs.score(begin, end, false),
                            cost(&columns[begin..end], false).slots,
                            "n={n},variant={variant},range={begin}..{end}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn four_refresh_partitions_match_independent_exhaustive_record_costs() {
        for n in [0, 1, 3, 8, 12] {
            for variant in 0..4 {
                let columns = fixture(n, variant);
                for first_tick in [0, 16, 17, 18, 19] {
                    for following in [None, Some(0), Some(2587)] {
                        let mut expected = (usize::MAX, Vec::new());
                        for third in 0..=n {
                            for second in 0..=third {
                                for first in 0..=second {
                                    let parts =
                                        vec![0..first, first..second, second..third, third..n];
                                    let peak =
                                        planned_peak(&columns, &parts, first_tick, true, following);
                                    if peak < expected.0 {
                                        expected = (peak, parts);
                                    }
                                }
                            }
                        }
                        let actual = partition_four(&columns, first_tick, following);
                        assert_eq!(
                            actual, expected.1,
                            "n={n} variant={variant} first_tick={first_tick}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn partition_is_optimal_with_next_header_work_and_cache_switches() {
        for n in 0..=16 {
            for variant in 0..4 {
                let columns = fixture(n, variant);
                for first_tick in [0, 17, 18, 19] {
                    for following_body in [None, Some(0)] {
                        let mut expected = (usize::MAX, 0, 0);
                        for second in 0..=n {
                            for first in 0..=second {
                                let parts = [0..first, first..second, second..n];
                                let peak = planned_peak(
                                    &columns,
                                    &parts,
                                    first_tick,
                                    true,
                                    following_body,
                                );
                                if peak < expected.0 {
                                    expected = (peak, first, second);
                                }
                            }
                        }
                        let actual = partition(&columns, first_tick, following_body);
                        assert_eq!(
                            actual,
                            vec![0..expected.1, expected.1..expected.2, expected.2..n],
                            "n={n},variant={variant},tick={first_tick}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn last_tick_reserves_unknown_lookahead_and_cache_marker() {
        let columns = fixture(80, 2);
        let parts = partition(&columns, 17, None);
        let ticks = parts
            .iter()
            .enumerate()
            .map(|(tick, range)| cost(&columns[range.clone()], tick == 0))
            .collect::<Vec<_>>();
        let expected = (ticks[0].slots + timing::next_prepare(ticks[1].body, false).slots())
            .max(ticks[1].slots + timing::next_prepare(ticks[2].body, false).slots())
            .max(ticks[2].slots + timing::max_next_prepare(true).slots());
        assert_eq!(planned_peak(&columns, &parts, 17, true, None), expected);
        assert!(timing::max_next_prepare(true).slots() > timing::max_next_prepare(false).slots());
    }

    #[test]
    fn final_frame_reclaims_unknown_lookahead_and_holds_include_cache_switch() {
        let columns = fixture(80, 2);
        for first_tick in [0, 17, 18, 19] {
            let unknown = partition(&columns, first_tick, None);
            let final_frame = partition(&columns, first_tick, Some(0));
            assert!(
                planned_peak(&columns, &final_frame, first_tick, true, Some(0))
                    <= planned_peak(&columns, &unknown, first_tick, true, None)
            );
            let (packets, peak) = holds(&vec![0; FRAME], first_tick).unwrap();
            assert_eq!(packets, vec![vec![0, 0]; TICKS]);
            assert_eq!(
                peak,
                timing::RECORD_CPU + timing::next_prepare(0, first_tick >= 17).slots()
            );
        }
    }
}
