//! Isolated span v4 experiment: official whole-frame LZ4HC12, then lossless
//! sequence segmentation across five PAL ticks with the decoded prefix in RAM.
//! Records use an explicit terminal token rather than LZ4's block end rules.
use super::*;
use std::collections::BTreeMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ops::Range;

const FRAME: usize = wide::FRAME_BYTES;
const TICKS: usize = 5;
const PREFETCH: usize = 1827;
const PREFETCH_STRIDE: usize = 9;
const TAIL_CHUNK: usize = 1024;
// Upper margins over isolated py65 fixtures with the decoder's short-run paths.
// Literal and match setup are charged separately, including token/helper work.
// Complete native playback remains authoritative for total timing.
fn literal_cycles(count: usize) -> usize {
    if count == 0 {
        0
    } else if count < 40 {
        600 + 19 * count
    } else {
        850 + 14 * count
    }
}
fn match_cycles(count: usize, offset: usize) -> usize {
    if offset == 1 {
        if count < 32 {
            750 + 14 * count
        } else {
            1100 + 7 * count
        }
    } else if count < 40 {
        750 + 20 * count
    } else {
        1050 + 14 * count
    }
}
fn length_wire(length: usize, base: usize) -> usize {
    if length < base {
        0
    } else {
        (length - base) / 255 + 1
    }
}
fn sequence_wire(literals: usize, matches: usize) -> usize {
    1 + length_wire(literals, 15)
        + literals
        + if matches > 0 {
            2 + length_wire(matches - 4, 15)
        } else {
            0
        }
}

#[cfg_attr(target_os = "linux", link(name = "dl"))]
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}
type BoundFn = unsafe extern "C" fn(c_int) -> c_int;
type CompressFn = unsafe extern "C" fn(*const c_void, *mut c_void, c_int, c_int, c_int) -> c_int;
type DecompressFn = unsafe extern "C" fn(*const c_void, *mut c_void, c_int, c_int) -> c_int;
type VersionFn = unsafe extern "C" fn() -> *const c_char;
struct Lz4 {
    handle: *mut c_void,
    bound: BoundFn,
    compress: CompressFn,
    decompress: DecompressFn,
    version: String,
}
impl Lz4 {
    fn open(explicit: Option<&Path>) -> Result<Self, String> {
        let candidates = if let Some(path) = explicit {
            vec![path.to_string_lossy().into_owned()]
        } else {
            vec![
                "/opt/homebrew/lib/liblz4.dylib".into(),
                "/usr/local/lib/liblz4.dylib".into(),
                "liblz4.dylib".into(),
                "liblz4.so.1".into(),
                "liblz4.so".into(),
            ]
        };
        for path in &candidates {
            let name = CString::new(path.as_str()).map_err(|_| "NUL in LZ4 library path")?;
            // RTLD_NOW; the handle remains live for every borrowed function pointer.
            let handle = unsafe { dlopen(name.as_ptr(), 2) };
            if handle.is_null() {
                continue;
            }
            let symbol = |name: &str| {
                let name = CString::new(name).unwrap();
                unsafe { dlsym(handle, name.as_ptr()) }
            };
            let (bound, compress, decompress, version) = (
                symbol("LZ4_compressBound"),
                symbol("LZ4_compress_HC"),
                symbol("LZ4_decompress_safe"),
                symbol("LZ4_versionString"),
            );
            if [bound, compress, decompress, version]
                .iter()
                .any(|p| p.is_null())
            {
                unsafe { dlclose(handle) };
                continue;
            }
            // POSIX dlsym exposes these exact signatures from the public LZ4 API.
            let version_fn: VersionFn = unsafe { std::mem::transmute(version) };
            let version = unsafe { CStr::from_ptr(version_fn()) }
                .to_string_lossy()
                .into_owned();
            return Ok(Self {
                handle,
                bound: unsafe { std::mem::transmute::<*mut c_void, BoundFn>(bound) },
                compress: unsafe { std::mem::transmute::<*mut c_void, CompressFn>(compress) },
                decompress: unsafe { std::mem::transmute::<*mut c_void, DecompressFn>(decompress) },
                version,
            });
        }
        Err(format!(
            "Cannot load official liblz4 HC API from{candidates:?}; use --lz4-library"
        ))
    }
    fn hc12(&self, input: &[u8]) -> Result<Vec<u8>, String> {
        let n = c_int::try_from(input.len()).map_err(|_| "LZ4 input exceeds int")?;
        let capacity = unsafe { (self.bound)(n) };
        if capacity <= 0 {
            return Err("LZ4_compressBound failed".into());
        }
        let mut compressed = vec![0; capacity as usize];
        let size = unsafe {
            (self.compress)(
                input.as_ptr().cast(),
                compressed.as_mut_ptr().cast(),
                n,
                capacity,
                12,
            )
        };
        if size <= 0 {
            return Err("LZ4_compress_HC(level12) failed".into());
        }
        compressed.truncate(size as usize);
        let mut decoded = vec![0; input.len()];
        let actual = unsafe {
            (self.decompress)(
                compressed.as_ptr().cast(),
                decoded.as_mut_ptr().cast(),
                size,
                n,
            )
        };
        if actual != n || decoded != input {
            return Err("Official LZ4 HC12 roundtrip mismatch".into());
        }
        Ok(compressed)
    }
}
impl Drop for Lz4 {
    fn drop(&mut self) {
        unsafe { dlclose(self.handle) };
    }
}

