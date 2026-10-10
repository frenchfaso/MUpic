//! Isolated v7: logical360x200 hardware video in180 page-stride columns.
//! Constant bars remain in front; active190-row columns use direct REU DMA.
//! Twelve columns are prefetched as9 bytes before and3 after each raster row.
use super::wide_lz4::{json_fields, link_or_write, scalar};
use super::*;

const WIDTH: usize = 360;
const HEIGHT: usize = 200;
const FRAME: usize = WIDTH / 2 * 256;
const REGIONS: [(usize, usize, usize); 2] = [(0, 2048, 0xe000), (2048, 44032, 0x1800)];
const COLUMNS: usize = 180;
const TOP: usize = 5;
const ROWS: usize = 190;

const PREFIX_COLUMNS: usize = 12;
const PREFETCH_BYTES: usize = PREFIX_COLUMNS * ROWS;

fn column_address(column: usize) -> usize {
    if column < 8 {
        0xe000 + column * 256
    } else {
        0x1800 + (column - 8) * 256
    }
}
fn compact(ram: &[u8]) -> Vec<u8> {
    REGIONS
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
        return Err("Invalid v7 RAM or payload".into());
    }
    let flags = payload[0];
    let count = payload[1] as usize;
    if flags & !3 != 0 || payload.len() != 2 + count * ROWS {
        return Err("Invalid v7 flags or raw column length".into());
    }
    if count == 0 && flags != 0 {
        return Err("Empty v7 record must be an EOF hold".into());
    }
    let start = if flags & 2 != 0 { 0 } else { *column };
    if start + count > COLUMNS || (flags & 1 != 0 && start + count != COLUMNS) {
        return Err("v7 column bounds or incomplete completion".into());
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
fn encode(frames: &[u8], ticks: usize) -> Result<Assets, String> {
    if frames.is_empty() || !frames.len().is_multiple_of(FRAME) {
        return Err("Invalid v7 logical frames".into());
    }
    if ![3, 4].contains(&ticks) {
        return Err("v7 requires three or four ticks per content".into());
    }
    let contents = frames.len() / FRAME;
    let counts = vec![COLUMNS / ticks; ticks];
    for frame in frames.chunks_exact(FRAME) {
        for col in frame.chunks_exact(256) {
            if col[..TOP].iter().chain(&col[TOP + ROWS..]).any(|&b| b != 0) {
                return Err(
                    "v7 raw active transport requires constant black top/bottom bars".into(),
                );
            }
        }
    }
    let mut ram = vec![0xaa; 65536];
    for &(at, n, a) in &REGIONS {
        ram[a..a + n].copy_from_slice(&frames[at..at + n]);
    }
    let mut column = COLUMNS;
    let mut assets = Assets {
        stream: Vec::new(),
        oracles: Vec::with_capacity(contents * ticks * FRAME),
        records: Vec::new(),
    };
    for content in 0..contents {
        let target = &frames[(content + 1).min(contents - 1) * FRAME
            ..((content + 1).min(contents - 1) + 1) * FRAME];
        for (tick, &tick_count) in counts.iter().enumerate() {
            assets.oracles.extend(compact(&ram));
            let count = if content + 1 < contents {
                tick_count
            } else {
                0
            };
            let reset = content + 1 < contents && tick == 0;
            let complete = content + 1 < contents && tick + 1 == ticks;
            let start = if reset { 0 } else { column };
            let mut payload = vec![u8::from(reset) * 2 + u8::from(complete), count as u8];
            payload.extend(active(target, start, count));
            if assets.stream.len() % 65536 + payload.len() + 2 > 65534 {
                assets
                    .stream
                    .resize((assets.stream.len() / 65536 + 1) * 65536, 0);
            }
            if decode(&mut ram, &mut column, &payload)? != complete {
                return Err("v7 completion mismatch".into());
            }
            if tick + 1 == ticks && compact(&ram) != target {
                return Err(format!("v7 reconstruction mismatch content{content}"));
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
        return Err("v7 reference altered I/O gap".into());
    }
    Ok(assets)
}
pub fn run(o: &Options) -> Result<(), String> {
    if o.output.exists() {
        return Err("v7 output must be a new directory".into());
    }
    let loaded = load(o)?;
    let Loaded {
        mut fields,
        frames,
        audio,
        source,
        all,
        count,
        audio_hz,
        source_rgb,
        mixed_rgb,
    } = loaded;
    let ticks = o.frame_ticks.unwrap();
    let duration = count as f64 * ticks as f64 / pal_tick_hz();
    println!("Reusing{count}/{all} exact frames for v7 raw active-column direct DMA");
    let assets = encode(&frames, ticks)?;
    fs::create_dir(&o.output)
        .map_err(|e| format!("Create new output directory{}: {e}", o.output.display()))?;
    if let Some(source) = &source {
        link_or_write(
            &source.join("initial.bin"),
            &o.output.join("initial.bin"),
            &frames[..FRAME],
            true,
        )?;
        link_or_write(
            &source.join("oracle.frames"),
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
        fields.insert(
            "source_asset_directory".into(),
            quoted(&source.to_string_lossy()),
        );
        fields.insert(
            "source_rgb_reference".into(),
            quoted(&source.join("source.rgb").to_string_lossy()),
        );
        fields.insert(
            "mixed_rgb_reference".into(),
            quoted(&source.join("mixed.rgb").to_string_lossy()),
        );
    } else {
        write(&o.output.join("initial.bin"), &frames[..FRAME])?;
        write(&o.output.join("oracle.frames"), &frames)?;
        write(&o.output.join("audio.pcm"), &audio)?;
        write(&o.output.join("source.rgb"), source_rgb.unwrap())?;
        write(&o.output.join("mixed.rgb"), mixed_rgb.unwrap())?;
    }
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
        ("format",quoted("MUpic progressive360 raw v7")),("span_version","7".into()),("raw_active_columns","true".into()),
        ("source_conversion_frames",all.to_string()),("source_frames",count.to_string()),
        ("logical_oracle_frames",count.to_string()),("duration_seconds",duration.to_string()),("phases",records.to_string()),("display_ticks",records.to_string()),
        ("phase_order",quoted("Adjacent target active columns copied progressively across N PAL ticks; final tick completes; EOF holds final image")),
        ("stream_bytes",assets.stream.len().to_string()),("stream_blocks",(assets.stream.len()/65536).to_string()),("record_bytes_total",total_wire.to_string()),
        ("video_record_bytes_per_second",(total_wire as f64/duration).to_string()),("max_packet_bytes",max_payload.to_string()),("max_tick_payload_bytes",max_payload.to_string()),
        ("max_chunk_bytes",if count>1 {ROWS} else {0}.to_string()),("buffered_chunk_capacity",ROWS.to_string()),("chunk_count_total",assets.records.iter().map(|r|r.count).sum::<usize>().to_string()),
        ("completion_count",(count-1).to_string()),("raw_column_counts_per_content",format!("{:?}",vec![COLUMNS/ticks;ticks])),("raw_active_frame_bytes",(COLUMNS*ROWS).to_string()),
        ("raw_active_column_bytes",ROWS.to_string()),("raw_active_column_count",COLUMNS.to_string()),("raw_columns_prefetched_per_tick",PREFIX_COLUMNS.to_string()),
        ("row_prefetch_max_bytes",PREFETCH_BYTES.to_string()),("row_prefetch_bytes_per_line","12".into()),("row_prefetch_head_bytes","9".into()),("row_prefetch_tail_bytes","3".into()),
        ("estimated_tick_cpu_slots_max",max_slots.to_string()),("tick_budget_cpu_slots",o.tick_budget.to_string()),("fits_tick_budget",(max_slots<=o.tick_budget).to_string()),
        ("max_estimated_decoder_cycles",assets.records.iter().map(Record::cpu).max().unwrap().to_string()),("max_written_bytes",assets.records.iter().map(|r|r.count*ROWS).max().unwrap().to_string()),
        ("progressive_wire_format",quoted("Outer LE16 payload length, flagsu8 bit1 reset column cursor0 and bit0 complete at180, column_countu8, count*190 column-major raw bytes for rows5..194; constant black bars remain unchanged. EOF hold payload[0,0]. First12 columns(2280bytes) prefetched to RAM; remaining columns DMA directly REU to front; chunk/command counts count active columns.")),
        ("progressive_tick_cost_model",quoted("Preliminary direct-DMA estimate:1000/record+13*prefetchedbytes+300/prefetchedcolumn+40/tailcolumn; DMA63*(outer2+flags1+count1+rawbody minusprefetchedbytes). Prefix12wholecolumns=2280bytes (9 before pixels,3 after right black gap per active row). Row prefetch outside blank excluded; complete native timing authoritative.")),
        ("audio_sample_rate",audio_hz.to_string()),("audio_samples",(audio.len()/4).to_string()),("audio_bytes",audio.len().to_string()),
        ("audio_gain_measurements_scope",quoted("Original source conversion; PCM reused unchanged, probe takes its prefix")),
        ("source_conversion_metrics_scope",quoted("Colour/quality/author-planning metrics inherited from original full conversion; target pixels reused byte for byte")),
        ("native_cache_ticks",o.cache_ticks.unwrap().to_string()),("reference_roundtrip_verified","true".into()),
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
    println!("v7 {count}frames/{records}ticks: maxpayload{max_payload}B, model{max_slots}/{}slots, wire{total_wire}B",o.tick_budget);
    if o.native_mux {
        let mux = mux_fixed_tick_blocks(
            &assets.stream,
            &audio,
            records,
            o.audio_divider,
            o.audio_offset,
            o.cache_ticks.unwrap(),
        )?;
        write(&o.output.join("stream.mup"), &mux.file)?;
        write(
            &o.output.join("cache-mux.json"),
            mux.metadata(o.audio_offset, o.audio_divider, audio_hz),
        )?;
    }
    Ok(())
}

pub(super) struct Loaded {
    pub(super) fields: std::collections::BTreeMap<String, String>,
    pub(super) frames: Vec<u8>,
    pub(super) audio: Vec<u8>,
    pub(super) source: Option<PathBuf>,
    pub(super) all: usize,
    pub(super) count: usize,
    pub(super) audio_hz: u32,
    pub(super) source_rgb: Option<Vec<u8>>,
    pub(super) mixed_rgb: Option<Vec<u8>>,
}
fn pack(pixels: &[u8]) -> Result<Vec<u8>, String> {
    if pixels.len() != WIDTH * HEIGHT || pixels.iter().any(|&v| v > 15) {
        return Err("v7 packing requires360x200 hardware indices0..15".into());
    }
    let mut frame = vec![0; FRAME];
    for c in 0..COLUMNS {
        for y in 0..HEIGHT {
            let at = y * WIDTH + c * 2;
            frame[c * 256 + y] = pixels[at] | pixels[at + 1] << 4;
        }
    }
    Ok(frame)
}
fn load(o: &Options) -> Result<Loaded, String> {
    if let Some(path) = &o.wide360_raw_source {
        let source = fs::canonicalize(path).map_err(|e| e.to_string())?;
        let fields = json_fields(
            &fs::read_to_string(source.join("manifest.json")).map_err(|e| e.to_string())?,
        )?;
        let ticks = o.frame_ticks.unwrap();
        for (key, value) in [
            ("width", WIDTH),
            ("height", HEIGHT),
            ("storage_height", 256),
            ("frame_ticks", ticks),
            ("active_width", WIDTH),
            ("active_height", ROWS),
            ("active_top", TOP),
            ("active_left", 0),
            ("framebuffer_bytes", FRAME),
        ] {
            if scalar::<usize>(&fields, key)? != value {
                return Err(format!("v7 source {key} mismatch"));
            }
        }
        if (scalar::<f64>(&fields, "source_fps")? / (pal_tick_hz() / ticks as f64) - 1.).abs()
            > 1e-6
        {
            return Err("v7 requires genuine PAL source cadence; retiming is unsupported".into());
        }
        let all = scalar::<usize>(&fields, "source_frames")?;
        let count = o.source_frame_limit.unwrap_or(all);
        if count == 0 || count > all {
            return Err("v7 frame limit exceeds source frames".into());
        }
        let mut frames = fs::read(source.join("oracle.frames")).map_err(|e| e.to_string())?;
        if frames.len() != all * FRAME {
            return Err("v7 source oracle size mismatch".into());
        }
        frames.truncate(count * FRAME);
        if fs::read(source.join("initial.bin")).map_err(|e| e.to_string())? != frames[..FRAME] {
            return Err("v7 initial/source mismatch".into());
        }
        let audio_hz = scalar::<u32>(&fields, "audio_sample_rate")?;
        let mut audio = fs::read(source.join("audio.pcm")).map_err(|e| e.to_string())?;
        if !audio.len().is_multiple_of(4) {
            return Err("v7 PCM alignment".into());
        }
        if count != all {
            audio.truncate((count as f64 / o.fps * audio_hz as f64).round() as usize * 4);
        }
        return Ok(Loaded {
            fields,
            frames,
            audio,
            source: Some(source),
            all,
            count,
            audio_hz,
            source_rgb: None,
            mixed_rgb: None,
        });
    }
    convert(o)
}
pub(super) fn convert(o: &Options) -> Result<Loaded, String> {
    let palette = read_palette(&o.palette)?;
    let g = fitted_geometry_canvas(
        probe_aspect(&o.input)?,
        o.pixel_aspect,
        &o.align,
        1,
        WIDTH,
        HEIGHT,
    )?;
    if g.width != WIDTH || g.height != ROWS || g.left != 0 || g.top != TOP {
        return Err("v7 first qualification requires active360x190 top5 in360x200; use BBB aspect16:9, pixel-aspect15:16 and aligncenter".into());
    }
    let count_f = (o.duration * o.fps).round();
    if !(1.0..=10000.).contains(&count_f) {
        return Err("v7 clip needs1..10000 content frames".into());
    }
    let count = count_f as usize;
    let ticks = o.frame_ticks.unwrap();
    let duration = count as f64 / o.fps;
    let common = vec![
        "-ss".into(),
        o.start.to_string(),
        "-i".into(),
        o.input.to_string_lossy().into_owned(),
    ];
    let mut args = common.clone();
    args.extend([
        "-an".into(),
        "-vf".into(),
        format!(
            "fps={},scale={}:{}:flags=lanczos,setsar=1,format=rgb24",
            o.fps, g.width, g.height
        ),
        "-frames:v".into(),
        count.to_string(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgb24".into(),
    ]);
    println!("Decoding {count} genuine PAL/{ticks} frames, active360x190 in360x200");
    let decoded = ffmpeg(&args, &o.input)?;
    let source_bytes = WIDTH * ROWS * 3;
    if decoded.len() != count * source_bytes {
        return Err("v7 decoded frame count mismatch".into());
    }
    let mut args = common;
    let audio_samples = (duration * o.audio_rate as f64).round() as usize;
    args.extend([
        "-vn".into(),
        // Bound decoded samples rather than timestamps: Opus priming can shift
        // output timestamps, and a shorter audio track needs a silent tail.
        "-af".into(),
        format!(
            "aresample={},apad,atrim=end_sample={audio_samples}",
            o.audio_rate
        ),
        "-ac".into(),
        "2".into(),
        "-ar".into(),
        o.audio_rate.to_string(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-f".into(),
        "s16le".into(),
    ]);
    let mut audio = ffmpeg(&args, &o.input)?;
    if audio
        .len()
        .abs_diff((duration * o.audio_rate as f64).round() as usize * 4)
        > 4
    {
        return Err(format!(
            "v7 audio duration mismatch: {} bytes vs {} expected ({duration}s)",
            audio.len(),
            audio_samples * 4
        ));
    }
    let gain = apply_audio_gain(&mut audio, o.audio_gain_db)?;
    let threads = o
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));
    let mut author = AuthorDither::new(&palette, o.neutral_guard)?;
    author.set_threads(threads);
    println!("Planning exact author RGB with {threads} workers");
    author.preplan_rgb(&decoded);
    let mut frames = Vec::with_capacity(count * FRAME);
    let mut source_rgb = Vec::with_capacity(count * WIDTH * HEIGHT * 3);
    let mut mixed_rgb = Vec::with_capacity(count * WIDTH * HEIGHT * 3);
    let mut error = 0.;
    let mut mixed_pixels = 0;
    for frame in decoded.chunks_exact(source_bytes) {
        let plans = author.frame_plans(frame);
        let mut pixels = vec![0; WIDTH * HEIGHT];
        let mut src = vec![0; WIDTH * HEIGHT * 3];
        let mut rgb = vec![0; WIDTH * HEIGHT * 3];
        for y in 0..ROWS {
            for x in 0..WIDTH {
                let i = y * WIDTH + x;
                let pos = (y + TOP) * WIDTH + x;
                let plan = plans[i];
                let colour = author.colour(plan, x, y + TOP);
                pixels[pos] = colour;
                mixed_pixels += usize::from(author.is_mixed(plan));
                src[pos * 3..pos * 3 + 3].copy_from_slice(&frame[i * 3..i * 3 + 3]);
                rgb[pos * 3..pos * 3 + 3].copy_from_slice(&palette[colour as usize]);
                let target = lab([
                    linear(frame[i * 3]),
                    linear(frame[i * 3 + 1]),
                    linear(frame[i * 3 + 2]),
                ]);
                let actual = lab(author.mean_linear(plan));
                error += (0..3)
                    .map(|j| (target[j] - actual[j]).powi(2))
                    .sum::<f64>()
                    .sqrt();
            }
        }
        frames.extend(pack(&pixels)?);
        source_rgb.extend(src);
        mixed_rgb.extend(rgb);
    }
    let aspect = g.source_aspect.as_ref().unwrap();
    let entries=vec![
        ("format",quoted("MUpic progressive360 raw v7")),("span_version","7".into()),("progressive","true".into()),("buffered","false".into()),("upic_width",WIDTH.to_string()),("width",WIDTH.to_string()),("height",HEIGHT.to_string()),("storage_height","256".into()),
        ("input",quoted(&o.input.to_string_lossy())),("start_seconds",o.start.to_string()),("source_fps",o.fps.to_string()),("source_frames",count.to_string()),("frame_ticks",ticks.to_string()),("ticks_per_content_frame",ticks.to_string()),("phase_fps",pal_tick_hz().to_string()),("display_fps",pal_tick_hz().to_string()),("logical_content_completion_fps",o.fps.to_string()),("atomic_content_publication","false".into()),("loop_transition","false".into()),
        ("colour_mode",quoted("hardware")),("temporal_mixing","false".into()),("temporal_pixels","0".into()),("palette_rgb",format!("{:?}",palette)),("colour_pair_count","16".into()),("spatial_dither",quoted(&o.dither)),("neutral_guard",o.neutral_guard.to_string()),("author_dither_adaptation",author.metadata()),("spatial_mix_pixels",mixed_pixels.to_string()),("mean_oklab_error",(error/(count*WIDTH*ROWS)as f64).to_string()),
        ("fit_canvas","true".into()),("align",quoted(&o.align)),("active_width",WIDTH.to_string()),("active_height",ROWS.to_string()),("active_top",TOP.to_string()),("active_left","0".into()),("display_pixel_aspect_ratio",o.pixel_aspect.to_string()),("original_width",aspect.width.to_string()),("original_height",aspect.height.to_string()),("original_sample_aspect_ratio",aspect.sample_aspect.to_string()),("original_display_aspect_ratio",aspect.display_aspect.to_string()),("active_display_aspect_ratio",(WIDTH as f64*o.pixel_aspect/ROWS as f64).to_string()),("aspect_rounding_error_relative",(WIDTH as f64*o.pixel_aspect/ROWS as f64/aspect.display_aspect-1.).to_string()),
        ("initial_frame_bytes",FRAME.to_string()),("framebuffer_bytes",FRAME.to_string()),("logical_oracle_file",quoted("oracle.frames")),("phase_oracle_semantics",quoted("Front framebuffer displayed BEFORE applying each tick record; intermediate progressive images")),("progressive_memory_map",quoted("180 page-stride columns, each256 rows:0..7 E000..E7FF;8..179 1800..C3FF; logical canvas360x200 padded zero through row255")),("framebuffer_regions","[{\"compact_offset\":0,\"length\":2048,\"address\":57344},{\"compact_offset\":2048,\"length\":44032,\"address\":6144}]".into()),("stream_block_bytes","65536".into()),
        ("audio_encoding",quoted("signed PCM16 stereo little-endian interleaved")),("audio_sample_rate",o.audio_rate.to_string()),("audio_gain_db",o.audio_gain_db.to_string()),("audio_gain",gain.metadata()),("threads",o.threads.map_or_else(||quoted("auto"),|v|v.to_string())),
    ];
    Ok(Loaded {
        fields: entries.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        frames,
        audio,
        source: None,
        all: count,
        count,
        audio_hz: o.audio_rate,
        source_rgb: Some(source_rgb),
        mixed_rgb: Some(mixed_rgb),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logical_canvas_packs_page_stride_padding_and_both_physical_regions() {
        let mut pixels = vec![0; WIDTH * HEIGHT];
        for (x, y, value) in [(0, 0, 3), (15, 199, 9), (16, 5, 12), (359, 199, 7)] {
            pixels[y * WIDTH + x] = value;
        }
        let frame = pack(&pixels).unwrap();
        assert_eq!(frame.len(), 46080);
        assert_eq!(frame[0], 3);
        assert_eq!(frame[7 * 256 + 199], 0x90);
        assert_eq!(frame[8 * 256 + 5], 12);
        assert_eq!(frame[179 * 256 + 199], 0x70);
        assert!(frame
            .chunks_exact(256)
            .all(|col| col[200..].iter().all(|&v| v == 0)));
        let mut ram = vec![0xaa; 65536];
        for &(at, n, a) in &REGIONS {
            ram[a..a + n].copy_from_slice(&frame[at..at + n]);
        }
        assert_eq!(compact(&ram), frame);
        assert_eq!(column_address(179) + 256, 0xc400);
        assert!(ram[0xc400..0xe000].iter().all(|&v| v == 0xaa));
    }
    #[test]
    fn raw_three_and_four_tick_reconstruction_prefix_black_bars_and_eof() {
        let mut frames = vec![0; FRAME * 3];
        for f in 1..3 {
            for c in 0..COLUMNS {
                for y in TOP..TOP + ROWS {
                    frames[f * FRAME + c * 256 + y] = (f * 73 + c * 31 + y * 7) as u8;
                }
            }
        }
        for ticks in [3, 4] {
            let assets = encode(&frames, ticks).unwrap();
            assert_eq!(assets.records.len(), ticks * 3);
            assert_eq!(assets.records.iter().filter(|r| r.complete).count(), 2);
            assert_eq!(assets.records.iter().map(|r| r.count).sum::<usize>(), 360);
            let mut ram = vec![0xaa; 65536];
            for &(at, n, a) in &REGIONS {
                ram[a..a + n].copy_from_slice(&frames[at..at + n]);
            }
            let mut cursor = COLUMNS;
            for r in &assets.records {
                assert_eq!(
                    compact(&ram),
                    assets.oracles[r.index * FRAME..(r.index + 1) * FRAME]
                );
                let payload = &assets.stream[r.offset + 2..r.offset + 2 + r.payload()];
                assert_eq!(decode(&mut ram, &mut cursor, payload).unwrap(), r.complete);
                if r.count > 0 {
                    assert_eq!(r.count, 180 / ticks);
                    assert_eq!(r.prefix(), 2280);
                    assert_eq!(
                        &payload[2..2 + r.prefix()],
                        active(
                            &frames[(r.source + 1) * FRAME..(r.source + 2) * FRAME],
                            r.start,
                            12
                        )
                    );
                }
                if r.complete {
                    assert_eq!(
                        compact(&ram),
                        frames[(r.source + 1) * FRAME..(r.source + 2) * FRAME]
                    );
                }
            }
            assert!(assets.records[ticks * 2..]
                .iter()
                .all(|r| r.count == 0 && !r.reset && !r.complete));
            assert!(ram[0xc400..0xe000].iter().all(|&v| v == 0xaa));
        }
    }
    #[test]
    fn invalid_wire_and_nonblack_padding_fail_without_mutation() {
        for p in [
            vec![4, 0],
            vec![2, 0],
            vec![1, 0],
            vec![0, 1],
            vec![0, 0, 1],
            vec![3, 1],
        ] {
            let mut ram = vec![0xaa; 65536];
            let mut c = 179;
            assert!(decode(&mut ram, &mut c, &p).is_err());
            assert_eq!(c, 179);
            assert!(ram.iter().all(|&v| v == 0xaa));
        }
        let mut frame = vec![0; FRAME];
        frame[200] = 1;
        assert!(encode(&frame, 4).is_err());
        assert!(encode(&vec![0; FRAME], 5).is_err());
        let r = Record {
            index: 0,
            offset: 0,
            source: 0,
            prepared: Some(1),
            start: 0,
            count: 11,
            reset: true,
            complete: false,
        };
        assert_eq!(r.prefix(), 0);
    }
    #[test]
    fn cli_isolated_cadence_and_cache_defaults_preserve_genuine_source_fps() {
        let args = |ticks: &str| {
            [
                "--input",
                "source",
                "--output",
                "new",
                "--upic-width",
                "360",
                "--colour-mode",
                "hardware",
                "--fit-canvas",
                "--pixel-aspect",
                "15:16",
                "--dither",
                "yliluoma-author",
                "--frame-ticks",
                ticks,
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
        };
        for (ticks, cache) in [(3, 16), (4, 20)] {
            let o = parse_options_args(args(&ticks.to_string()))
                .unwrap()
                .unwrap();
            assert_eq!(o.cache_ticks, Some(cache));
            assert_eq!(o.audio_offset, 196608);
            assert_eq!(o.fps, pal_tick_hz() / ticks as f64);
        }
        assert!(parse_options_args(args("5")).is_err());
        let reuse = |extra: &[&str]| {
            ["--wide360-raw-source", "source", "--output", "new"]
                .into_iter()
                .chain(extra.iter().copied())
                .map(str::to_string)
                .collect::<Vec<_>>()
                .into_iter()
        };
        let o = parse_options_args(reuse(&["--frame-ticks", "3"]))
            .unwrap()
            .unwrap();
        assert_eq!(o.upic_width, 360);
        assert_eq!(o.cache_ticks, Some(16));
        assert!(
            parse_options_args(reuse(&["--frame-ticks", "3", "--fps", "12.531135531"])).is_err()
        );
        assert!(parse_options_args(reuse(&["--frame-ticks", "3", "--cache-ticks", "20"])).is_err());
        let aspect = parse_probe_aspect(
            "width=1280\nheight=720\nsample_aspect_ratio=1:1\ndisplay_aspect_ratio=16:9",
        )
        .unwrap();
        let g = fitted_geometry_canvas(aspect, 15. / 16., "center", 1, 360, 200).unwrap();
        assert_eq!((g.width, g.height, g.left, g.top), (360, 190, 0, 5));
    }
}
