//! Isolated v6 PAL/4 trial: transmit only active columns that differ exactly.
//! Hardware colours/author plans are reused; neither lossy threshold nor retime.
use super::wide_lz4::{json_fields, link_or_write, scalar};
use super::*;

const FRAME: usize = wide::FRAME_BYTES;
const COLUMNS: usize = 192;
const ROWS: usize = 203;
const TOP: usize = 26;
const TICKS: usize = 4;
const PREFIX_COLUMNS: usize = 9;
fn address(column: usize) -> usize {
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
fn active_column(frame: &[u8], column: usize) -> &[u8] {
    &frame[column * 256 + TOP..column * 256 + TOP + ROWS]
}
fn changed_columns(before: &[u8], after: &[u8]) -> Vec<u8> {
    (0..COLUMNS)
        .filter(|&c| active_column(before, c) != active_column(after, c))
        .map(|c| c as u8)
        .collect()
}

fn decode(ram: &mut [u8], last_column: &mut Option<usize>, payload: &[u8]) -> Result<bool, String> {
    if ram.len() != 65536 || payload.len() < 2 {
        return Err("Invalid v6 RAM or header".into());
    }
    let flags = payload[0];
    let count = payload[1] as usize;
    if flags & !3 != 0 || payload.len() != 2 + count * (ROWS + 1) {
        return Err("Invalid v6 flags or sparse body length".into());
    }
    let mut last = if flags & 2 != 0 { None } else { *last_column };
    let indices = &payload[2..2 + count];
    for &index in indices {
        let index = index as usize;
        if index >= COLUMNS || last.is_some_and(|p| index <= p) {
            return Err("v6 indices must be ascending unique framebuffer columns".into());
        }
        last = Some(index);
    }
    // Completion of a sparse set cannot be inferred from192: the independent
    // frame oracle checks that omitted columns already equal the target image.
    for (&index, bytes) in indices.iter().zip(payload[2 + count..].chunks_exact(ROWS)) {
        let a = address(index as usize) + TOP;
        ram[a..a + ROWS].copy_from_slice(bytes);
    }
    *last_column = last;
    Ok(flags & 1 != 0)
}
struct Record {
    index: usize,
    offset: usize,
    source: usize,
    prepared: Option<usize>,
    ids: Vec<u8>,
    reset: bool,
    complete: bool,
}
impl Record {
    fn count(&self) -> usize {
        self.ids.len()
    }
    fn prefix_columns(&self) -> usize {
        if self.count() >= PREFIX_COLUMNS {
            PREFIX_COLUMNS
        } else {
            0
        }
    }
    fn prefix(&self) -> usize {
        self.prefix_columns() * ROWS
    }
    fn payload(&self) -> usize {
        2 + self.count() * (ROWS + 1)
    }
    fn cpu(&self) -> usize {
        1000 + 13 * self.prefix()
            + 300 * self.prefix_columns()
            + 60 * (self.count() - self.prefix_columns())
    }
    fn slots(&self) -> usize {
        self.cpu() + 63 * (self.payload() + 2 - self.prefix())
    }
    fn json(&self, total: usize) -> String {
        format!("{{\"index\":{},\"from_phase\":{},\"to_phase\":{},\"offset\":{},\"payload_offset\":{},\"payload_bytes\":{},\"record_bytes\":{},\"source_content\":{},\"prepared_content\":{},\"swap\":{},\"completion\":{},\"reset\":{},\"column_indices\":{:?},\"raw_column_count\":{},\"active_raw_bytes\":{},\"written_bytes\":{},\"copy_bytes\":{},\"copy_commands\":{},\"command_count\":{},\"runs\":{},\"chunk_count\":{},\"max_chunk_bytes\":{},\"prefetched_columns\":{},\"prefetched_body_bytes\":{},\"wire_dma_bytes\":{},\"blank_dma_bytes\":{},\"estimated_decoder_cycles\":{},\"estimated_tick_cpu_slots\":{}}}",self.index,self.index,(self.index+1).min(total-1),self.offset,self.offset+2,self.payload(),self.payload()+2,self.source,self.prepared.map_or_else(||"null".into(),|v|v.to_string()),self.complete,self.complete,self.reset,self.ids,self.count(),self.count()*ROWS,self.count()*ROWS,self.count()*ROWS,self.count(),self.count(),self.count(),self.count(),if self.count()>0 {ROWS} else {0},self.prefix_columns(),self.prefix(),self.payload()+2,self.payload()+2-self.prefix(),self.cpu(),self.slots())
    }
}
struct Assets {
    stream: Vec<u8>,
    oracles: Vec<u8>,
    records: Vec<Record>,
    changed: Vec<usize>,
}
fn encode(frames: &[u8]) -> Result<Assets, String> {
    if frames.is_empty() || !frames.len().is_multiple_of(FRAME) {
        return Err("Invalid v6 logical frames".into());
    }
    let count = frames.len() / FRAME;
    for frame in frames.chunks_exact(FRAME) {
        for col in frame.chunks_exact(256) {
            if col[..TOP].iter().chain(&col[TOP + ROWS..]).any(|&v| v != 0) {
                return Err("v6 requires constant black bars".into());
            }
        }
    }
    let mut ram = vec![0xaa; 65536];
    for &(at, n, a) in &wide::REGIONS {
        ram[a..a + n].copy_from_slice(&frames[at..at + n]);
    }
    let mut last = None;
    let mut assets = Assets {
        stream: Vec::new(),
        oracles: Vec::with_capacity(count * TICKS * FRAME),
        records: Vec::new(),
        changed: Vec::new(),
    };
    for content in 0..count {
        let next = (content + 1).min(count - 1);
        let before = &frames[content * FRAME..(content + 1) * FRAME];
        let target = &frames[next * FRAME..(next + 1) * FRAME];
        let ids = if content + 1 < count {
            changed_columns(before, target)
        } else {
            Vec::new()
        };
        if content + 1 < count {
            assets.changed.push(ids.len());
        }
        let mut at = 0;
        for tick in 0..TICKS {
            let n = ids.len() / TICKS + usize::from(tick >= TICKS - ids.len() % TICKS);
            let indices = ids[at..at + n].to_vec();
            at += n;
            let reset = content + 1 < count && tick == 0;
            let complete = content + 1 < count && tick + 1 == TICKS;
            let mut payload = vec![u8::from(reset) * 2 + u8::from(complete), n as u8];
            payload.extend(&indices);
            for &id in &indices {
                payload.extend(active_column(target, id as usize));
            }
            assets.oracles.extend(compact(&ram));
            if assets.stream.len() % 65536 + payload.len() + 2 > 65534 {
                assets
                    .stream
                    .resize((assets.stream.len() / 65536 + 1) * 65536, 0);
            }
            if decode(&mut ram, &mut last, &payload)? != complete {
                return Err("v6 completion flag mismatch".into());
            }
            if tick + 1 == TICKS && compact(&ram) != target {
                return Err(format!("v6 exact reconstruction failed content{content}"));
            }
            let record = Record {
                index: assets.records.len(),
                offset: assets.stream.len(),
                source: content,
                prepared: (content + 1 < count).then_some(next),
                ids: indices,
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
        return Err("v6 altered I/O gap".into());
    }
    Ok(assets)
}
pub fn run(o: &Options) -> Result<(), String> {
    let source =
        fs::canonicalize(o.wide_skip_source.as_ref().unwrap()).map_err(|e| e.to_string())?;
    let mut fields =
        json_fields(&fs::read_to_string(source.join("manifest.json")).map_err(|e| e.to_string())?)?;
    if scalar::<usize>(&fields, "width")? != 384
        || scalar::<usize>(&fields, "height")? != 256
        || scalar::<usize>(&fields, "frame_ticks")? != TICKS
        || scalar::<usize>(&fields, "active_width")? != 384
        || scalar::<usize>(&fields, "active_height")? != ROWS
        || scalar::<usize>(&fields, "active_top")? != TOP
        || scalar::<usize>(&fields, "active_left")? != 0
        || (scalar::<f64>(&fields, "source_fps")? / (pal_tick_hz() / TICKS as f64) - 1.).abs()
            > 1e-6
    {
        return Err("v6 source must be genuinely sampled PAL/4,384x256/active384x203 top26".into());
    }
    let all = scalar::<usize>(&fields, "source_frames")?;
    let count = o.source_frame_limit.unwrap_or(all);
    if count == 0 || count > all {
        return Err("v6 frame limit exceeds source frames".into());
    }
    let frame_path = source.join("oracle.frames");
    let mut frames = fs::read(&frame_path).map_err(|e| e.to_string())?;
    if frames.len() != all * FRAME {
        return Err("v6 source logical oracle size mismatch".into());
    }
    frames.truncate(count * FRAME);
    if fs::read(source.join("initial.bin")).map_err(|e| e.to_string())? != frames[..FRAME] {
        return Err("v6 initial/oracle mismatch".into());
    }
    let audio_hz = scalar::<u32>(&fields, "audio_sample_rate")?;
    let mut audio = fs::read(source.join("audio.pcm")).map_err(|e| e.to_string())?;
    if !audio.len().is_multiple_of(4) {
        return Err("v6 source PCM alignment".into());
    }
    let duration = count as f64 * TICKS as f64 / pal_tick_hz();
    if count != all {
        audio.truncate((duration * audio_hz as f64).round() as usize * 4);
    }
    let assets = encode(&frames)?;
    fs::create_dir(&o.output)
        .map_err(|e| format!("Create new v6 output{}: {e}", o.output.display()))?;
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
    let totalwire = assets
        .records
        .iter()
        .map(|r| r.payload() + 2)
        .sum::<usize>();
    let maxslots = assets.records.iter().map(Record::slots).max().unwrap();
    let maxpacket = assets.records.iter().map(Record::payload).max().unwrap();
    fields.remove("records");
    let updates=vec![
        ("format",quoted("MUpic progressive wide sparse v6")),("span_version","6".into()),("exact_unchanged_column_skip","true".into()),
        ("source_asset_directory",quoted(&source.to_string_lossy())),("source_conversion_frames",all.to_string()),("source_frames",count.to_string()),
        ("logical_oracle_frames",count.to_string()),("duration_seconds",duration.to_string()),("phases",records.to_string()),("display_ticks",records.to_string()),
        ("phase_order",quoted("Exactly changed active columns, ascending indices, balanced across four PAL ticks; final tick completes even if count0; EOF holds last image")),
        ("stream_bytes",assets.stream.len().to_string()),("stream_blocks",(assets.stream.len()/65536).to_string()),("record_bytes_total",totalwire.to_string()),
        ("video_record_bytes_per_second",(totalwire as f64/duration).to_string()),("max_packet_bytes",maxpacket.to_string()),("max_tick_payload_bytes",maxpacket.to_string()),
        ("max_chunk_bytes",if assets.records.iter().any(|r|r.count()>0){ROWS}else{0}.to_string()),("buffered_chunk_capacity",ROWS.to_string()),
        ("chunk_count_total",assets.records.iter().map(Record::count).sum::<usize>().to_string()),("completion_count",(count-1).to_string()),
        ("changed_columns_per_content",format!("{:?}",assets.changed)),("unchanged_columns_min",(COLUMNS-assets.changed.iter().max().copied().unwrap_or(0)).to_string()),
        ("raw_active_frame_bytes",(COLUMNS*ROWS).to_string()),("raw_active_column_bytes",ROWS.to_string()),("raw_active_column_count",COLUMNS.to_string()),
        ("raw_columns_prefetched_per_tick",PREFIX_COLUMNS.to_string()),("row_prefetch_max_bytes",(PREFIX_COLUMNS*ROWS).to_string()),("row_prefetch_bytes_per_line","9".into()),
        ("estimated_tick_cpu_slots_max",maxslots.to_string()),("tick_budget_cpu_slots",o.tick_budget.to_string()),("fits_tick_budget",(maxslots<=o.tick_budget).to_string()),
        ("max_estimated_decoder_cycles",assets.records.iter().map(Record::cpu).max().unwrap().to_string()),("max_written_bytes",assets.records.iter().map(|r|r.count()*ROWS).max().unwrap().to_string()),
        ("progressive_wire_format",quoted("OuterLE16payloadlen; flags bit1 reset per-content diagnostics,bit0 logical completion;countu8;ascending unique column indices[count]0..191;data203*count bytes rows26..228. Complete count0 valid. EOF[0,0]. Prefix contains only data: first9changedcolumns1827bytes ifcount>=9,else0;remainingcolumns directlyDMA to front. Chunk/command counts count transmittedcolumns. No byte thresholds/no quality changes.")),
        ("progressive_tick_cost_model",quoted("Preliminary sparse directDMA estimate:1000/record+13*prefetchedbytes+300/prefetchedcolumn+60/tailcolumn;DMA63*(outer2+flags1+count1+indices+rawbody−prefetcheddata). Prefix onlyDATA,1827 iffcount>=9. Native timing authoritative; visible-row prefetch work outside blank excluded.")),
        ("audio_sample_rate",audio_hz.to_string()),("audio_samples",(audio.len()/4).to_string()),("audio_bytes",audio.len().to_string()),
        ("source_conversion_metrics_scope",quoted("Original genuine PAL/4 source conversion; hardware pixels and PCM are reused byte for byte")),
        ("source_rgb_reference",quoted(&source.join("source.rgb").to_string_lossy())),("mixed_rgb_reference",quoted(&source.join("mixed.rgb").to_string_lossy())),
        ("native_cache_ticks","20".into()),("reference_roundtrip_verified","true".into()),
        ("records",format!("[\n{}\n]",assets.records.iter().map(|r|r.json(records)).collect::<Vec<_>>().join(",\n"))),
    ];
    for (k, v) in updates {
        fields.insert(k.into(), v);
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
    println!("v6 {count}genuine PAL4frames/{records}ticks,maxpacket{maxpacket},model{maxslots}/{}slots,changedcolumns{}",o.tick_budget,assets.records.iter().map(Record::count).sum::<usize>());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omission_is_exact_and_sparse_replay_preserves_unchanged_columns_and_bars() {
        let mut frames = vec![0; FRAME * 4];
        for frame in 1..4 {
            frames[frame * FRAME..(frame + 1) * FRAME].fill(0);
            for (c, v) in [(0, 3), (7, 5), (8, 9), (80, 12), (191, 2)] {
                for y in TOP..TOP + ROWS {
                    frames[frame * FRAME + c * 256 + y] = v + frame as u8;
                }
            }
        }
        // Last content duplicates its predecessor: reset/complete with count0.
        let last = frames[2 * FRAME..3 * FRAME].to_vec();
        frames[3 * FRAME..].copy_from_slice(&last);
        let assets = encode(&frames).unwrap();
        assert_eq!(assets.records.len(), 16);
        assert_eq!(assets.changed, vec![5, 5, 0]);
        assert_eq!(assets.records.iter().filter(|r| r.complete).count(), 3);
        assert!(assets.records[8..].iter().all(|r| r.ids.is_empty()));
        assert!(assets.records[8].reset);
        assert!(assets.records[11].complete);
        let mut ram = vec![0xaa; 65536];
        for &(at, n, a) in &wide::REGIONS {
            ram[a..a + n].copy_from_slice(&frames[at..at + n]);
        }
        let mut last = None;
        for r in &assets.records {
            assert_eq!(
                compact(&ram),
                assets.oracles[r.index * FRAME..(r.index + 1) * FRAME]
            );
            decode(
                &mut ram,
                &mut last,
                &assets.stream[r.offset + 2..r.offset + 2 + r.payload()],
            )
            .unwrap();
        }
        assert_eq!(compact(&ram), frames[3 * FRAME..]);
        assert!(ram[0xd000..0xe000].iter().all(|&b| b == 0xaa));
    }
    #[test]
    fn sparse_bad_indices_lengths_and_flags_are_transactional() {
        for (flags, ids) in [
            (4, vec![]),
            (0, vec![192]),
            (0, vec![9, 9]),
            (0, vec![8, 7]),
        ] {
            let mut payload = vec![flags, ids.len() as u8];
            payload.extend(&ids);
            payload.extend(vec![9; ids.len() * ROWS]);
            let mut ram = vec![0xaa; 65536];
            let mut last = None;
            assert!(decode(&mut ram, &mut last, &payload).is_err());
            assert!(ram.iter().all(|&b| b == 0xaa));
            assert_eq!(last, None);
        }
        let mut ram = vec![0xaa; 65536];
        let mut last = Some(9);
        let mut p = vec![0, 1, 8];
        p.extend([7; ROWS]);
        assert!(decode(&mut ram, &mut last, &p).is_err());
        assert_eq!(last, Some(9));
        assert!(decode(&mut ram, &mut last, &[0, 1, 10]).is_err());
        assert!(decode(&mut ram, &mut last, &[1, 0]).unwrap());
        assert_eq!(last, Some(9));
        decode(&mut ram, &mut last, &[2, 0]).unwrap();
        assert_eq!(last, None);
    }
    #[test]
    fn skip_cli_requires_true_pal4_and_model_prefetch_only_starts_at_nine() {
        let args = |extra: &[&str]| {
            ["--wide-skip-source", "sample", "--output", "new"]
                .into_iter()
                .chain(extra.iter().copied())
                .map(str::to_string)
                .collect::<Vec<_>>()
                .into_iter()
        };
        let o = parse_options_args(args(&[])).unwrap().unwrap();
        assert_eq!(o.frame_ticks, Some(4));
        assert_eq!(o.audio_offset, 196608);
        assert!(parse_options_args(args(&["--frame-ticks", "5"])).is_err());
        assert!(parse_options_args(args(&["--fps", "10.024908"])).is_err());
        let mut r = Record {
            index: 0,
            offset: 0,
            source: 0,
            prepared: Some(1),
            ids: (0..8).collect(),
            reset: true,
            complete: false,
        };
        assert_eq!(r.prefix(), 0);
        r.ids.push(8);
        assert_eq!(r.prefix(), 1827);
    }
}