#[derive(Clone, Debug)]
enum Atom {
    Literal {
        start: usize,
        count: usize,
    },
    Match {
        start: usize,
        count: usize,
        offset: usize,
    },
}
impl Atom {
    fn start(&self) -> usize {
        match *self {
            Self::Literal { start, .. } | Self::Match { start, .. } => start,
        }
    }
    fn count(&self) -> usize {
        match *self {
            Self::Literal { count, .. } | Self::Match { count, .. } => count,
        }
    }
    fn literal(&self) -> bool {
        matches!(self, Self::Literal { .. })
    }
}
fn region_end(logical: usize) -> usize {
    if logical < 2048 {
        2048
    } else {
        FRAME
    }
}
fn address(logical: usize) -> usize {
    if logical < 2048 {
        0xe000 + logical
    } else {
        0x1800 + logical - 2048
    }
}
fn compact(ram: &[u8]) -> Vec<u8> {
    wide::REGIONS
        .iter()
        .flat_map(|&(_, n, a)| ram[a..a + n].iter().copied())
        .collect()
}

fn extension(input: &[u8], at: &mut usize, base: usize) -> Result<usize, String> {
    let mut length = base;
    if base == 15 {
        loop {
            let byte = *input.get(*at).ok_or("Truncated LZ4 length extension")?;
            *at += 1;
            length = length
                .checked_add(byte as usize)
                .ok_or("LZ4 length overflow")?;
            if byte != 255 {
                break;
            }
        }
    }
    Ok(length)
}
fn literal_atom(atoms: &mut Vec<Atom>, start: usize, count: usize) {
    if count == 0 {
        return;
    }
    if let Some(Atom::Literal {
        start: previous,
        count: n,
    }) = atoms.last_mut()
    {
        if *previous + *n == start && *n + count <= 255 && start + count <= region_end(*previous) {
            *n += count;
            return;
        }
    }
    atoms.push(Atom::Literal { start, count });
}
fn atoms_from_hc(block: &[u8], target: &[u8]) -> Result<Vec<Atom>, String> {
    let mut atoms = Vec::new();
    let (mut at, mut output) = (0, 0);
    while at < block.len() {
        let token = block[at];
        at += 1;
        let literals = extension(block, &mut at, (token >> 4) as usize)?;
        let bytes = block
            .get(at..at + literals)
            .ok_or("Truncated HC literals")?;
        if target.get(output..output + literals) != Some(bytes) {
            return Err("HC literal mismatch".into());
        }
        let mut left = literals;
        while left > 0 {
            let n = left.min(255).min(region_end(output) - output);
            literal_atom(&mut atoms, output, n);
            output += n;
            left -= n;
        }
        at += literals;
        if at == block.len() {
            break;
        }
        let pair = block.get(at..at + 2).ok_or("Truncated HC offset")?;
        let offset = usize::from(u16::from_le_bytes([pair[0], pair[1]]));
        at += 2;
        if offset == 0 || offset > output {
            return Err("Invalid HC offset".into());
        }
        let mut left = extension(block, &mut at, (token & 15) as usize)? + 4;
        if output + left > target.len() {
            return Err("HC output exceeds frame".into());
        }
        while left > 0 {
            let source = output - offset;
            let n = left
                .min(255)
                .min(region_end(output) - output)
                .min(region_end(source) - source);
            if n < 4 {
                literal_atom(&mut atoms, output, n);
            } else {
                atoms.push(Atom::Match {
                    start: output,
                    count: n,
                    offset,
                });
            }
            output += n;
            left -= n;
        }
    }
    if output != target.len() {
        return Err("HC decoded length mismatch".into());
    }
    Ok(atoms)
}
fn put_extension(body: &mut Vec<u8>, length: usize, base: usize) {
    if length < base {
        return;
    }
    let mut extra = length - base;
    while extra >= 255 {
        body.push(255);
        extra -= 255;
    }
    body.push(extra as u8);
}

