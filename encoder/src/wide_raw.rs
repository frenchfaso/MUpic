//! Isolated v5: constant bars stay in the front; active columns are copied
//! directly from REU, with the first nine columns prefetched into ordinary RAM.
//! Conversion is reused byte for byte from the existing PAL/5 author assets.
use super::wide_lz4::{json_fields, link_or_write, scalar};
use super::*;

const FRAME: usize = wide::FRAME_BYTES;
const COLUMNS: usize = 192;
const TOP: usize = 26;
const ROWS: usize = 203;
const TICKS: usize = 5;
const COUNTS: [usize; TICKS] = [38, 38, 38, 39, 39];
const PREFIX_COLUMNS: usize = 9;
const PREFETCH_BYTES: usize = PREFIX_COLUMNS * ROWS;

fn column_address(column: usize) -> usize {
    if column < 8 {
        0xe000 + column * 256
    } else {
        0x1800 + (column - 8) * 256
    }
}
fn compact(ram: &[u8]) -> Vec<u8> {
    wide::REGIONS
        .iter()
        .flat_map(|&(_, n, a)| ram[a..a + n].iter().copied())
        .collect()
}
fn active(frame: &[u8], column: usize, count: usize) -> Vec<u8> {
    (column..column + count)
        .flat_map(|c| frame[c * 256 + TOP..c * 256 + TOP + ROWS].iter().copied())
        .collect()
}
fn decode(ram: &mut [u8], column: &mut usize, payload: &[u8]) -> Result<bool, String> {
    if ram.len() != 65536 || payload.len() < 2 {
        return Err("Invalid v5 RAM or payload".into());
    }
    let flags = payload[0];
    let count = payload[1] as usize;
    if flags & !3 != 0 || payload.len() != 2 + count * ROWS {
        return Err("Invalid v5 flags or raw column length".into());
    }
    if count == 0 && flags != 0 {
        return Err("Empty v5 record must be an EOF hold".into());
    }
    let start = if flags & 2 != 0 { 0 } else { *column };
    if start + count > COLUMNS || (flags & 1 != 0 && start + count != COLUMNS) {
        return Err("v5 column bounds or incomplete completion".into());
    }
    // Validate every field before touching the active framebuffer.
    for (i, bytes) in payload[2..].chunks_exact(ROWS).enumerate() {
        let dest = column_address(start + i) + TOP;
        ram[dest..dest + ROWS].copy_from_slice(bytes);
    }
    *column = start + count;
    Ok(flags & 1 != 0)
}
struct Record {
    index: usize,
    offset: usize,
    source: usize,
    prepared: Option<usize>,
    start: usize,
    count: usize,
    reset: bool,
    complete: bool,
}
impl Record {
    fn payload(&self) -> usize {
        2 + self.count * ROWS
    }
    fn prefix_columns(&self) -> usize {
        if self.count >= PREFIX_COLUMNS {
            PREFIX_COLUMNS
        } else {
            0
        }
    }
    fn prefix(&self) -> usize {
        self.prefix_columns() * ROWS
    }
    fn cpu(&self) -> usize {
        // Preliminary model; direct decoder native measurements are decisive.
        1000 + 13 * self.prefix()
            + 300 * self.prefix_columns()
            + 40 * (self.count - self.prefix_columns())
    }
    fn slots(&self) -> usize {
        self.cpu() + 63 * (self.payload() + 2 - self.prefix())
    }
    fn json(&self, total: usize) -> String {
        format!("{{\"index\":{},\"from_phase\":{},\"to_phase\":{},\"offset\":{},\"payload_offset\":{},\"payload_bytes\":{},\"record_bytes\":{},\"source_content\":{},\"prepared_content\":{},\"swap\":{},\"completion\":{},\"reset\":{},\"logical_column_start\":{},\"logical_column_end\":{},\"raw_column_count\":{},\"active_raw_bytes\":{},\"written_bytes\":{},\"copy_bytes\":{},\"copy_commands\":{},\"command_count\":{},\"runs\":{},\"chunk_count\":{},\"max_chunk_bytes\":{},\"prefetched_columns\":{},\"prefetched_body_bytes\":{},\"wire_dma_bytes\":{},\"blank_dma_bytes\":{},\"estimated_decoder_cycles\":{},\"estimated_tick_cpu_slots\":{}}}",self.index,self.index,(self.index+1).min(total-1),self.offset,self.offset+2,self.payload(),self.payload()+2,self.source,self.prepared.map_or_else(||"null".into(),|v|v.to_string()),self.complete,self.complete,self.reset,self.start,self.start+self.count,self.count,self.count*ROWS,self.count*ROWS,self.count*ROWS,self.count,self.count,self.count,self.count,if self.count>0 {ROWS} else {0},self.prefix_columns(),self.prefix(),self.payload()+2,self.payload()+2-self.prefix(),self.cpu(),self.slots())
    }
}
struct Assets {
    stream: Vec<u8>,
    oracles: Vec<u8>,
    records: Vec<Record>,
}
fn encode(frames: &[u8]) -> Result<Assets, String> {
    if frames.is_empty() || !frames.len().is_multiple_of(FRAME) {
        return Err("Invalid v5 logical frames".into());
    }
    let contents = frames.len() / FRAME;
    for frame in frames.chunks_exact(FRAME) {
        for col in frame.chunks_exact(256) {
            if col[..TOP].iter().chain(&col[TOP + ROWS..]).any(|&b| b != 0) {
                return Err(
                    "v5 raw active transport requires constant black top/bottom bars".into(),
                );
            }
        }
    }
    let mut ram = vec![0xaa; 65536];
    for &(at, n, a) in &wide::REGIONS {
        ram[a..a + n].copy_from_slice(&frames[at..at + n]);
    }
    let mut column = COLUMNS;
    let mut assets = Assets {
        stream: Vec::new(),
        oracles: Vec::with_capacity(contents * TICKS * FRAME),
        records: Vec::new(),
    };
    for content in 0..contents {
        let target = &frames[(content + 1).min(contents - 1) * FRAME
            ..((content + 1).min(contents - 1) + 1) * FRAME];
        for (tick, &tick_count) in COUNTS.iter().enumerate() {
            assets.oracles.extend(compact(&ram));
            let count = if content + 1 < contents {
                tick_count
            } else {
                0
            };
            let reset = content + 1 < contents && tick == 0;
            let complete = content + 1 < contents && tick + 1 == TICKS;
            let start = if reset { 0 } else { column };
            let mut payload = vec![u8::from(reset) * 2 + u8::from(complete), count as u8];
            payload.extend(active(target, start, count));
            if assets.stream.len() % 65536 + payload.len() + 2 > 65534 {
                assets
                    .stream
                    .resize((assets.stream.len() / 65536 + 1) * 65536, 0);
            }
            if decode(&mut ram, &mut column, &payload)? != complete {
                return Err("v5 completion mismatch".into());
            }
            if tick + 1 == TICKS && compact(&ram) != target {
                return Err(format!("v5 reconstruction mismatch content{content}"));
            }
            let record = Record {
                index: assets.records.len(),
                offset: assets.stream.len(),
                source: content,
                prepared: (content + 1 < contents).then_some(content + 1),
                start,
                count,
                reset,
                complete,
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
    if ram[0xd000..0xe000].iter().any(|&v| v != 0xaa) {
        return Err("v5 reference altered I/O gap".into());
    }
    Ok(assets)
}
pub fn run(o: &Options) -> Result<(), String> {
    let source =
        fs::canonicalize(o.wide_raw_source.as_ref().unwrap()).map_err(|e| e.to_string())?;
    let mut fields =
        json_fields(&fs::read_to_string(source.join("manifest.json")).map_err(|e| e.to_string())?)?;
    if scalar::<usize>(&fields, "width")? != 384
        || scalar::<usize>(&fields, "height")? != 256
        || scalar::<usize>(&fields, "frame_ticks")? != TICKS
        || scalar::<usize>(&fields, "active_height")? != ROWS
        || scalar::<usize>(&fields, "active_top")? != TOP
        || scalar::<usize>(&fields, "active_left")? != 0
        || scalar::<usize>(&fields, "active_width")? != 384
        || (scalar::<f64>(&fields, "source_fps")? / (pal_tick_hz() / TICKS as f64) - 1.).abs()
            > 1e-6
    {
        return Err("v5 reuse requires384x256/active384x203 top26, PAL/5 source".into());
    }
    let all = scalar::<usize>(&fields, "source_frames")?;
    let count = o.source_frame_limit.unwrap_or(all);
    if count == 0 || count > all {
        return Err("Source frame limit exceeds available frames".into());
    }
    let frame_path = source.join("oracle.frames");
    let mut frames = fs::read(&frame_path).map_err(|e| e.to_string())?;
    if frames.len() != all * FRAME {
        return Err("Source v5 oracle.frames size mismatch".into());
    }
    frames.truncate(count * FRAME);
    if fs::read(source.join("initial.bin")).map_err(|e| e.to_string())? != frames[..FRAME] {
        return Err("Source v5 initial/oracle mismatch".into());
    }
    let audio_hz = scalar::<u32>(&fields, "audio_sample_rate")?;
    let mut audio = fs::read(source.join("audio.pcm")).map_err(|e| e.to_string())?;
    if !audio.len().is_multiple_of(4) {
        return Err("Source v5 PCM alignment".into());
    }
    let duration = count as f64 * TICKS as f64 / pal_tick_hz();
    if count != all {
        audio.truncate((duration * audio_hz as f64).round() as usize * 4);
    }
    println!("Reusing{count}/{all} exact frames for v5 raw active-column direct DMA");
    let assets = encode(&frames)?;
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
    let records = assets.records.len();
    let total_wire = assets
        .records
        .iter()
        .map(|r| r.payload() + 2)
        .sum::<usize>();
    let max_slots = assets.records.iter().map(Record::slots).max().unwrap();
    let max_payload = assets.records.iter().map(Record::payload).max().unwrap();
    fields.remove("records");
    let updates=vec![
        ("format",quoted("MUpic progressive wide raw v5")),("span_version","5".into()),("raw_active_columns","true".into()),
        ("source_asset_directory",quoted(&source.to_string_lossy())),("source_conversion_frames",all.to_string()),("source_frames",count.to_string()),
        ("logical_oracle_frames",count.to_string()),("duration_seconds",duration.to_string()),("phases",records.to_string()),("display_ticks",records.to_string()),
        ("phase_order",quoted("Adjacent target active columns copied progressively across five PAL ticks, [38,38,38,39,39]; final tick completes; EOF holds final image")),
        ("stream_bytes",assets.stream.len().to_string()),("stream_blocks",(assets.stream.len()/65536).to_string()),("record_bytes_total",total_wire.to_string()),
        ("video_record_bytes_per_second",(total_wire as f64/duration).to_string()),("max_packet_bytes",max_payload.to_string()),("max_tick_payload_bytes",max_payload.to_string()),
        ("max_chunk_bytes",if count>1 {ROWS} else {0}.to_string()),("buffered_chunk_capacity",ROWS.to_string()),("chunk_count_total",assets.records.iter().map(|r|r.count).sum::<usize>().to_string()),
        ("completion_count",(count-1).to_string()),("raw_column_counts_per_content","[38,38,38,39,39]".into()),("raw_active_frame_bytes",(COLUMNS*ROWS).to_string()),
        ("raw_active_column_bytes",ROWS.to_string()),("raw_active_column_count",COLUMNS.to_string()),("raw_columns_prefetched_per_tick",PREFIX_COLUMNS.to_string()),
        ("row_prefetch_max_bytes",PREFETCH_BYTES.to_string()),("row_prefetch_bytes_per_line","9".into()),
        ("estimated_tick_cpu_slots_max",max_slots.to_string()),("tick_budget_cpu_slots",o.tick_budget.to_string()),("fits_tick_budget",(max_slots<=o.tick_budget).to_string()),
        ("max_estimated_decoder_cycles",assets.records.iter().map(Record::cpu).max().unwrap().to_string()),("max_written_bytes",assets.records.iter().map(|r|r.count*ROWS).max().unwrap().to_string()),
        ("progressive_wire_format",quoted("Outer LE16 payload length, flagsu8 bit1 reset column cursor0 and bit0 complete at192, column_countu8, count*203 column-major raw bytes for rows26..228; constant black bars remain unchanged. EOF hold payload[0,0]. First9 columns(1827bytes) prefetched to RAM; remaining columns DMA directly REU to front; chunk/command counts count active columns.")),
        ("progressive_tick_cost_model",quoted("Preliminary direct-DMA estimate:1000/record+13*prefetchedbytes+300/prefetchedcolumn+40/tailcolumn; DMA63*(outer2+flags1+count1+rawbody minusprefetchedbytes). Prefix9wholecolumns=1827bytes. Row prefetch outside blank excluded; complete native timing authoritative.")),
        ("audio_sample_rate",audio_hz.to_string()),("audio_samples",(audio.len()/4).to_string()),("audio_bytes",audio.len().to_string()),
        ("audio_gain_measurements_scope",quoted("Original source conversion; PCM reused unchanged, probe takes its prefix")),
        ("source_conversion_metrics_scope",quoted("Colour/quality/author-planning metrics inherited from original full conversion; target pixels reused byte for byte")),
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
    println!("v5 {count}frames/{records}ticks: maxpayload{max_payload}B, model{max_slots}/{}slots, wire{total_wire}B",o.tick_budget);
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
    fn raw_columns_reconstruct_partials_both_regions_black_bars_and_eof() {
        let mut frames = vec![0; FRAME * 3];
        for frame in 1..3 {
            for c in 0..COLUMNS {
                for r in TOP..TOP + ROWS {
                    frames[frame * FRAME + c * 256 + r] = (frame * 73 + c * 31 + r * 7) as u8;
                }
            }
        }
        let assets = encode(&frames).unwrap();
        assert_eq!(assets.records.len(), 15);
        assert_eq!(assets.records.iter().map(|r| r.count).sum::<usize>(), 384);
        assert_eq!(
            assets.records[..5]
                .iter()
                .map(|r| r.count)
                .collect::<Vec<_>>(),
            COUNTS
        );
        let mut ram = vec![0xaa; 65536];
        for &(at, n, a) in &wide::REGIONS {
            ram[a..a + n].copy_from_slice(&frames[at..at + n]);
        }
        let mut cursor = COLUMNS;
        for record in &assets.records {
            assert_eq!(
                compact(&ram),
                assets.oracles[record.index * FRAME..(record.index + 1) * FRAME]
            );
            let p = &assets.stream[record.offset + 2..record.offset + 2 + record.payload()];
            decode(&mut ram, &mut cursor, p).unwrap();
        }
        assert_eq!(compact(&ram), frames[FRAME * 2..]);
        assert!(ram[0xd000..0xe000].iter().all(|&v| v == 0xaa));
        assert!(assets.records[10..]
            .iter()
            .all(|r| r.count == 0 && r.payload() == 2));
        for col in 0..COLUMNS {
            let p = column_address(col);
            assert!(ram[p..p + TOP]
                .iter()
                .chain(&ram[p + TOP + ROWS..p + 256])
                .all(|&v| v == 0));
        }
    }
    #[test]
    fn raw_invalid_flags_counts_completion_and_lengths_do_not_mutate() {
        for p in [
            vec![4, 0],
            vec![2, 0],
            vec![1, 0],
            vec![0, 1],
            vec![0, 0, 1],
            vec![3, 1],
        ] {
            let mut ram = vec![0xaa; 65536];
            let mut col = 191;
            assert!(decode(&mut ram, &mut col, &p).is_err());
            assert_eq!(col, 191);
            assert!(ram.iter().all(|&v| v == 0xaa));
        }
        let mut ram = vec![0xaa; 65536];
        let mut col = 192;
        let mut p = vec![0, 1];
        p.extend([9; ROWS]);
        assert!(decode(&mut ram, &mut col, &p).is_err());
        let mut frames = vec![0; FRAME];
        frames[0] = 1;
        assert!(encode(&frames).is_err());
    }
    #[test]
    fn raw_cli_is_independent_of_lz4_and_defaults_to_192k_video_area() {
        let args = |extra: &[&str]| {
            ["--wide-raw-source", "sample", "--output", "new"]
                .into_iter()
                .chain(extra.iter().copied())
                .map(str::to_string)
                .collect::<Vec<_>>()
                .into_iter()
        };
        let o = parse_options_args(args(&[])).unwrap().unwrap();
        assert_eq!(o.audio_offset, 196608);
        assert_eq!(o.frame_ticks, Some(5));
        assert!(parse_options_args(args(&["--lz4-coalesce"])).is_err());
        assert!(parse_options_args(args(&["--frame-ticks", "8"])).is_err());
    }
}
