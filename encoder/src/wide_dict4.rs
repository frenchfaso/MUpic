//! Isolated v8: exact four-byte vertical dictionary words, unchanged-word SKIP,
//! escaped words, and direct RAW192 fallback. Reuses genuine PAL/3 source RGB.
use super::wide_lz4::{json_fields, link_or_write, scalar};
use super::*;
#[path = "dict4_streaming.rs"]
mod streaming;
const FRAME: usize = 46080;
const COLS: usize = 180;
const TOP: usize = 5;
const ROWS: usize = 192;
const TICKS: usize = 3;
const REGIONS: [(usize, usize, usize); 2] = [(0, 2048, 0xe000), (2048, 44032, 0x1800)];
#[derive(Clone, Copy, Debug)]
struct Codec {
    version: usize,
    top: usize,
    active: usize,
    rows: usize,
}
impl Codec {
    const LEGACY: Self = Self {
        version: 8,
        top: TOP,
        active: 190,
        rows: ROWS,
    };
    const FULL_HEIGHT: Self = Self {
        version: 9,
        top: 0,
        active: 200,
        rows: 200,
    };
    fn words(self) -> usize {
        self.rows / 4
    }
}
const LUT_BYTES: usize = 1024;
const COL_SETUP: usize = 500;
const RECORD_SETUP: usize = 5000;
#[cfg(test)]
fn address(c: usize) -> usize {
    address_codec(c, Codec::LEGACY)
}
fn address_codec(c: usize, codec: Codec) -> usize {
    if c < 8 {
        0xe000 + c * 256 + codec.top
    } else {
        0x1800 + (c - 8) * 256 + codec.top
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
    fn writes(&self) -> usize {
        if self.raw {
            self.target.len()
        } else {
            4 * self.code.iter().filter(|&&v| v != 254).count()
        }
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
    words: usize,
}
#[cfg(test)]
fn plan(before: &[u8], after: &[u8]) -> Plan {
    plan_codec(before, after, Codec::LEGACY)
}
fn plan_codec(before: &[u8], after: &[u8], codec: Codec) -> Plan {
    let mut freq: HashMap<[u8; 4], (usize, usize)> = HashMap::new();
    for c in 0..COLS {
        for y in (codec.top..codec.top + codec.rows).step_by(4) {
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
        let old = &before[c * 256 + codec.top..c * 256 + codec.top + codec.rows];
        let target = &after[c * 256 + codec.top..c * 256 + codec.top + codec.rows];
        if old == target {
            continue;
        }
        let mut code = Vec::with_capacity(codec.words());
        let mut escaped = Vec::new();
        for at in (0..codec.rows).step_by(4) {
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
        let raw = 63 * (2 + codec.words() + escaped.len()) + cpu + COL_SETUP
            >= 63 * (2 + codec.rows) + COL_SETUP;
        columns.push(Column {
            index: c,
            raw,
            code,
            escaped,
            target: target.to_vec(),
            cpu: if raw { 0 } else { cpu },
        });
    }
    Plan {
        lut,
        columns,
        words: entries.len(),
    }
}
#[derive(Clone, Debug)]
struct Cost {
    body: usize,
    prefix: usize,
    raw_prefix: usize,
    cpu: usize,
    slots: usize,
}
#[cfg(test)]
fn cost_with_prefetch(columns: &[Column], reset: bool, prefetch: usize) -> Cost {
    cost_with_codec(columns, reset, prefetch, Codec::LEGACY)
}
fn cost_with_codec(columns: &[Column], reset: bool, prefetch: usize, codec: Codec) -> Cost {
    let mut body = if reset { LUT_BYTES } else { 0 };
    for c in columns {
        body += c.wire();
    }
    let prefix = (body.min(prefetch * codec.active) / prefetch) * prefetch;
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
        prefix,
        raw_prefix,
        cpu,
        slots: cpu + 63 * (body + 4 - prefix),
    }
}
#[cfg(test)]
fn cost(columns: &[Column], reset: bool) -> Cost {
    cost_with_prefetch(columns, reset, 12)
}
#[cfg(test)]
fn partition_with_prefetch(columns: &[Column], prefetch: usize) -> Vec<std::ops::Range<usize>> {
    partition_with_codec(columns, prefetch, Codec::LEGACY)
}
fn partition_with_codec(
    columns: &[Column],
    prefetch: usize,
    codec: Codec,
) -> Vec<std::ops::Range<usize>> {
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
        let prefix = body.min(prefetch * codec.active) / prefetch * prefetch;
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
#[cfg(test)]
fn partition(columns: &[Column]) -> Vec<std::ops::Range<usize>> {
    partition_with_prefetch(columns, 12)
}
fn peak_cost(before: &[u8], after: &[u8], prefetch: usize) -> usize {
    peak_cost_codec(before, after, prefetch, Codec::LEGACY)
}
fn peak_cost_codec(before: &[u8], after: &[u8], prefetch: usize, codec: Codec) -> usize {
    let p = plan_codec(before, after, codec);
    partition_with_codec(&p.columns, prefetch, codec)
        .into_iter()
        .enumerate()
        .map(|(tick, range)| cost_with_codec(&p.columns[range], tick == 0, prefetch, codec).slots)
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
#[cfg(test)]
fn decode(ram: &mut [u8], state: &mut State, payload: &[u8]) -> Result<bool, String> {
    decode_codec(ram, state, payload, Codec::LEGACY)
}
fn decode_codec(
    ram: &mut [u8],
    state: &mut State,
    payload: &[u8],
    codec: Codec,
) -> Result<bool, String> {
    if ram.len() != 65536 || payload.len() < 2 {
        return Err("Invalid v8 RAM/payload".into());
    }
    let flags = payload[0];
    let count = payload[1] as usize;
    if flags & !3 != 0 || count > COLS {
        return Err("Invalid v8 flags/count".into());
    }
    let mut next = state.clone();
    let mut at = 2;
    if flags & 2 != 0 {
        next.lut = payload
            .get(at..at + LUT_BYTES)
            .ok_or("Truncated v8 LUT")?
            .to_vec();
        at += LUT_BYTES;
        if (0..4).any(|k| next.lut[k * 256 + 254] != 0 || next.lut[k * 256 + 255] != 0) {
            return Err("Reserved v8 LUT entries must be zero".into());
        }
        next.initialized = true;
        next.last = None;
    }
    if count > 0 && !next.initialized {
        return Err("v8 columns require a dictionary reset".into());
    }
    let mut writes = Vec::with_capacity(count);
    for _ in 0..count {
        let header = payload
            .get(at..at + 2)
            .ok_or("Truncated v8 column header")?;
        at += 2;
        let col = header[0] as usize;
        let length = header[1] as usize;
        if col >= COLS || next.last.is_some_and(|p| col <= p) {
            return Err("v8 columns must be ascending and unique".into());
        }
        let dest = address_codec(col, codec);
        let mut output = ram[dest..dest + codec.rows].to_vec();
        if length == 255 {
            output = payload
                .get(at..at + codec.rows)
                .ok_or("Truncated v8 RAW192")?
                .to_vec();
            at += codec.rows;
        } else {
            if length > codec.words() * 5 || !length.is_multiple_of(5) {
                return Err("Invalid v8 escape length".into());
            }
            let code = payload
                .get(at..at + codec.words())
                .ok_or("Truncated v8 index plane")?;
            at += codec.words();
            let escaped = payload
                .get(at..at + length)
                .ok_or("Truncated v8 escaped words")?;
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
                        .ok_or("Missing v8 escaped word")?;
                    if e[0] as usize != y {
                        return Err("v8 escape offset must match its255 index".into());
                    }
                    output[y..y + 4].copy_from_slice(&e[1..]);
                    literal += 5;
                }
            }
            if literal != length {
                return Err("Extra v8 escaped words".into());
            }
        }
        if output[codec.active..].iter().any(|&v| v != 0) {
            return Err("v8 bottom padding must remain zero".into());
        }
        writes.push((dest, output));
        next.last = Some(col);
    }
    if at != payload.len() {
        return Err("Trailing v8 bytes".into());
    }
    if flags == 0 && count == 0 && payload.len() != 2 {
        return Err("Invalid v8 EOF hold".into());
    }
    for (dest, bytes) in writes {
        ram[dest..dest + codec.rows].copy_from_slice(&bytes);
    }
    *state = next;
    Ok(flags & 1 != 0)
}
struct Record {
    index: usize,
    offset: usize,
    source: usize,
    prepared: Option<usize>,
    reset: bool,
    complete: bool,
    columns: Vec<Column>,
    words: usize,
    cost: Cost,
}
impl Record {
    fn payload(&self) -> usize {
        self.cost.body + 2
    }
    fn json(&self, total: usize) -> String {
        let raw = self.columns.iter().filter(|c| c.raw).count();
        let dictionary = self.columns.len() - raw;
        format!("{{\"index\":{},\"from_phase\":{},\"to_phase\":{},\"offset\":{},\"payload_offset\":{},\"payload_bytes\":{},\"record_bytes\":{},\"source_content\":{},\"prepared_content\":{},\"reset\":{},\"completion\":{},\"swap\":{},\"column_indices\":{:?},\"column_count\":{},\"raw_columns\":{},\"dictionary_columns\":{},\"dictionary_words\":{},\"dictionary_loads\":{},\"dictionary_bootstrap_bytes\":{},\"escaped_words\":{},\"skipped_words\":{},\"written_bytes\":{},\"command_count\":{},\"runs\":{},\"chunk_count\":{},\"max_chunk_bytes\":{},\"prefetched_body_bytes\":{},\"wire_dma_bytes\":{},\"blank_dma_bytes\":{},\"raw_prefetch_copy_bytes\":{},\"estimated_decoder_cycles\":{},\"estimated_tick_cpu_slots\":{}}}",self.index,self.index,(self.index+1).min(total-1),self.offset,self.offset+2,self.payload(),self.payload()+2,self.source,self.prepared.map_or_else(||"null".into(),|v|v.to_string()),self.reset,self.complete,self.complete,self.columns.iter().map(|c|c.index).collect::<Vec<_>>(),self.columns.len(),raw,dictionary,self.words,usize::from(self.reset),usize::from(self.reset)*1024,self.columns.iter().filter(|c|!c.raw).map(|c|c.escaped.len()/5).sum::<usize>(),self.columns.iter().filter(|c|!c.raw).map(|c|c.code.iter().filter(|&&v|v==254).count()).sum::<usize>(),self.columns.iter().map(Column::writes).sum::<usize>(),self.columns.len(),self.columns.len(),self.columns.len(),self.columns.iter().map(Column::body_len).max().unwrap_or(0),self.cost.prefix,self.payload()+2,self.payload()+2-self.cost.prefix,self.cost.raw_prefix,self.cost.cpu,self.cost.slots)
    }
}
struct Assets {
    stream: Vec<u8>,
    oracles: Vec<u8>,
    records: Vec<Record>,
}
fn encode_with_prefetch(frames: &[u8], prefetch: usize) -> Result<Assets, String> {
    encode_with_codec(frames, prefetch, Codec::LEGACY)
}
fn encode_with_codec(frames: &[u8], prefetch: usize, codec: Codec) -> Result<Assets, String> {
    if frames.is_empty() || !frames.len().is_multiple_of(FRAME) {
        return Err("v8 source oracle alignment".into());
    }
    for f in frames.chunks_exact(FRAME) {
        for c in f.chunks_exact(256) {
            if c[..codec.top]
                .iter()
                .chain(c[codec.top + codec.active..].iter())
                .any(|&v| v != 0)
            {
                return Err("v8 requires constant black bars/padding".into());
            }
        }
    }
    let n = frames.len() / FRAME;
    let mut ram = vec![0xaa; 65536];
    for &(at, size, a) in &REGIONS {
        ram[a..a + size].copy_from_slice(&frames[at..at + size]);
    }
    let mut state = State::new();
    let mut assets = Assets {
        stream: Vec::new(),
        oracles: Vec::with_capacity(n * TICKS * FRAME),
        records: Vec::new(),
    };
    for source in 0..n {
        let target = (source + 1).min(n - 1);
        let after = &frames[target * FRAME..(target + 1) * FRAME];
        let p = plan_codec(&frames[source * FRAME..(source + 1) * FRAME], after, codec);
        let parts = if source + 1 < n {
            partition_with_codec(&p.columns, prefetch, codec)
        } else {
            vec![0..0; TICKS]
        };
        for (tick, range) in parts.into_iter().enumerate() {
            assets.oracles.extend(compact(&ram));
            let reset = source + 1 < n && tick == 0;
            let complete = source + 1 < n && tick + 1 == TICKS;
            let columns = p.columns[range].to_vec();
            let c = cost_with_codec(&columns, reset, prefetch, codec);
            let mut payload = vec![
                u8::from(reset) * 2 + u8::from(complete),
                columns.len() as u8,
            ];
            if reset {
                payload.extend(&p.lut);
            }
            for col in &columns {
                col.append(&mut payload);
            }
            if payload.len() != c.body + 2 || payload.len() > u16::MAX as usize {
                return Err("v8 payload model/size mismatch".into());
            }
            if assets.stream.len() % 65536 + payload.len() + 2 > 65534 {
                assets
                    .stream
                    .resize((assets.stream.len() / 65536 + 1) * 65536, 0);
            }
            if decode_codec(&mut ram, &mut state, &payload, codec)? != complete {
                return Err("v8 completion mismatch".into());
            }
            if tick + 1 == TICKS && compact(&ram) != after {
                return Err(format!("v8 reconstruction mismatch content{source}"));
            }
            let record = Record {
                index: assets.records.len(),
                offset: assets.stream.len(),
                source,
                prepared: (source + 1 < n).then_some(target),
                reset,
                complete,
                columns,
                words: if reset { p.words } else { 0 },
                cost: c,
            };
            assets
                .stream
                .extend_from_slice(&(payload.len() as u16).to_le_bytes());
            assets.stream.extend(payload);
            assets.records.push(record);
        }
    }
    assets
        .stream
        .resize(assets.stream.len().div_ceil(65536) * 65536, 0);
    if ram[0xc400..0xe000].iter().any(|&v| v != 0xaa) {
        return Err("v8 reference altered nonframebuffer RAM".into());
    }
    Ok(assets)
}
#[cfg(test)]
fn encode(frames: &[u8]) -> Result<Assets, String> {
    encode_with_prefetch(frames, 12)
}
fn load(o: &Options) -> Result<small_wide::Loaded, String> {
    if o.wide360_dict {
        return small_wide::convert(o);
    }
    let source =
        fs::canonicalize(o.wide_dict4_source.as_ref().unwrap()).map_err(|e| e.to_string())?;
    let fields =
        json_fields(&fs::read_to_string(source.join("manifest.json")).map_err(|e| e.to_string())?)?;
    for (key, value) in [
        ("width", 360),
        ("height", 200),
        ("storage_height", 256),
        ("frame_ticks", 3),
        ("active_width", 360),
        ("active_height", 190),
        ("active_top", 5),
        ("active_left", 0),
        ("framebuffer_bytes", FRAME),
    ] {
        if scalar::<usize>(&fields, key)? != value {
            return Err(format!("v8 source {key} mismatch"));
        }
    }
    if (scalar::<f64>(&fields, "source_fps")? / (pal_tick_hz() / 3.) - 1.).abs() > 1e-6 {
        return Err("v8 requires genuine PAL/3 source times".into());
    }
    let all = scalar::<usize>(&fields, "source_frames")?;
    let count = o.source_frame_limit.unwrap_or(all);
    if all == 0 || all > 10000 || count == 0 || count > all {
        return Err("v8 source requires1..10000 genuine PAL/3 frames".into());
    }
    let mut frames = fs::read(source.join("oracle.frames")).map_err(|e| e.to_string())?;
    if frames.len() != all * FRAME {
        return Err("v8 source frame size mismatch".into());
    }
    frames.truncate(count * FRAME);
    if fs::read(source.join("initial.bin")).map_err(|e| e.to_string())? != frames[..FRAME] {
        return Err("v8 source initial/oracle mismatch".into());
    }
    let hz = scalar::<u32>(&fields, "audio_sample_rate")?;
    let mut audio = fs::read(source.join("audio.pcm")).map_err(|e| e.to_string())?;
    if !audio.len().is_multiple_of(4) {
        return Err("v8 source PCM alignment".into());
    }
    let duration = count as f64 * TICKS as f64 / pal_tick_hz();
    if count < all {
        audio.truncate((duration * hz as f64).round() as usize * 4);
    }
    Ok(small_wide::Loaded {
        fields,
        frames,
        audio,
        source: Some(source),
        all,
        count,
        audio_hz: hz,
        source_rgb: None,
        mixed_rgb: None,
    })
}
pub fn run(o: &Options) -> Result<(), String> {
    if o.streaming_dict4 {
        return streaming::run(o);
    }
    if o.output.exists() {
        return Err("v8 output must be a new directory".into());
    }
    let small_wide::Loaded {
        mut fields,
        mut frames,
        audio,
        source,
        all,
        count,
        audio_hz: hz,
        source_rgb,
        mixed_rgb,
    } = load(o)?;
    let duration = count as f64 * TICKS as f64 / pal_tick_hz();
    let rate_report = if let Some(budget) = o.dict4_rate_budget {
        let palette = read_palette(&o.palette)?;
        let source_palette = fields.get("palette_rgb").ok_or("Missing source palette")?;
        let source_channels = source_palette
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<u8>().map_err(|_| "Invalid source palette"))
            .collect::<Result<Vec<_>, _>>()?;
        if source_channels != palette.iter().flatten().copied().collect::<Vec<_>>() {
            return Err("Rate-control palette must match the source conversion palette".into());
        }
        let (converted, report) =
            dict4_rate::convert(&frames, &palette, budget, |before, after| {
                peak_cost(before, after, o.dict4_prefetch)
            })?;
        frames = converted;
        Some(report)
    } else {
        None
    };
    let assets = encode_with_prefetch(&frames, o.dict4_prefetch)?;
    if let Some(budget) = o.dict4_rate_budget {
        if assets.records.iter().any(|r| r.cost.slots > budget) {
            return Err("Rate-controlled dictionary exceeded its requested model budget".into());
        }
    }
    fs::create_dir(&o.output).map_err(|e| e.to_string())?;
    if let Some(source) = &source {
        for (name, data, whole) in [
            ("initial.bin", &frames[..FRAME], true),
            (
                "oracle.frames",
                frames.as_slice(),
                count == all && rate_report.is_none(),
            ),
            ("audio.pcm", audio.as_slice(), count == all),
        ] {
            link_or_write(&source.join(name), &o.output.join(name), data, whole)?;
        }
        for name in ["source.rgb", "mixed.rgb"] {
            if name == "mixed.rgb" && rate_report.is_some() {
                continue;
            }
            if count == all {
                fs::hard_link(source.join(name), o.output.join(name)).map_err(|e| e.to_string())?;
            } else {
                let bytes = fs::read(source.join(name)).map_err(|e| e.to_string())?;
                write(&o.output.join(name), &bytes[..count * 360 * 200 * 3])?;
            }
        }
    } else {
        write(&o.output.join("initial.bin"), &frames[..FRAME])?;
        write(&o.output.join("oracle.frames"), &frames)?;
        write(&o.output.join("audio.pcm"), &audio)?;
        write(&o.output.join("source.rgb"), source_rgb.unwrap())?;
        if rate_report.is_none() {
            write(&o.output.join("mixed.rgb"), mixed_rgb.unwrap())?;
        }
    }
    if let Some(report) = &rate_report {
        let palette = read_palette(&o.palette)?;
        let mut rgb = Vec::with_capacity(count * 360 * 200 * 3);
        for frame in frames.chunks_exact(FRAME) {
            for row in 0..200 {
                for col in 0..COLS {
                    let pair = frame[col * 256 + row];
                    rgb.extend(palette[(pair & 15) as usize]);
                    rgb.extend(palette[(pair >> 4) as usize]);
                }
            }
        }
        write(&o.output.join("mixed.rgb"), rgb)?;
        write(&o.output.join("rate-control.json"), report)?;
    }
    write(&o.output.join("oracle.phases"), &assets.oracles)?;
    write(&o.output.join("stream.bin"), &assets.stream)?;
    let total = assets.records.len();
    let max_payload = assets.records.iter().map(Record::payload).max().unwrap();
    let slots = assets.records.iter().map(|r| r.cost.slots).max().unwrap();
    let wire = assets
        .records
        .iter()
        .map(|r| r.payload() + 2)
        .sum::<usize>();
    for key in [
        "records",
        "raw_column_counts_per_content",
        "raw_columns_prefetched_per_tick",
        "raw_active_columns",
    ] {
        fields.remove(key);
    }
    let updates=vec![
        ("format",quoted("MUpic progressive360 word dictionary v8")),("span_version","8".into()),("dictionary4","true".into()),("raw_active_columns","false".into()),("source_asset_directory",source.as_ref().map_or_else(||"null".into(),|s|quoted(&s.to_string_lossy()))),("fresh_source_conversion",o.wide360_dict.to_string()),("source_conversion_frames",all.to_string()),("source_frames",count.to_string()),("logical_oracle_frames",count.to_string()),("duration_seconds",duration.to_string()),("phases",total.to_string()),("display_ticks",total.to_string()),
        ("phase_order",quoted("Three progressive PAL ticks per genuine sampled content frame; sparse exact4-byte words/RAW192 updatefront; finaltick completes; EOF holds finalframe")),("stream_bytes",assets.stream.len().to_string()),("stream_blocks",(assets.stream.len()/65536).to_string()),("record_bytes_total",wire.to_string()),("video_record_bytes_per_second",(wire as f64/duration).to_string()),("max_packet_bytes",max_payload.to_string()),("max_tick_payload_bytes",max_payload.to_string()),("max_chunk_bytes",assets.records.iter().flat_map(|r|&r.columns).map(Column::body_len).max().unwrap_or(0).to_string()),("buffered_chunk_capacity","288".into()),("chunk_count_total",assets.records.iter().map(|r|r.columns.len()).sum::<usize>().to_string()),("completion_count",(count-1).to_string()),("dictionary_loads_total",(count-1).to_string()),("dictionary_raw_columns_total",assets.records.iter().flat_map(|r|&r.columns).filter(|c|c.raw).count().to_string()),("dictionary_columns_total",assets.records.iter().flat_map(|r|&r.columns).filter(|c|!c.raw).count().to_string()),
        ("dictionary_entries","254".into()),("dictionary_entry_bytes","4".into()),("dictionary_word_rows","192".into()),("dictionary_planar_bytes","1024".into()),("dictionary_address","50688".into()),("dictionary_stage_address","56320".into()),("dictionary_stage_capacity","288".into()),("raw_literal_column_bytes","192".into()),("row_prefetch_max_bytes","2280".into()),("row_prefetch_bytes_per_line","12".into()),("row_prefetch_semantics",quoted("Body ONLY afterflags/count, includingfirst1024LUT, percolumnheaders/indices/escaped/raw. P=12*floor(min(bodybytes,2280)/12); partialrow prefetch disablesafterP/12 rows; remainingbytes DMA inblank.")),
        ("estimated_tick_cpu_slots_max",slots.to_string()),("tick_budget_cpu_slots",o.tick_budget.to_string()),("fits_tick_budget",(slots<=o.tick_budget).to_string()),("max_estimated_decoder_cycles",assets.records.iter().map(|r|r.cost.cpu).max().unwrap().to_string()),("max_written_bytes",assets.records.iter().map(|r|r.columns.iter().map(Column::writes).sum::<usize>()).max().unwrap().to_string()),("dictionary_frequency_ties",quoted("Changed NEW4byteword descendingfrequency; tiesbyfirstencounterinascendingcolumnthenrow. Slots254/255reservedzero. Unusedentrieszero.")),
        ("progressive_wire_format",quoted("OuterLE16length; flags bit1reset(always1024planarLUT) andbit0complete,countu8. Eachsparseascendingcolumn:indexu8,escapebytesu8(0..240multiple5) OR255RAW;DICT48indices+escaped(rowoffsetu8,4bytes),RAW192bytes. Index0..253dictionary,254SKIPoldword,255escapedword. Rows5..196includingtwozerobottompads. Emptycompletion flags1count0 legal;EOF[0,0].")),("progressive_tick_cost_model",quoted("Preliminary conservative NMOSprimitivebound perDICTcolumn:801+45*lookupwords+14*controlwords+controlwords(pagecrossbranchmargin)+(escapes>0?4+62*escapes:0)+max(47,escapedbytes-1) sourcealignment. RAWprimitive0. Add500/column+5000/tick+15*1024firstLUTrelocation+15*exactprefetchedRAWbytes. DMA63*(outer2+flags/count2+body-P). Exactminimaxcontiguouspartitionover3ticks; allsourcepixeldataunchanged. Integratedactual6502/parser/cache/raster timing remainsauthoritative.")),
        ("audio_samples",(audio.len()/4).to_string()),("audio_bytes",audio.len().to_string()),("native_cache_ticks",o.cache_ticks.unwrap().to_string()),("reference_roundtrip_verified","true".into()),("source_conversion_metrics_scope",quoted(if o.wide360_dict {"Fresh genuine PAL/3 RGB conversion uses the same author/packing/audio pipeline asv7; no RAW transport intermediate"} else {"Original genuine PAL/3 source conversion; originalRGB/logicalpixels/PCM are reused without modification"})),
        ("records",format!("[\n{}\n]",assets.records.iter().map(|r|r.json(total)).collect::<Vec<_>>().join(",\n"))),
    ];
    for (key, value) in updates {
        fields.insert(key.into(), value);
    }
    fields.insert(
        "row_prefetch_max_bytes".into(),
        (o.dict4_prefetch * 190).to_string(),
    );
    fields.insert(
        "row_prefetch_bytes_per_line".into(),
        o.dict4_prefetch.to_string(),
    );
    fields.insert("row_prefetch_semantics".into(), quoted(&format!("Body only after flags/count; P={}*floor(min(bodybytes,{})/{}). Player must use the same bytes per row.", o.dict4_prefetch, o.dict4_prefetch*190, o.dict4_prefetch)));
    fields.insert(
        "lossy_rate_control".into(),
        rate_report.is_some().to_string(),
    );
    if let Some(report) = rate_report {
        fields.insert("rate_control".into(), report);
        if let Some(original) = fields.remove("mean_oklab_error") {
            fields.insert("pre_rate_control_mean_oklab_error".into(), original);
        }
        fields.insert("source_conversion_metrics_scope".into(), quoted("Author/neutral-guard conversion followed by explicitly lossy word rate control; mixed.rgb and oracle.frames describe the reconstructed result, source.rgb remains the genuine PAL/3 source. See rate-control.json for measured changes against the author reference."));
        fields.insert("phase_order".into(), quoted("Three progressive PAL ticks per genuine source sample; perceptual word rate control within the requested CPU/DMA model budget; source timestamps and PCM unchanged. Final tick completes each reconstructed image."));
        fields.insert(
            "reference_author_frames".into(),
            source.as_ref().map_or_else(
                || "null".into(),
                |s| quoted(&s.join("oracle.frames").to_string_lossy()),
            ),
        );
        fields.insert("progressive_tick_cost_model".into(), quoted("NMOS dictionary primitive and parser/setup allowances plus63 slots per DMA byte; exact minimax three-record partition; word rate control verified against this model on the reconstructed previous frame. Integrated actual6502/native timing remains authoritative."));
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
    println!("v8 {count}sourceframes/{total}ticks: maxpayload{max_payload}, wire{wire}, modelpeak{slots}/{}",o.tick_budget);
    if o.native_mux {
        let mux = mux_fixed_tick_blocks(
            &assets.stream,
            &audio,
            total,
            o.audio_divider,
            o.audio_offset,
            o.cache_ticks.unwrap(),
        )?;
        write(&o.output.join("stream.mup"), &mux.file)?;
        write(
            &o.output.join("cache-mux.json"),
            mux.metadata(o.audio_offset, o.audio_divider, hz),
        )?;
    }
    if o.strict_budget && max_payload > o.packet_budget {
        return Err("v8 exceedspacketbudget; statistics retained".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thirteen_byte_prefetch_partition_matches_brute_force_cost() {
        let before = vec![0; FRAME];
        let mut after = before.clone();
        for c in 0..12 {
            for y in 0..190 {
                after[c * 256 + TOP + y] = ((c * 41 + y * 23 + y * y) & 255) as u8;
            }
        }
        let p = plan(&before, &after);
        for prefetch in [12, 13] {
            let parts = partition_with_prefetch(&p.columns, prefetch);
            let actual = parts
                .iter()
                .enumerate()
                .map(|(i, r)| cost_with_prefetch(&p.columns[r.clone()], i == 0, prefetch).slots)
                .max()
                .unwrap();
            let mut expected = usize::MAX;
            for a in 0..=p.columns.len() {
                for b in a..=p.columns.len() {
                    expected = expected.min(
                        cost_with_prefetch(&p.columns[..a], true, prefetch)
                            .slots
                            .max(cost_with_prefetch(&p.columns[a..b], false, prefetch).slots)
                            .max(cost_with_prefetch(&p.columns[b..], false, prefetch).slots),
                    );
                }
            }
            assert_eq!(actual, expected);
        }
    }
    #[test]
    fn dictionary_ties_follow_first_column_row_and_skips_keep_previous_words() {
        let before = vec![0; FRAME];
        let mut after = before.clone();
        let a = [1, 2, 3, 4];
        let b = [9, 8, 7, 6];
        for (col, row, value) in [(0, 0, a), (0, 1, b), (1, 0, b), (1, 1, a)] {
            after[col * 256 + TOP + row * 4..col * 256 + TOP + row * 4 + 4].copy_from_slice(&value);
        }
        let p = plan(&before, &after);
        assert_eq!(p.words, 2);
        assert_eq!(p.columns.len(), 2);
        assert_eq!((0..4).map(|k| p.lut[k * 256]).collect::<Vec<_>>(), a);
        assert_eq!((0..4).map(|k| p.lut[k * 256 + 1]).collect::<Vec<_>>(), b);
        assert_eq!(&p.columns[0].code[..2], &[0, 1]);
        assert!(p.columns[0].code[2..].iter().all(|&v| v == 254));
        assert_eq!(p.lut, plan(&before, &after).lut);
    }
    fn reset_payload(count: u8) -> Vec<u8> {
        let mut p = vec![2, count];
        p.extend([0; 1024]);
        p
    }
    #[test]
    fn malformed_lengths_offsets_indices_padding_and_reserved_entries_are_transactional() {
        let mut bad = vec![
            vec![4, 0],
            vec![2, 0],
            vec![0, 1],
            vec![0, 0, 5],
            vec![0, 181],
        ];
        let mut p = reset_payload(0);
        p[2 + 254] = 1;
        bad.push(p);
        for header in [[180, 255], [0, 1], [0, 245], [0, 252]] {
            let mut p = reset_payload(1);
            p.extend(header);
            bad.push(p);
        }
        let mut p = reset_payload(1);
        p.extend([0, 0]);
        p.extend([255; 48]);
        bad.push(p);
        let mut p = reset_payload(1);
        p.extend([0, 5]);
        p.push(255);
        p.extend([254; 47]);
        p.extend([4, 1, 2, 3, 4]);
        bad.push(p);
        let mut p = reset_payload(1);
        p.extend([0, 5]);
        p.extend([254; 48]);
        p.extend([0, 1, 2, 3, 4]);
        bad.push(p);
        let mut p = reset_payload(1);
        p.extend([0, 255]);
        p.extend([0; 192]);
        *p.last_mut().unwrap() = 1;
        bad.push(p);
        for p in bad {
            let mut ram = vec![0; 65536];
            let mut state = State::new();
            assert!(decode(&mut ram, &mut state, &p).is_err());
            assert!(!state.initialized);
            assert_eq!(state.last, None);
            assert!(ram.iter().all(|&v| v == 0));
        }
        let mut ram = vec![0; 65536];
        let mut state = State::new();
        let mut p = reset_payload(1);
        p.extend([9, 255]);
        p.extend([0; 192]);
        decode(&mut ram, &mut state, &p).unwrap();
        let before = ram.clone();
        let mut p = vec![0, 1, 9, 255];
        p.extend([0; 192]);
        assert!(decode(&mut ram, &mut state, &p).is_err());
        assert_eq!(ram, before);
        assert_eq!(state.last, Some(9));
        let mut p = reset_payload(0);
        p[0] = 3;
        assert!(decode(&mut ram, &mut state, &p).unwrap());
        assert_eq!(state.last, None);
        assert!(!decode(&mut ram, &mut state, &[0, 0]).unwrap());
    }
    #[test]
    fn dictionary_raw_escape_and_partial_tick_replay_match_all_frames_and_eof() {
        let mut frames = vec![0; FRAME * 3];
        for c in 0..COLS {
            for row in 0..48 {
                let value = (c * 48 + row + 1) as u32;
                let at = FRAME + c * 256 + TOP + row * 4;
                frames[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        let previous = frames[FRAME..FRAME * 2].to_vec();
        frames[FRAME * 2..].copy_from_slice(&previous);
        frames[FRAME * 2 + TOP..FRAME * 2 + TOP + 4].copy_from_slice(&[11, 22, 33, 44]);
        let assets = encode(&frames).unwrap();
        assert_eq!(assets.records.len(), 9);
        assert!(assets
            .records
            .iter()
            .flat_map(|r| &r.columns)
            .any(|c| c.raw));
        assert!(assets
            .records
            .iter()
            .flat_map(|r| &r.columns)
            .any(|c| !c.raw));
        let mut ram = vec![0xaa; 65536];
        for &(at, n, a) in &REGIONS {
            ram[a..a + n].copy_from_slice(&frames[at..at + n]);
        }
        let mut state = State::new();
        for r in &assets.records {
            assert_eq!(
                compact(&ram),
                assets.oracles[r.index * FRAME..(r.index + 1) * FRAME]
            );
            let payload = &assets.stream[r.offset + 2..r.offset + 2 + r.payload()];
            assert_eq!(decode(&mut ram, &mut state, payload).unwrap(), r.complete);
            assert!(r.cost.prefix <= 2280 && r.cost.prefix.is_multiple_of(12));
            if r.complete {
                assert_eq!(
                    compact(&ram),
                    frames[(r.source + 1) * FRAME..(r.source + 2) * FRAME]
                );
            }
        }
        assert!(assets.records[6..]
            .iter()
            .all(|r| r.columns.is_empty() && !r.reset && !r.complete && r.payload() == 2));
        assert!(ram[0xc400..0xe000].iter().all(|&v| v == 0xaa));
        // Exercise escaped-word decode explicitly (slots beyond the254-word LUT).
        let mut ram = vec![0; 65536];
        let mut state = State::new();
        let mut p = reset_payload(1);
        p.extend([0, 5]);
        p.push(255);
        p.extend([254; 47]);
        p.extend([0, 5, 6, 7, 8]);
        decode(&mut ram, &mut state, &p).unwrap();
        assert_eq!(&ram[address(0)..address(0) + 4], &[5, 6, 7, 8]);
    }
    #[test]
    fn native_sd_roundtrip_covers_first_last_rows_and_preserves_vertical_bars() {
        let codec = Codec::FULL_HEIGHT;
        let mut frames = vec![0; FRAME * 3];
        for f in 1..3 {
            for c in 1..COLS - 1 {
                for y in 0..codec.rows {
                    frames[f * FRAME + c * 256 + y] = ((c * 37 + y * 13 + f * 91) % 251 + 1) as u8;
                }
            }
        }
        let assets = encode_with_codec(&frames, 13, codec).unwrap();
        let mut ram = vec![0xaa; 65536];
        for &(at, n, a) in &REGIONS {
            ram[a..a + n].copy_from_slice(&frames[at..at + n]);
        }
        let mut state = State::new();
        for record in &assets.records {
            let payload = &assets.stream[record.offset + 2..record.offset + 2 + record.payload()];
            assert_eq!(
                decode_codec(&mut ram, &mut state, payload, codec).unwrap(),
                record.complete
            );
            if record.complete {
                assert_eq!(
                    compact(&ram),
                    frames[(record.source + 1) * FRAME..(record.source + 2) * FRAME]
                );
            }
            assert!(record.cost.prefix <= 2600 && record.cost.prefix % 13 == 0);
        }
        let final_frame = compact(&ram);
        for c in 0..COLS {
            assert!(final_frame[c * 256 + 200..(c + 1) * 256]
                .iter()
                .all(|&v| v == 0));
        }
        assert!(final_frame[..256].iter().all(|&v| v == 0));
        assert!(final_frame[(COLS - 1) * 256..].iter().all(|&v| v == 0));
        assert_eq!(primitive_cycles(&[0; 50], 0), 3132);
        assert_eq!(primitive_cycles(&[254; 50], 0), 1632);
        // Decode the longest legal escaped body: fifty tokens and250 literal bytes.
        let mut payload = vec![2, 1];
        payload.extend(vec![0; LUT_BYTES]);
        payload.extend([1, 250]);
        payload.extend(vec![255; 50]);
        for row in 0..50 {
            payload.push((row * 4) as u8);
            payload.extend([row as u8 + 1; 4]);
        }
        let mut state = State::new();
        let mut ram = vec![0; 65536];
        decode_codec(&mut ram, &mut state, &payload, codec).unwrap();
        assert_eq!(
            &ram[address_codec(1, codec) + 196..address_codec(1, codec) + 200],
            &[50; 4]
        );
        assert_eq!(
            Column {
                index: 1,
                raw: false,
                code: vec![255; 50],
                escaped: vec![0; 250],
                target: vec![0; 200],
                cpu: 0
            }
            .body_len(),
            300
        );
    }
    #[test]
    fn native_sd_cli_is_explicit_and_rejects_old_blank_budget() {
        let args = |extras: &[&str]| {
            [
                "--input",
                "source",
                "--output",
                "new",
                "--wide360-dict",
                "--streaming-dict4",
                "--dict4-full-height",
                "--frame-ticks",
                "3",
                "--colour-mode",
                "hardware",
                "--fit-canvas",
                "--pixel-aspect",
                "1",
                "--dither",
                "yliluoma-author",
            ]
            .into_iter()
            .chain(extras.iter().copied())
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
        };
        assert!(
            parse_options_args(args(&[]))
                .unwrap()
                .unwrap()
                .dict4_full_height
        );
        assert!(parse_options_args(args(&["--dict4-rate-budget", "425000"])).is_ok());
        assert!(parse_options_args(args(&["--dict4-rate-budget", "450000"])).is_err());
        assert!(parse_options_args(args(&["--pixel-aspect", "15:16"])).is_err());
    }
    #[test]
    fn timing_partition_is_exact_minimax_and_counts_only_prefetched_raw_data() {
        let cols = (0..6)
            .map(|i| Column {
                index: i,
                raw: i % 2 == 0,
                code: vec![254; 48],
                escaped: Vec::new(),
                target: vec![0; 192],
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
        let brute = (0..=6)
            .flat_map(|a| (a..=6).map(move |b| (a, b)))
            .map(|(a, b)| {
                cost(&cols[..a], true)
                    .slots
                    .max(cost(&cols[a..b], false).slots)
                    .max(cost(&cols[b..], false).slots)
            })
            .min()
            .unwrap();
        assert_eq!(got, brute);
        assert_eq!(parts[0].start, 0);
        assert_eq!(parts[2].end, 6);
        let first = cost(&[], true);
        assert_eq!(first.body, 1024);
        assert_eq!(first.prefix, 1020);
        assert_eq!(first.slots, 20864);
        let partial = cost(&cols[..1], false);
        assert_eq!(partial.body, 194);
        assert_eq!(partial.prefix, 192);
        assert_eq!(partial.raw_prefix, 190);
        assert_eq!(primitive_cycles(&[0; 48], 0), 3008);
        assert_eq!(primitive_cycles(&[254; 48], 0), 1568);
    }
    #[test]
    fn direct_conversion_cli_requires_explicit_pal3_and_exact_colour_geometry() {
        let args = |extra: &[&str]| {
            [
                "--input",
                "source",
                "--output",
                "new",
                "--wide360-dict",
                "--frame-ticks",
                "3",
                "--colour-mode",
                "hardware",
                "--fit-canvas",
                "--pixel-aspect",
                "15:16",
                "--dither",
                "yliluoma-author",
            ]
            .into_iter()
            .chain(extra.iter().copied())
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
        };
        let o = parse_options_args(args(&[])).unwrap().unwrap();
        assert!(o.wide360_dict);
        assert!(o.wide_dict4_source.is_none());
        assert_eq!(o.upic_width, 360);
        assert_eq!(o.frame_ticks, Some(3));
        assert_eq!(o.cache_ticks, Some(20));
        assert_eq!(o.fps, pal_tick_hz() / 3.);
        assert!(parse_options_args(args(&["--frame-ticks", "4"])).is_err());
        assert!(parse_options_args(args(&["--colour-mode", "temporal"])).is_err());
        assert!(parse_options_args(args(&["--scale2"])).is_err());
        assert!(parse_options_args(args(&["--dither", "jjn"])).is_err());
        assert!(parse_options_args(args(&["--upic-width", "384"])).is_err());
        assert!(parse_options_args(args(&["--upic-width", "400"])).is_err());
        assert!(parse_options_args(args(&["--wide-dict4-source", "other"])).is_err());
        assert!(parse_options_args(
            ["--input", "source", "--output", "new", "--wide360-dict"]
                .into_iter()
                .map(str::to_string)
        )
        .is_err());
    }
    #[test]
    fn cli_requires_genuine_pal3_reuse_and_is_separate_from_other_transports() {
        let args = |extra: &[&str]| {
            ["--wide-dict4-source", "sample", "--output", "new"]
                .into_iter()
                .chain(extra.iter().copied())
                .map(str::to_string)
                .collect::<Vec<_>>()
                .into_iter()
        };
        let o = parse_options_args(args(&[])).unwrap().unwrap();
        assert_eq!(o.frame_ticks, Some(3));
        assert_eq!(o.cache_ticks, Some(20));
        assert_eq!(o.audio_offset, 196608);
        assert_eq!(o.fps, pal_tick_hz() / 3.);
        assert!(parse_options_args(args(&["--frame-ticks", "4"])).is_err());
        assert!(parse_options_args(args(&["--cache-ticks", "24"])).is_err());
        assert_eq!(
            parse_options_args(args(&["--cache-ticks", "16"]))
                .unwrap()
                .unwrap()
                .cache_ticks,
            Some(16)
        );
        assert!(parse_options_args(args(&["--wide360-raw-source", "other"])).is_err());
        assert!(parse_options_args(args(&["--lz4-coalesce"])).is_err());
    }
}

/// The public SD encoder uses this same qualified packet planner and cost model.
pub(super) fn sd_peak(before: &[u8], after: &[u8]) -> usize {
    peak_cost_codec(before, after, 13, Codec::FULL_HEIGHT)
}

pub(super) fn sd_group(before: &[u8], after: Option<&[u8]>) -> Result<(Vec<Vec<u8>>, usize), String> {
    let codec = Codec::FULL_HEIGHT;
    if before.len() != FRAME || after.is_some_and(|f| f.len() != FRAME) {
        return Err("SD frame must contain 46080 page-stride bytes".into());
    }
    let mut ram = vec![0xaa; 65536];
    for &(at, count, address) in &REGIONS {
        ram[address..address + count].copy_from_slice(&before[at..at + count]);
    }
    let mut decoder = State::new();
    let plan = after.map(|target| plan_codec(before, target, codec));
    let parts = plan.as_ref().map_or_else(|| vec![0..0; TICKS], |p| partition_with_codec(&p.columns, 13, codec));
    let mut packets = Vec::with_capacity(TICKS);
    let mut peak = 0;
    for (tick, range) in parts.into_iter().enumerate() {
        let reset = after.is_some() && tick == 0;
        let complete = after.is_some() && tick + 1 == TICKS;
        let columns = plan.as_ref().map_or(&[][..], |p| &p.columns[range]);
        let cost = cost_with_codec(columns, reset, 13, codec);
        peak = peak.max(cost.slots);
        let mut payload = vec![u8::from(reset) * 2 + u8::from(complete), columns.len() as u8];
        if reset { payload.extend_from_slice(&plan.as_ref().unwrap().lut); }
        for column in columns { column.append(&mut payload); }
        if payload.len() != cost.body + 2 || payload.len() > u16::MAX as usize {
            return Err("SD payload cost/size mismatch".into());
        }
        if decode_codec(&mut ram, &mut decoder, &payload, codec)? != complete {
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
mod public_sd_tests {
    use super::*;
    #[test]
    fn public_group_matches_original_packets_byte_for_byte() {
        let mut frames = vec![0; FRAME * 2];
        for c in 1..179 { for y in 0..200 {
            frames[c * 256 + y] = ((c + y / 8) & 15) as u8 * 17;
            frames[FRAME + c * 256 + y] = ((c + y / 8 + 1) & 15) as u8 * 17;
        } }
        let original = encode_with_codec(&frames, 13, Codec::FULL_HEIGHT).unwrap();
        let (packets, peak) = sd_group(&frames[..FRAME], Some(&frames[FRAME..])).unwrap();
        for (record, payload) in original.records.iter().take(3).zip(&packets) {
            assert_eq!(&original.stream[record.offset + 2..record.offset + 2 + record.payload()], payload);
        }
        assert_eq!(peak, original.records.iter().take(3).map(|r| r.cost.slots).max().unwrap());
        let final_original = encode_with_codec(&frames[FRAME..], 13, Codec::FULL_HEIGHT).unwrap();
        let (holds, _) = sd_group(&frames[FRAME..], None).unwrap();
        for (record, payload) in final_original.records.iter().zip(&holds) {
            assert_eq!(&final_original.stream[record.offset + 2..record.offset + 2 + record.payload()], payload);
        }
    }
}