/// The HC dictionary/matches remain authoritative. DP may absorb an entire HC
/// match into exact oracle literals, reducing setups without adding references.
/// State is the pending literal count0..255; a mapping boundary may close a
/// literal-only record. Long following literals invalidate overlong paths.
fn coalesce(atoms: &[Atom]) -> Result<(Vec<Atom>, usize), String> {
    #[derive(Clone, Copy)]
    struct Back {
        previous: u16,
        action: u8,
    }
    let invalid = Back {
        previous: 0,
        action: 255,
    };
    let mut history = Vec::<[Back; 256]>::with_capacity(atoms.len());
    let mut costs = [usize::MAX; 256];
    costs[0] = 0;
    for atom in atoms {
        let mut next = [usize::MAX; 256];
        let mut back = [invalid; 256];
        for (pending, &cost) in costs.iter().enumerate() {
            if cost == usize::MAX {
                continue;
            }
            let mut consider = |state: usize, value: usize, action: u8| {
                if value < next[state] {
                    next[state] = value;
                    back[state] = Back {
                        previous: pending as u16,
                        action,
                    };
                }
            };
            match *atom {
                Atom::Literal { start, count } => {
                    let pending_start = start - pending;
                    if pending + count <= 255 && start + count <= region_end(pending_start) {
                        consider(pending + count, cost, 0);
                    } else if pending > 0 && start == 2048 {
                        consider(
                            count,
                            cost + literal_cycles(pending) + 63 * sequence_wire(pending, 0),
                            3,
                        );
                    }
                }
                Atom::Match {
                    start,
                    count,
                    offset,
                } => {
                    consider(
                        0,
                        cost + literal_cycles(pending)
                            + match_cycles(count, offset)
                            + 63 * sequence_wire(pending, count),
                        1,
                    );
                    if pending + count <= 255 && start + count <= region_end(start - pending) {
                        consider(pending + count, cost, 2);
                    }
                }
            }
        }
        if next.iter().all(|&c| c == usize::MAX) {
            return Err("HC coalescer has no valid pending-literal path".into());
        }
        costs = next;
        history.push(back);
    }
    let mut final_cost = usize::MAX;
    let mut state = 0;
    for (pending, &cost) in costs.iter().enumerate() {
        if cost == usize::MAX {
            continue;
        }
        let extra = if pending > 0 {
            literal_cycles(pending) + 63 * sequence_wire(pending, 0)
        } else {
            0
        };
        if cost + extra < final_cost {
            final_cost = cost + extra;
            state = pending;
        }
    }
    let mut actions = vec![0; atoms.len()];
    for i in (0..atoms.len()).rev() {
        let back = history[i][state];
        if back.action == 255 {
            return Err("Broken coalescer traceback".into());
        }
        actions[i] = back.action;
        state = back.previous as usize;
    }
    let mut result = Vec::new();
    let mut converted = 0;
    for (atom, action) in atoms.iter().zip(actions) {
        match *atom {
            Atom::Literal { start, count } => literal_atom(&mut result, start, count),
            Atom::Match { start, count, .. } if action == 2 => {
                literal_atom(&mut result, start, count);
                converted += count;
            }
            Atom::Match { .. } => result.push(atom.clone()),
        }
    }
    if result
        .windows(2)
        .any(|a| a[0].literal() && a[1].literal() && a[1].start() != 2048)
    {
        return Err("Coalescer introduced an unsupported literal-only record boundary".into());
    }
    Ok((result, converted))
}
#[derive(Clone, Default, Debug)]
struct Measure {
    body: Vec<u8>,
    sequences: usize,
    literals: usize,
    matches: usize,
    fills: usize,
    start: usize,
    end: usize,
    run_cycles: usize,
}
impl Measure {
    fn prefix(&self) -> usize {
        self.body.len().min(PREFETCH) / PREFETCH_STRIDE * PREFETCH_STRIDE
    }
    fn chunks(&self) -> usize {
        usize::from(self.prefix() > 0) + (self.body.len() - self.prefix()).div_ceil(TAIL_CHUNK)
    }
    fn cpu(&self) -> usize {
        323 + self.run_cycles + 500 * self.chunks()
    }
    fn slots(&self) -> usize {
        self.cpu() + 63 * (self.body.len() + 3 - self.prefix())
    }
    fn scheduling_slots(&self) -> usize {
        // Actual rounded prefix/chunk costs have downward steps at multiples9.
        // Binary partitioning instead uses this monotone upper envelope: at
        // most8 prefix bytes remain in the blank, plus its possible tail chunk.
        let continuous_prefix = self.body.len().min(PREFETCH);
        let tail_upper = self.body.len() - continuous_prefix + PREFETCH_STRIDE - 1;
        let chunks_upper =
            usize::from(self.body.len() >= PREFETCH_STRIDE) + tail_upper.div_ceil(TAIL_CHUNK);
        self.cpu() - 500 * self.chunks() + 500 * chunks_upper + 63 * (tail_upper + 3)
    }
}
fn body_for(atoms: &[Atom], target: &[u8]) -> Result<Measure, String> {
    let mut m = Measure::default();
    m.start = atoms.first().map_or(0, Atom::start);
    m.end = atoms.last().map_or(m.start, |a| a.start() + a.count());
    let mut i = 0;
    while i < atoms.len() {
        let (literal_start, literals) = if let Atom::Literal { start, count } = atoms[i] {
            i += 1;
            (start, count)
        } else {
            (0, 0)
        };
        let next = atoms.get(i);
        let (matches, offset) = if let Some(Atom::Match { count, offset, .. }) = next {
            i += 1;
            (*count, *offset)
        } else {
            if i != atoms.len() {
                return Err("Adjacent literal runs require record boundary".into());
            }
            (0, 0)
        };
        m.body
            .push(((literals.min(15) as u8) << 4) | matches.saturating_sub(4).min(15) as u8);
        put_extension(&mut m.body, literals, 15);
        m.body
            .extend_from_slice(&target[literal_start..literal_start + literals]);
        if matches > 0 {
            m.body.extend_from_slice(&(offset as u16).to_le_bytes());
            put_extension(&mut m.body, matches - 4, 15);
            m.matches += matches;
            m.run_cycles += match_cycles(matches, offset);
            if offset == 1 {
                m.fills += matches;
            }
        }
        m.literals += literals;
        m.run_cycles += literal_cycles(literals);
        m.sequences += 1;
    }
    m.body.push(0);
    Ok(m)
}

