//! Native SD codec9: deterministic dictionary/SKIP/RAW planner and reference decoder.
use std::collections::HashMap;

const FRAME: usize = 46080;
const COLS: usize = 180;
const ROWS: usize = 200;
const WORDS: usize = ROWS / 4;
const PREFETCH: usize = 13;
const TICKS: usize = 3;
const REGIONS: [(usize, usize, usize); 2] = [(0, 2048, 0xe000), (2048, 44032, 0x1800)];
const LUT_BYTES: usize = 1024;
const COL_SETUP: usize = 500;
const RECORD_SETUP: usize = 5000;
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
fn primitive_cycles(code: &[u8], escaped: usize) -> usize {
    let dictionary = code.iter().filter(|&&v| v < 254).count();
    let control = code.len() - dictionary;
    // Measured NMOS primitive instruction counts, including RTS and all SMC.
    // Worst source alignment adds max(47,escaped_bytes-1). Each taken BCS is
    // conservatively allowed one extra cycle for an unknown final code page.
    33 + 16 * code.len()
        + 45 * dictionary
        + 14 * control
        + control
        + if escaped > 0 {
            4 + 62 * (escaped / 5)
        } else {
            0
        }
        + (code.len() - 1).max(escaped.saturating_sub(1))
}
#[derive(Clone, Debug)]
struct Column {
    index: usize,
    raw: bool,
    code: Vec<u8>,
    escaped: Vec<u8>,
    target: Vec<u8>,
    cpu: usize,
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
        let cpu = primitive_cycles(&code, escaped.len());
        let raw = 63 * (2 + WORDS + escaped.len()) + cpu + COL_SETUP >= 63 * (2 + ROWS) + COL_SETUP;
        columns.push(Column {
            index: c,
            raw,
            code,
            escaped,
            target: target.to_vec(),
            cpu: if raw { 0 } else { cpu },
        });
    }
    Plan { lut, columns }
}
#[derive(Clone, Debug)]
struct Cost {
    body: usize,
    slots: usize,
}
fn cost(columns: &[Column], reset: bool) -> Cost {
    let mut body = if reset { LUT_BYTES } else { 0 };
    for c in columns {
        body += c.wire();
    }
    let prefix = (body.min(PREFETCH * ROWS) / PREFETCH) * PREFETCH;
    let mut at = if reset { LUT_BYTES } else { 0 };
    let mut raw_prefix = 0;
    for c in columns {
        at += 2;
        if c.raw {
            raw_prefix += prefix.saturating_sub(at).min(c.target.len());
        }
        at += c.body_len();
    }
    let cpu = RECORD_SETUP
        + columns.iter().map(|c| c.cpu + COL_SETUP).sum::<usize>()
        + usize::from(reset) * LUT_BYTES * 15
        + raw_prefix * 15;
    Cost {
        body,
        slots: cpu + 63 * (body + 4 - prefix),
    }
}
fn partition(columns: &[Column]) -> Vec<std::ops::Range<usize>> {
    let n = columns.len();
    // Cumulative wire/CPU/raw lengths make candidate range costs O(log n).
    // This matters when the modern encoder tries several perceptual thresholds.
    let mut wire = vec![0; n + 1];
    let mut cpu = vec![0; n + 1];
    let mut raw = vec![0; n + 1];
    for (i, c) in columns.iter().enumerate() {
        wire[i + 1] = wire[i] + c.wire();
        cpu[i + 1] = cpu[i] + c.cpu + COL_SETUP;
        raw[i + 1] = raw[i] + if c.raw { c.target.len() } else { 0 };
    }
    let raw_before = |at: usize| {
        let i = wire.partition_point(|&v| v <= at).saturating_sub(1).min(n);
        raw[i]
            + if i < n && columns[i].raw {
                at.saturating_sub(wire[i] + 2).min(columns[i].target.len())
            } else {
                0
            }
    };
    let score = |begin: usize, end: usize, reset: bool| {
        let lut = if reset { LUT_BYTES } else { 0 };
        let body = lut + wire[end] - wire[begin];
        let prefix = body.min(PREFETCH * ROWS) / PREFETCH * PREFETCH;
        let raw_prefix = raw_before(wire[begin] + prefix.saturating_sub(lut)) - raw[begin];
        RECORD_SETUP + cpu[end] - cpu[begin] + lut * 15 + raw_prefix * 15 + 63 * (body + 4 - prefix)
    };
    let mut dp = vec![vec![usize::MAX; n + 1]; TICKS + 1];
    let mut prev = vec![vec![0; n + 1]; TICKS + 1];
    dp[0][0] = 0;
    for t in 1..=TICKS {
        for end in 0..=n {
            for begin in 0..=end {
                if dp[t - 1][begin] == usize::MAX {
                    continue;
                }
                let peak = dp[t - 1][begin].max(score(begin, end, t == 1));
                if peak < dp[t][end] {
                    dp[t][end] = peak;
                    prev[t][end] = begin;
                }
            }
        }
    }
    let mut parts = Vec::new();
    let mut end = n;
    for t in (1..=TICKS).rev() {
        let begin = prev[t][end];
        parts.push(begin..end);
        end = begin;
    }
    parts.reverse();
    parts
}
pub(super) fn peak(before: &[u8], after: &[u8]) -> usize {
    let p = plan(before, after);
    partition(&p.columns)
        .into_iter()
        .enumerate()
        .map(|(tick, range)| cost(&p.columns[range], tick == 0).slots)
        .max()
        .unwrap()
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
pub(super) fn group(before: &[u8], after: Option<&[u8]>) -> Result<(Vec<Vec<u8>>, usize), String> {
    if before.len() != FRAME || after.is_some_and(|f| f.len() != FRAME) {
        return Err("SD frame must contain 46080 page-stride bytes".into());
    }
    let mut ram = vec![0xaa; 65536];
    for &(at, count, address) in &REGIONS {
        ram[address..address + count].copy_from_slice(&before[at..at + count]);
    }
    let mut decoder = State::new();
    let plan = after.map(|target| plan(before, target));
    let parts = plan
        .as_ref()
        .map_or_else(|| vec![0..0; TICKS], |p| partition(&p.columns));
    let mut packets = Vec::with_capacity(TICKS);
    let mut peak = 0;
    for (tick, range) in parts.into_iter().enumerate() {
        let reset = after.is_some() && tick == 0;
        let complete = after.is_some() && tick + 1 == TICKS;
        let columns = plan.as_ref().map_or(&[][..], |p| &p.columns[range]);
        let cost = cost(columns, reset);
        peak = peak.max(cost.slots);
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
    Ok((packets, peak))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn packets_match_qualified_codec9_fixtures() {
        // Recorded from the original codec9 encoder before removing legacy paths.
        let fixtures = [
            (
                280540,
                [3730, 3278, 3278],
                [
                    "48bff77fcaca593ac47df84888e609dcaf1ac1f3fe4a3d113472f3ec313a1cff",
                    "18cc960add26686396c6b8bf8d0476af5f8039575bf2f1344f0cbbe2517308df",
                    "43f194650cb1cff16cb4344c1754ea1f4e47ec6f09a3ab911927b2d45b609005",
                ],
            ),
            (
                676372,
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
                (vec![vec![0, 0]; TICKS], 5252)
            );
        }
    }

    #[test]
    fn partition_matches_brute_force_and_counts_prefetched_raw_bytes() {
        let cols = (0..12)
            .map(|i| Column {
                index: i,
                raw: i % 2 == 0,
                code: vec![254; WORDS],
                escaped: Vec::new(),
                target: vec![0; ROWS],
                cpu: if i % 2 == 0 { 0 } else { 2000 + i * 97 },
            })
            .collect::<Vec<_>>();
        let parts = partition(&cols);
        let got = parts
            .iter()
            .enumerate()
            .map(|(i, r)| cost(&cols[r.clone()], i == 0).slots)
            .max()
            .unwrap();
        let mut brute = usize::MAX;
        for a in 0..=cols.len() {
            for b in a..=cols.len() {
                brute = brute.min(
                    cost(&cols[..a], true)
                        .slots
                        .max(cost(&cols[a..b], false).slots)
                        .max(cost(&cols[b..], false).slots),
                );
            }
        }
        assert_eq!(got, brute);
        assert_eq!(parts[0].start, 0);
        assert_eq!(parts[TICKS - 1].end, cols.len());
        assert_eq!(cost(&[], true).slots, 21242);
        // Body202, prefix195, of which193 bytes are RAW data (headers excluded).
        assert_eq!(cost(&cols[..1], false).slots, 5500 + 193 * 15 + 11 * 63);
        assert_eq!(primitive_cycles(&[0; WORDS], 0), 3132);
        assert_eq!(primitive_cycles(&[254; WORDS], 0), 1632);
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