fn partition(atoms: &[Atom], target: &[u8], ticks: usize) -> Result<Vec<Range<usize>>, String> {
    let barriers = (1..atoms.len())
        .filter(|&i| atoms[i - 1].literal() && atoms[i].literal())
        .collect::<Vec<_>>();
    if barriers.len() + 1 > ticks {
        return Err("HC literal boundaries require more display ticks".into());
    }
    let greedy = |cap: usize| -> Result<Vec<Range<usize>>, String> {
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < atoms.len() {
            let boundary = barriers
                .iter()
                .copied()
                .find(|&b| b > start)
                .unwrap_or(atoms.len());
            let mut low = start;
            let mut high = boundary;
            while low < high {
                let mid = (low + high).div_ceil(2);
                if body_for(&atoms[start..mid], target)?.scheduling_slots() <= cap {
                    low = mid;
                } else {
                    high = mid - 1;
                }
            }
            if low == start {
                return Ok(vec![0..0; ticks + 1]);
            }
            ranges.push(start..low);
            start = low;
            if ranges.len() > ticks {
                break;
            }
        }
        Ok(ranges)
    };
    let mut low = 0;
    let mut high = atoms
        .iter()
        .map(|a| body_for(std::slice::from_ref(a), target).map(|m| m.scheduling_slots()))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .sum::<usize>();
    while low < high {
        let mid = (low + high) / 2;
        if greedy(mid)?.len() <= ticks {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    let mut result = greedy(low)?;
    if result.last().map(|r| r.end) != Some(atoms.len()) {
        return Err("LZ4 partition does not cover frame".into());
    }
    result.resize(ticks, atoms.len()..atoms.len());
    Ok(result)
}

/// Transactional reference decoder: malformed late data does not alter front RAM.
fn decode(ram: &mut [u8], cursor: &mut usize, payload: &[u8]) -> Result<bool, String> {
    if ram.len() != 65536 || payload.len() < 2 {
        return Err("Invalid v4 RAM or payload".into());
    }
    let flags = payload[0];
    if flags & !3 != 0 {
        return Err("Unknown v4 flags".into());
    }
    let mut next_cursor = if flags & 2 != 0 { 0 } else { *cursor };
    let mut next = ram.to_vec();
    let mut at = 1;
    loop {
        let token = *payload.get(at).ok_or("Missing v4 terminal")?;
        at += 1;
        if at == payload.len() {
            if token != 0 {
                return Err("Invalid v4 terminal".into());
            }
            break;
        }
        let n = extension(payload, &mut at, (token >> 4) as usize)?;
        if n > 255
            || next_cursor + n > FRAME
            || (n > 0 && next_cursor + n > region_end(next_cursor))
        {
            return Err("Invalid v4 literal run bounds".into());
        }
        let bytes = payload.get(at..at + n).ok_or("Truncated v4 literals")?;
        at += n;
        for &v in bytes {
            next[address(next_cursor)] = v;
            next_cursor += 1;
        }
        if at + 1 == payload.len() {
            if payload[at] != 0 {
                return Err("Invalid literal-only terminal".into());
            }
            break;
        }
        let off = payload.get(at..at + 2).ok_or("Truncated v4 offset")?;
        at += 2;
        let offset = usize::from(u16::from_le_bytes([off[0], off[1]]));
        let n = extension(payload, &mut at, (token & 15) as usize)? + 4;
        if offset == 0 || offset > next_cursor || n > 255 || next_cursor + n > FRAME {
            return Err("Invalid v4 match bounds or offset".into());
        }
        let src = next_cursor - offset;
        if next_cursor + n > region_end(next_cursor) || src + n > region_end(src) {
            return Err("v4 match crosses physical region".into());
        }
        for _ in 0..n {
            let value = next[address(next_cursor - offset)];
            next[address(next_cursor)] = value;
            next_cursor += 1;
        }
    }
    if flags & 1 != 0 && next_cursor != FRAME {
        return Err("Incomplete v4 completion".into());
    }
    ram.copy_from_slice(&next);
    *cursor = next_cursor;
    Ok(flags & 1 != 0)
}

struct Record {
    index: usize,
    offset: usize,
    source: usize,
    prepared: Option<usize>,
    reset: bool,
    complete: bool,
    payload: usize,
    m: Measure,
}
impl Record {
    fn json(&self, total: usize) -> String {
        format!("{{\"index\":{},\"from_phase\":{},\"to_phase\":{},\"offset\":{},\"payload_offset\":{},\"payload_bytes\":{},\"record_bytes\":{},\"source_content\":{},\"prepared_content\":{},\"swap\":{},\"completion\":{},\"reset\":{},\"logical_output_start\":{},\"logical_output_end\":{},\"written_bytes\":{},\"literal_bytes\":{},\"match_bytes\":{},\"offset1_bytes\":{},\"command_count\":{},\"sequence_count\":{},\"runs\":{},\"chunk_count\":{},\"max_chunk_bytes\":{},\"prefetched_body_bytes\":{},\"wire_dma_bytes\":{},\"blank_dma_bytes\":{},\"estimated_decoder_cycles\":{},\"estimated_tick_cpu_slots\":{}}}",self.index,self.index,(self.index+1).min(total-1),self.offset,self.offset+2,self.payload,self.payload+2,self.source,self.prepared.map_or_else(||"null".into(),|v|v.to_string()),self.complete,self.complete,self.reset,self.m.start,self.m.end,self.m.literals+self.m.matches,self.m.literals,self.m.matches,self.m.fills,self.m.sequences,self.m.sequences,self.m.sequences,self.m.chunks(),self.m.prefix().max((self.m.body.len()-self.m.prefix()).min(TAIL_CHUNK)),self.m.prefix(),self.payload+2,self.payload+2-self.m.prefix(),self.m.cpu(),self.m.slots())
    }
}
struct Assets {
    stream: Vec<u8>,
    oracles: Vec<u8>,
    records: Vec<Record>,
    hc_sizes: Vec<usize>,
    coalesced_bytes: usize,
}
fn encode(frames: &[u8], lz4: &Lz4, optimise: bool) -> Result<Assets, String> {
    if frames.is_empty() || !frames.len().is_multiple_of(FRAME) {
        return Err("Invalid v4 logical frames".into());
    }
    let count = frames.len() / FRAME;
    let mut ram = vec![0xaa; 65536];
    for &(at, n, a) in &wide::REGIONS {
        ram[a..a + n].copy_from_slice(&frames[at..at + n]);
    }
    let mut cursor = FRAME;
    let mut assets = Assets {
        stream: Vec::new(),
        oracles: Vec::with_capacity(count * TICKS * FRAME),
        records: Vec::new(),
        hc_sizes: Vec::new(),
        coalesced_bytes: 0,
    };
    for content in 0..count {
        let target = if content + 1 < count {
            &frames[(content + 1) * FRAME..(content + 2) * FRAME]
        } else {
            &frames[content * FRAME..(content + 1) * FRAME]
        };
        let atoms = if content + 1 < count {
            let hc = lz4.hc12(target)?;
            assets.hc_sizes.push(hc.len());
            let atoms = atoms_from_hc(&hc, target)?;
            if optimise {
                let (planned, converted) = coalesce(&atoms)?;
                assets.coalesced_bytes += converted;
                planned
            } else {
                atoms
            }
        } else {
            Vec::new()
        };
        let ranges = if atoms.is_empty() {
            vec![0..0; TICKS]
        } else {
            partition(&atoms, target, TICKS)?
        };
        for (tick, range) in ranges.into_iter().enumerate() {
            assets.oracles.extend(compact(&ram));
            let mut m = body_for(&atoms[range], target)?;
            if m.literals + m.matches == 0 {
                m.start = cursor;
                m.end = cursor;
            }
            let reset = content + 1 < count && tick == 0;
            let complete = content + 1 < count && tick + 1 == TICKS;
            let mut payload = vec![u8::from(reset) * 2 + u8::from(complete)];
            payload.extend_from_slice(&m.body);
            if payload.len() > u16::MAX as usize {
                return Err("v4 packet exceeds u16".into());
            }
            if assets.stream.len() % 65536 + payload.len() + 2 > 65534 {
                assets
                    .stream
                    .resize((assets.stream.len() / 65536 + 1) * 65536, 0);
            }
            if decode(&mut ram, &mut cursor, &payload)? != complete {
                return Err("v4 reference completion mismatch".into());
            }
            if tick + 1 == TICKS && compact(&ram) != target {
                return Err(format!("v4 front mismatch atcontent{content}"));
            }
            let record = Record {
                index: assets.records.len(),
                offset: assets.stream.len(),
                source: content,
                prepared: (content + 1 < count).then_some(content + 1),
                reset,
                complete,
                payload: payload.len(),
                m,
            };
            assets
                .stream
                .extend_from_slice(&(payload.len() as u16).to_le_bytes());
            assets.stream.extend(payload);
            assets.records.push(record);
        }
        if content % 40 == 0 {
            println!("LZ4HC12 repacked content{content}/{count}");
        }
    }
    assets
        .stream
        .resize(assets.stream.len().div_ceil(65536) * 65536, 0);
    if ram[0xd000..0xe000].iter().any(|&v| v != 0xaa) {
        return Err("v4 reference altered I/O gap".into());
    }
    Ok(assets)
}

/// Preserve source metadata verbatim without adding a JSON dependency. This
/// scanner extracts complete top-level values, respecting strings/nesting.
pub(super) fn json_fields(text: &str) -> Result<BTreeMap<String, String>, String> {
    let bytes = text.as_bytes();
    let mut at = 0;
    let skip = |at: &mut usize| {
        while bytes.get(*at).is_some_and(u8::is_ascii_whitespace) {
            *at += 1;
        }
    };
    skip(&mut at);
    if bytes.get(at) != Some(&b'{') {
        return Err("Source manifest is not JSON object".into());
    }
    at += 1;
    let mut fields = BTreeMap::new();
    loop {
        skip(&mut at);
        if bytes.get(at) == Some(&b'}') {
            at += 1;
            break;
        }
        if bytes.get(at) != Some(&b'"') {
            return Err("Invalid manifest key".into());
        }
        at += 1;
        let key_start = at;
        while bytes.get(at).is_some_and(|b| *b != b'"' && *b != b'\\') {
            at += 1;
        }
        if bytes.get(at) != Some(&b'"') {
            return Err("Escaped manifest key unsupported".into());
        }
        let key = text[key_start..at].to_string();
        at += 1;
        skip(&mut at);
        if bytes.get(at) != Some(&b':') {
            return Err("Missing manifest colon".into());
        }
        at += 1;
        skip(&mut at);
        let start = at;
        let (mut depth, mut string, mut escape) = (0usize, false, false);
        while let Some(&b) = bytes.get(at) {
            if string {
                if escape {
                    escape = false;
                } else if b == b'\\' {
                    escape = true;
                } else if b == b'"' {
                    string = false;
                }
            } else {
                match b {
                    b'"' => string = true,
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' if depth > 0 => depth -= 1,
                    b',' | b'}' if depth == 0 => break,
                    _ => {}
                }
            }
            at += 1;
        }
        if string || depth > 0 || at == start {
            return Err("Invalid manifest value".into());
        }
        if fields
            .insert(key, text[start..at].trim().to_string())
            .is_some()
        {
            return Err("Duplicate source manifest key".into());
        }
        skip(&mut at);
        match bytes.get(at) {
            Some(b',') => at += 1,
            Some(b'}') => {}
            _ => return Err("Invalid manifest separator".into()),
        }
    }
    skip(&mut at);
    if at != bytes.len() {
        return Err("Trailing source manifest data".into());
    }
    Ok(fields)
}
pub(super) fn scalar<T: std::str::FromStr>(
    fields: &BTreeMap<String, String>,
    key: &str,
) -> Result<T, String> {
    fields
        .get(key)
        .ok_or_else(|| format!("Missing source{key}"))?
        .parse()
        .map_err(|_| format!("Invalid source{key}"))
}
pub(super) fn link_or_write(
    source: &Path,
    target: &Path,
    data: &[u8],
    whole: bool,
) -> Result<(), String> {
    if !whole || fs::hard_link(source, target).is_err() {
        write(target, data)?;
    }
    Ok(())
}
pub fn run(o: &Options) -> Result<(), String> {
    let source =
        fs::canonicalize(o.wide_lz4_source.as_ref().unwrap()).map_err(|e| e.to_string())?;
    let mut fields =
        json_fields(&fs::read_to_string(source.join("manifest.json")).map_err(|e| e.to_string())?)?;
    if scalar::<usize>(&fields, "width")? != 384
        || scalar::<usize>(&fields, "height")? != 256
        || scalar::<usize>(&fields, "frame_ticks")? != TICKS
        || scalar::<usize>(&fields, "active_height")? != 203
        || scalar::<usize>(&fields, "active_top")? != 26
        || scalar::<usize>(&fields, "active_left")? != 0
        || scalar::<usize>(&fields, "active_width")? != 384
        || (scalar::<f64>(&fields, "source_fps")? / (pal_tick_hz() / TICKS as f64) - 1.).abs()
            > 1e-6
    {
        return Err("v4 reuse currently requires384x256/active384x203 top26, PAL/5 source".into());
    }
    let frame_path = source.join("oracle.frames");
    let mut frames = fs::read(&frame_path).map_err(|e| e.to_string())?;
    let all = scalar::<usize>(&fields, "source_frames")?;
    if frames.len() != all * FRAME {
        return Err("Source oracle.frames size mismatch".into());
    }
    let count = o.source_frame_limit.unwrap_or(all);
    if count == 0 || count > all {
        return Err("Source frame limit exceeds available frames".into());
    }
    frames.truncate(count * FRAME);
    if fs::read(source.join("initial.bin")).map_err(|e| e.to_string())? != frames[..FRAME] {
        return Err("Source initial/oracle mismatch".into());
    }
    let audio_hz = scalar::<u32>(&fields, "audio_sample_rate")?;
    let mut audio = fs::read(source.join("audio.pcm")).map_err(|e| e.to_string())?;
    if !audio.len().is_multiple_of(4) {
        return Err("Source PCM alignment".into());
    }
    let duration = count as f64 * TICKS as f64 / pal_tick_hz();
    if count != all {
        audio.truncate((duration * audio_hz as f64).round() as usize * 4);
    }
    let lib = Lz4::open(o.lz4_library.as_deref())?;
    println!(
        "Reusing{count}/{all} exact frames, official LZ4{} HC12; five ticks, prefetch1827B",
        lib.version
    );
    let assets = encode(&frames, &lib, o.lz4_coalesce)?;
    fs::create_dir(&o.output)
        .map_err(|e| format!("Create new output directory{}: {e}", o.output.display()))?;
    link_or_write(
        &source.join("initial.bin"),
        &o.output.join("initial.bin"),
        &frames[..FRAME],
        true,
    )?;
    link_or_write(
        &frame_path,
        &o.output.join("oracle.frames"),
        &frames,
        count == all,
    )?;
    link_or_write(
        &source.join("audio.pcm"),
        &o.output.join("audio.pcm"),
        &audio,
        count == all,
    )?;
    write(&o.output.join("oracle.phases"), &assets.oracles)?;
    write(&o.output.join("stream.bin"), &assets.stream)?;
    let max_slots = assets.records.iter().map(|r| r.m.slots()).max().unwrap();
    let max_packet = assets.records.iter().map(|r| r.payload).max().unwrap();
    let total_wire = assets.records.iter().map(|r| r.payload + 2).sum::<usize>();
    let records = assets.records.len();
    fields.remove("records");
    let updates=vec![
        ("format",quoted("MUpic progressive wide LZ4 v4")),("span_version","4".into()),
        ("source_asset_directory",quoted(&source.to_string_lossy())),("source_conversion_frames",all.to_string()),
        ("source_frames",count.to_string()),("logical_oracle_frames",count.to_string()),("phases",records.to_string()),("display_ticks",records.to_string()),
        ("duration_seconds",duration.to_string()),("stream_bytes",assets.stream.len().to_string()),("stream_blocks",(assets.stream.len()/65536).to_string()),
        ("record_bytes_total",total_wire.to_string()),("video_record_bytes_per_second",(total_wire as f64/duration).to_string()),
        ("max_packet_bytes",max_packet.to_string()),("max_tick_payload_bytes",max_packet.to_string()),
        ("max_chunk_bytes",assets.records.iter().map(|r|r.m.prefix().max((r.m.body.len()-r.m.prefix()).min(TAIL_CHUNK))).max().unwrap().to_string()),
        ("buffered_chunk_capacity",TAIL_CHUNK.to_string()),("chunk_count_total",assets.records.iter().map(|r|r.m.chunks()).sum::<usize>().to_string()),
        ("completion_count",(count-1).to_string()),("estimated_tick_cpu_slots_max",max_slots.to_string()),
        ("tick_budget_cpu_slots",o.tick_budget.to_string()),("fits_tick_budget",(max_slots<=o.tick_budget).to_string()),
        ("max_estimated_decoder_cycles",assets.records.iter().map(|r|r.m.cpu()).max().unwrap().to_string()),
        ("max_written_bytes",assets.records.iter().map(|r|r.m.literals+r.m.matches).max().unwrap().to_string()),
        ("phase_order",quoted("Full next-frame HC12 decoded progressively across five PAL ticks; first resets logical dst, fifth completes, EOF holds last image")),
        ("progressive_wire_format",quoted("Outer LE16 payload length; flags bit1 reset logical destination, bit0 complete. Body LZ4 token, literal length extensions, literals, nonzero offsetLE16, match extensions. Every body ends terminal token0; a sole remaining0 after literals ends literal-only. Runs<=255, matches>=4, physical2048 boundary split for destination/source. Forward overlapping matches allowed; global decoded frame prefix persists between records.")),
        ("progressive_tick_cost_model",quoted("323/record plus separately charged runs: literal<40:600+19*n, >=40:850+14*n; general match<40:750+20*n, >=40:1050+14*n; offset1<32:750+14*n, >=32:1100+7*n;500/DMA chunk. DMA63*(outer2+flags1+body minus prefetched body bytes). Prefix floor(min(body,1827)/9)*9; chunks=(prefix>0)+ceil((body-prefix)/1024). Margins over isolated py65 short-path fixtures, complete native timing authoritative; row prefetch work outside blank excluded.")),
        ("lz4_compressor",quoted("Official liblz4 LZ4_compress_HC")),("lz4_library_version",quoted(&lib.version)),("lz4_compression_level","12".into()),
        ("lz4_official_frame_bytes_total",assets.hc_sizes.iter().sum::<usize>().to_string()),("lz4_official_frame_bytes_max",assets.hc_sizes.iter().max().unwrap_or(&0).to_string()),
        ("lz4_segmented_token_format","true".into()),("row_prefetch_max_bytes",PREFETCH.to_string()),("row_prefetch_bytes_per_line",PREFETCH_STRIDE.to_string()),
        ("lz4_cost_coalescing",o.lz4_coalesce.to_string()),("lz4_matches_absorbed_as_literals_bytes",assets.coalesced_bytes.to_string()),
        ("lz4_repack_planner",quoted(if o.lz4_coalesce {"DP over original HC12 atoms and pending literal count0..255; retain match or absorb its exact target bytes as literals; minimise measured-run CPU model+63*wire, no new backrefs or colour changes"} else {"Original HC12 matches retained; split only255byte/physical2048 bounds"})),
        ("partition_cost_envelope",quoted("Monotone continuous-prefix upper bound: reserve up to8 rounded prefix bytes and one possible tail DMA chunk; actual per-record statistics retain floor9 prefetch")),
        ("audio_sample_rate",audio_hz.to_string()),("audio_samples",(audio.len()/4).to_string()),("audio_bytes",audio.len().to_string()),
        ("audio_gain_measurements_scope",quoted("Original source conversion; PCM reused unchanged, probe takes its prefix")),
        ("source_conversion_metrics_scope",quoted("Colour/quality/author-planning metrics inherited from the original full source conversion; pixels are reused unchanged")),
        ("source_rgb_reference",quoted(&source.join("source.rgb").to_string_lossy())),("mixed_rgb_reference",quoted(&source.join("mixed.rgb").to_string_lossy())),
        ("native_cache_ticks","20".into()),("reference_roundtrip_verified","true".into()),
        ("records",format!("[\n{}\n]",assets.records.iter().map(|r|r.json(records)).collect::<Vec<_>>().join(",\n"))),
    ];
    for (key, value) in updates {
        fields.insert(key.into(), value);
    }
    write(
        &o.output.join("manifest.json"),
        format!(
            "{{\n{}\n}}\n",
            fields
                .iter()
                .map(|(k, v)| format!("  {}:{v}", quoted(k)))
                .collect::<Vec<_>>()
                .join(",\n")
        ),
    )?;
    println!("v4{count}frames/{records}ticks: maxpacket{max_packet}, modelmax{max_slots}/{}; wire{total_wire} bytes",o.tick_budget);
    if o.native_mux {
        let mux = mux_fixed_tick_blocks(
            &assets.stream,
            &audio,
            records,
            o.audio_divider,
            o.audio_offset,
            20,
        )?;
        write(&o.output.join("stream.mup"), &mux.file)?;
        write(
            &o.output.join("cache-mux.json"),
            mux.metadata(o.audio_offset, o.audio_divider, audio_hz),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_only_terminal_zero_and_zero_token_match_are_unambiguous() {
        let mut ram = vec![0xaa; 65536];
        let mut cursor = 0;
        decode(&mut ram, &mut cursor, &[2, 0x10, 0x7b, 0]).unwrap();
        assert_eq!(cursor, 1);
        decode(&mut ram, &mut cursor, &[0, 0, 1, 0, 0]).unwrap();
        assert_eq!(cursor, 5);
        assert!(compact(&ram)[..5].iter().all(|&b| b == 0x7b));
        decode(&mut ram, &mut cursor, &[0, 0]).unwrap();
        assert_eq!(cursor, 5);
    }
    #[test]
    fn malformed_records_are_transactional_and_reject_cross_region_runs() {
        for payload in [
            vec![4, 0],
            vec![0, 0x10, 9],
            vec![0, 0, 0, 0, 0],
            vec![1, 0],
            vec![0, 0x10, 9, 1],
            vec![2, 0x1f, 9, 1, 0, 255],
        ] {
            let mut ram = vec![0xaa; 65536];
            let mut cursor = 1;
            assert!(
                decode(&mut ram, &mut cursor, &payload).is_err(),
                "{payload:?}"
            );
            assert!(ram.iter().all(|&b| b == 0xaa));
            assert_eq!(cursor, 1);
        }
        let mut ram = vec![0xaa; 65536];
        let mut cursor = 2047;
        assert!(decode(&mut ram, &mut cursor, &[0, 0x20, 1, 2, 0]).is_err());
        cursor = 2048;
        assert!(decode(&mut ram, &mut cursor, &[0, 0, 1, 0, 0]).is_err());
    }
    #[test]
    fn hc12_repacking_preserves_overlap_boundary_partials_and_eof() {
        let lib = Lz4::open(None).unwrap();
        let mut frames = vec![0; FRAME * 3];
        for i in 0..FRAME {
            frames[FRAME + i] = ((i / 71) % 11) as u8;
            frames[2 * FRAME + i] = ((i * 43 + i / 257) % 251) as u8;
        }
        let assets = encode(&frames, &lib, false).unwrap();
        assert_eq!(assets.records.len(), 15);
        assert_eq!(assets.records.iter().filter(|r| r.complete).count(), 2);
        assert!(assets.records[10..]
            .iter()
            .all(|r| r.payload == 2 && r.m.literals + r.m.matches == 0));
        let mut ram = vec![0xaa; 65536];
        for &(at, n, a) in &wide::REGIONS {
            ram[a..a + n].copy_from_slice(&frames[at..at + n]);
        }
        let mut cursor = FRAME;
        for record in &assets.records {
            assert_eq!(
                compact(&ram),
                assets.oracles[record.index * FRAME..(record.index + 1) * FRAME]
            );
            decode(
                &mut ram,
                &mut cursor,
                &assets.stream[record.offset + 2..record.offset + 2 + record.payload],
            )
            .unwrap();
        }
        assert_eq!(compact(&ram), frames[2 * FRAME..]);
        assert!(ram[0xd000..0xe000].iter().all(|&b| b == 0xaa));
    }
    #[test]
    fn length_splitting_fallback_literals_and_balanced_ab_boundaries() {
        let target = vec![9; FRAME];
        // One literal byte and a long offset1 run that crosses both sides of2048.
        let mut block = vec![0x1f, 9, 1, 0];
        put_extension(&mut block, FRAME - 1 - 4, 15);
        let atoms = atoms_from_hc(&block, &target).unwrap();
        for atom in &atoms {
            assert!(atom.count() <= 255);
            assert!(atom.start() + atom.count() <= region_end(atom.start()));
            if let Atom::Match {
                start,
                count,
                offset,
            } = *atom
            {
                assert!(count >= 4);
                assert!(start - offset + count <= region_end(start - offset));
            }
        }
        let ranges = partition(&atoms, &target, 5).unwrap();
        assert_eq!(ranges.len(), 5);
        let mut ram = vec![0xaa; 65536];
        let mut cursor = 0;
        for (i, r) in ranges.into_iter().enumerate() {
            let m = body_for(&atoms[r], &target).unwrap();
            let mut p = vec![u8::from(i == 0) * 2 + u8::from(i == 4)];
            p.extend(m.body);
            decode(&mut ram, &mut cursor, &p).unwrap();
        }
        assert_eq!(compact(&ram), target);
    }
    #[test]
    fn metadata_values_preserve_nested_arrays_strings_and_escaped_quotes() {
        let f =
            json_fields("{\"s\":\"comma, braces} and \\\"quote\\\"\",\"a\":[{\"x\":3},2],\"n\":5}")
                .unwrap();
        assert_eq!(f["a"], "[{\"x\":3},2]");
        assert_eq!(scalar::<usize>(&f, "n").unwrap(), 5);
        assert!(json_fields("{\"n\":1,\"n\":2}").is_err());
    }
    #[test]
    fn partition_envelope_is_monotone_and_metadata_eof_holds_last_phase() {
        let target = vec![9; FRAME];
        let atoms = [
            Atom::Literal { start: 0, count: 1 },
            Atom::Match {
                start: 1,
                count: 4,
                offset: 1,
            },
            Atom::Literal { start: 5, count: 3 },
            Atom::Match {
                start: 8,
                count: 4,
                offset: 1,
            },
            Atom::Literal {
                start: 12,
                count: 8,
            },
        ];
        let mut previous = 0;
        for n in 1..=atoms.len() {
            let m = body_for(&atoms[..n], &target).unwrap();
            assert!(m.scheduling_slots() >= m.slots());
            assert!(m.scheduling_slots() >= previous);
            previous = m.scheduling_slots();
        }
        let r = Record {
            index: 14,
            offset: 0,
            source: 2,
            prepared: None,
            reset: false,
            complete: false,
            payload: 2,
            m: Measure::default(),
        };
        assert_eq!(json_fields(&r.json(15)).unwrap()["to_phase"], "14");
    }
    #[test]
    fn cost_coalescing_reduces_short_match_setups_and_replays_exact_target() {
        let lib = Lz4::open(None).unwrap();
        let mut frames = vec![0; FRAME * 2];
        for i in 0..FRAME {
            frames[FRAME + i] = ((i * 43 + i / 257) % 251) as u8;
        }
        let raw = lib.hc12(&frames[FRAME..]).unwrap();
        let original = atoms_from_hc(&raw, &frames[FRAME..]).unwrap();
        let (planned, converted) = coalesce(&original).unwrap();
        assert!(converted > 0);
        let original_matches = original.iter().filter(|a| !a.literal()).count();
        assert!(planned.iter().filter(|a| !a.literal()).count() < original_matches);
        let assets = encode(&frames, &lib, true).unwrap();
        assert!(assets.coalesced_bytes > 0);
        assert_eq!(assets.records.len(), 10);
        assert!(assets.records[5..].iter().all(|r| r.payload == 2));
    }
    #[test]
    fn lz4_cli_reuse_requires_pal5_but_does_not_need_video_input() {
        let args = |extra: &[&str]| {
            ["--wide-lz4-source", "sample", "--output", "new"]
                .into_iter()
                .chain(extra.iter().copied())
                .map(str::to_string)
                .collect::<Vec<_>>()
                .into_iter()
        };
        let o = parse_options_args(args(&[])).unwrap().unwrap();
        assert_eq!(o.frame_ticks, Some(5));
        assert_eq!(o.audio_offset, 131072);
        assert!(parse_options_args(args(&["--frame-ticks", "8"])).is_err());
        assert!(parse_options_args(args(&["--fps", "25"])).is_err());
        assert!(parse_options_args(args(&["--source-frame-limit", "0"])).is_err());
        let o = parse_options_args(args(&["--lz4-coalesce", "--audio-offset", "196608"]))
            .unwrap()
            .unwrap();
        assert!(o.lz4_coalesce);
        assert_eq!(o.audio_offset, 196608);
    }
}
