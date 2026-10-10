//! Bounded-memory v8 conversion. Only RGB batches, one reconstructed transition,
//! one raw record group and one 256 KiB cache block live in memory. Metadata is
//! spooled incrementally; full frame/phase oracles are SHA256 digests instead.
use super::*;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::process::{Child, ChildStdout};

const RGB_BATCH: usize = 8;

struct DecodePipe {
    child: Child,
    stdout: ChildStdout,
}
impl DecodePipe {
    fn spawn(args: &[String]) -> Result<Self, String> {
        let mut child = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-threads", "1"])
            .args(args)
            .arg("pipe:1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // Never buffer unbounded stderr or risk a pipe filling during RGB reads.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Cannot start streaming FFmpeg: {e}"))?;
        let stdout = child.stdout.take().ok_or("Missing FFmpeg stdout")?;
        Ok(Self { child, stdout })
    }
    fn finish(&mut self) -> Result<(), String> {
        let mut trailing = [0];
        if self.stdout.read(&mut trailing).map_err(|e| e.to_string())? != 0 {
            return Err(
                "FFmpeg emitted more data than the requested exact sample/frame count".into(),
            );
        }
        if !self.child.wait().map_err(|e| e.to_string())?.success() {
            return Err("Streaming FFmpeg failed (see stderr diagnostics)".into());
        }
        Ok(())
    }
}
impl Drop for DecodePipe {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct JsonList {
    writer: BufWriter<File>,
    path: PathBuf,
    count: usize,
}
impl JsonList {
    fn new(path: PathBuf) -> Result<Self, String> {
        Ok(Self {
            writer: BufWriter::new(File::create(&path).map_err(|e| e.to_string())?),
            path,
            count: 0,
        })
    }
    fn push(&mut self, value: &str) -> Result<(), String> {
        if self.count != 0 {
            self.writer.write_all(b",\n").map_err(|e| e.to_string())?;
        }
        self.writer
            .write_all(value.as_bytes())
            .map_err(|e| e.to_string())?;
        self.count += 1;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), String> {
        self.writer.flush().map_err(|e| e.to_string())
    }
    fn copy_to(&mut self, output: &mut impl Write) -> Result<(), String> {
        self.flush()?;
        io::copy(
            &mut File::open(&self.path).map_err(|e| e.to_string())?,
            output,
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}
fn write_object(
    path: &Path,
    fields: &BTreeMap<String, String>,
    lists: &mut [(&str, &mut JsonList)],
) -> Result<(), String> {
    let mut output = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
    output.write_all(b"{\n").map_err(|e| e.to_string())?;
    let mut first = true;
    for (name, value) in fields {
        if !first {
            output.write_all(b",\n").map_err(|e| e.to_string())?;
        }
        first = false;
        write!(output, "  {}:{value}", quoted(name)).map_err(|e| e.to_string())?;
    }
    for (name, list) in lists {
        if !first {
            output.write_all(b",\n").map_err(|e| e.to_string())?;
        }
        first = false;
        writeln!(output, "  {}:[", quoted(name)).map_err(|e| e.to_string())?;
        list.copy_to(&mut output)?;
        output.write_all(b"\n]").map_err(|e| e.to_string())?;
    }
    output.write_all(b"\n}\n").map_err(|e| e.to_string())?;
    output.flush().map_err(|e| e.to_string())
}

#[derive(Default)]
struct RateStats {
    pixels: usize,
    adapted: usize,
    changed: usize,
    worst_percent: f64,
    sse: f64,
    oklab: f64,
    skips: usize,
    peak: usize,
}
impl RateStats {
    fn observe(&mut self, report: &str, index: usize, rows: &mut JsonList) -> Result<(), String> {
        let fields = json_fields(report)?;
        self.adapted += scalar::<usize>(&fields, "adapted_frames")?;
        self.changed += scalar::<usize>(&fields, "total_changed_pixels")?;
        self.worst_percent = self
            .worst_percent
            .max(scalar::<f64>(&fields, "worst_frame_changed_pixel_percent")?);
        // A two-frame projection report includes its exact initial frame in
        // the denominator. Recover sums before normalizing the complete clip.
        self.sse += scalar::<f64>(&fields, "rgb_mse")? * (self.pixels * 2 * 3) as f64;
        self.oklab += scalar::<f64>(&fields, "mean_oklab_delta")? * (self.pixels * 2) as f64;
        self.skips += scalar::<usize>(&fields, "temporal_skip_words")?;
        self.peak = self.peak.max(scalar::<usize>(
            &fields,
            "maximum_estimated_tick_cpu_slots",
        )?);
        let array = fields
            .get("frames")
            .ok_or("Missing rate-control frame metrics")?;
        let row = array
            .trim()
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .ok_or("Invalid rate-control frame metrics")?;
        rows.push(&row.replacen("\"frame\":1,", &format!("\"frame\":{index},"), 1))
    }
    fn fields(&self, count: usize, budget: usize) -> BTreeMap<String, String> {
        let denominator = (count * self.pixels) as f64;
        let psnr = if self.sse == 0.0 {
            "null".into()
        } else {
            (10.0 * (255.0_f64.powi(2) / (self.sse / (denominator * 3.0))).log10()).to_string()
        };
        [
            ("algorithm", quoted("DICT4 host-only palette-word projection or temporal SKIP, ordered by per-pixel squared OKLab error; exact record-cost callback")),
            ("lossy", (self.changed > 0).to_string()),
            ("reference", quoted("current exact author conversion, including accumulated temporal error")),
            ("neutral_guard", quoted("each reference grayscale pixel stays grayscale")),
            ("black_padding_preserved", "true".into()), ("whole_frame_holds_allowed", "false".into()),
            ("source_frames", count.to_string()), ("adapted_frames", self.adapted.to_string()),
            ("tick_budget_cpu_slots", budget.to_string()), ("maximum_estimated_tick_cpu_slots", self.peak.to_string()),
            ("total_changed_pixels", self.changed.to_string()), ("changed_pixel_percent", (self.changed as f64 * 100.0 / denominator).to_string()),
            ("worst_frame_changed_pixel_percent", self.worst_percent.to_string()), ("rgb_mse", (self.sse / (denominator * 3.0)).to_string()),
            ("rgb_psnr_db", psnr), ("mean_oklab_delta", (self.oklab / denominator).to_string()), ("temporal_skip_words", self.skips.to_string()),
        ].into_iter().map(|(k,v)| (k.into(),v)).collect()
    }
}

struct RawWriter {
    file: BufWriter<File>,
    bytes: usize,
}
impl RawWriter {
    fn new(path: &Path) -> Result<Self, String> {
        Ok(Self {
            file: BufWriter::new(File::create(path).map_err(|e| e.to_string())?),
            bytes: 0,
        })
    }
    fn pad(&mut self, size: usize) -> Result<(), String> {
        let zeros = [0; 65536];
        if size > zeros.len() {
            return Err("Raw padding overflow".into());
        }
        self.file
            .write_all(&zeros[..size])
            .map_err(|e| e.to_string())?;
        self.bytes += size;
        Ok(())
    }
    fn packet(&mut self, payload: &[u8]) -> Result<usize, String> {
        if payload.len() > 65532 {
            return Err("v8 raw packet too large".into());
        }
        if self.bytes % 65536 + payload.len() + 2 > 65534 {
            self.pad(65536 - self.bytes % 65536)?;
        }
        let offset = self.bytes;
        self.file
            .write_all(&(payload.len() as u16).to_le_bytes())
            .map_err(|e| e.to_string())?;
        self.file.write_all(payload).map_err(|e| e.to_string())?;
        self.bytes += payload.len() + 2;
        Ok(offset)
    }
    fn finish(&mut self) -> Result<(), String> {
        if !self.bytes.is_multiple_of(65536) {
            self.pad(65536 - self.bytes % 65536)?;
        }
        self.file.flush().map_err(|e| e.to_string())
    }
}
struct MuxWriter {
    file: BufWriter<File>,
    pcm: File,
    block: Vec<u8>,
    cursor: usize,
    pending: usize,
    total: usize,
    audio_offset: usize,
    divider: u16,
    ticks: usize,
    meta: CacheMux,
    record_offsets: JsonList,
}
impl MuxWriter {
    fn new(o: &Options) -> Result<Self, String> {
        Ok(Self {
            file: BufWriter::new(
                File::create(o.output.join("stream.mup")).map_err(|e| e.to_string())?,
            ),
            pcm: File::open(o.output.join("audio.pcm")).map_err(|e| e.to_string())?,
            block: vec![0; FIXED_CACHE_BLOCK_BYTES],
            cursor: 0,
            pending: 0,
            total: 0,
            audio_offset: o.audio_offset,
            divider: o.audio_divider,
            ticks: o.cache_ticks.unwrap(),
            meta: CacheMux {
                file: Vec::new(),
                block_records: Vec::new(),
                audio_lengths: Vec::new(),
                record_offsets: Vec::new(),
                useful_video_bytes: 0,
                padded_audio_bytes: 0,
                block_bytes: FIXED_CACHE_BLOCK_BYTES,
                fixed_ticks_per_block: o.cache_ticks,
            },
            record_offsets: JsonList::new(o.output.join(".mux-offsets.tmp"))?,
        })
    }
    fn packet(&mut self, payload: &[u8]) -> Result<(), String> {
        if self.cursor + payload.len() + 4 > self.audio_offset {
            return Err(format!(
                "Cache block {} video area overflow",
                self.meta.block_records.len()
            ));
        }
        let offset = self.meta.block_records.len() * FIXED_CACHE_BLOCK_BYTES + self.cursor;
        self.record_offsets.push(&offset.to_string())?;
        self.block[self.cursor..self.cursor + 2]
            .copy_from_slice(&(payload.len() as u16).to_le_bytes());
        self.cursor += 2;
        self.block[self.cursor..self.cursor + payload.len()].copy_from_slice(payload);
        self.cursor += payload.len();
        self.pending += 1;
        self.total += 1;
        if self.pending == self.ticks {
            self.flush_block()?;
        }
        Ok(())
    }
    fn flush_block(&mut self) -> Result<(), String> {
        if self.pending == 0 {
            return Ok(());
        }
        if self.meta.block_records.len() >= 65535 {
            return Err("Streaming v8 supports at most65535 cache blocks".into());
        }
        let start = sample_at_tick(self.total - self.pending, self.divider) * 4;
        let end = sample_at_tick(self.total, self.divider) * 4;
        let bytes = end - start;
        if bytes > FIXED_CACHE_BLOCK_BYTES - self.audio_offset {
            return Err("Cache block PCM area overflow".into());
        }
        let audio = &mut self.block[self.audio_offset..self.audio_offset + bytes];
        let mut available = 0;
        while available < bytes {
            let read = self
                .pcm
                .read(&mut audio[available..])
                .map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            available += read;
        }
        self.meta.padded_audio_bytes += bytes - available;
        self.meta.useful_video_bytes += self.cursor;
        self.meta.block_records.push(self.pending);
        self.meta.audio_lengths.push(bytes);
        self.file
            .write_all(&self.block)
            .map_err(|e| e.to_string())?;
        self.block.fill(0);
        self.cursor = 0;
        self.pending = 0;
        Ok(())
    }
    fn finish(&mut self, o: &Options) -> Result<(), String> {
        self.flush_block()?;
        self.file.flush().map_err(|e| e.to_string())?;
        // Reuse established metadata fields without allocating the media file
        // or every record offset; replace their two length-derived fields.
        let file_bytes = self.meta.block_records.len() * FIXED_CACHE_BLOCK_BYTES;
        let mut fields = json_fields(&self.meta.metadata_file_bytes(
            o.audio_offset,
            o.audio_divider,
            o.audio_rate,
            file_bytes,
        ))?;
        fields.remove("record_offsets");
        let useful_audio = self.meta.audio_lengths.iter().sum::<usize>();
        fields.insert("file_bytes".into(), file_bytes.to_string());
        fields.insert(
            "padding_bytes".into(),
            (file_bytes - self.meta.useful_video_bytes - useful_audio).to_string(),
        );
        fields.insert(
            "duration_seconds".into(),
            (self.total as f64 / pal_tick_hz()).to_string(),
        );
        write_object(
            &o.output.join("cache-mux.json"),
            &fields,
            &mut [("record_offsets", &mut self.record_offsets)],
        )
    }
}

#[derive(Default)]
struct RecordStats {
    count: usize,
    wire: usize,
    max_payload: usize,
    slots: usize,
    decoder: usize,
    max_written: usize,
    chunks: usize,
    max_chunk: usize,
    raw: usize,
    dictionary: usize,
}
impl RecordStats {
    fn observe(&mut self, record: &Record) {
        self.count += 1;
        self.wire += record.payload() + 2;
        self.max_payload = self.max_payload.max(record.payload());
        self.slots = self.slots.max(record.cost.slots);
        self.decoder = self.decoder.max(record.cost.cpu);
        self.max_written = self
            .max_written
            .max(record.columns.iter().map(Column::writes).sum());
        self.chunks += record.columns.len();
        self.max_chunk = self.max_chunk.max(
            record
                .columns
                .iter()
                .map(Column::body_len)
                .max()
                .unwrap_or(0),
        );
        self.raw += record.columns.iter().filter(|c| c.raw).count();
        self.dictionary += record.columns.iter().filter(|c| !c.raw).count();
    }
}

struct RecordSink {
    raw: RawWriter,
    mux: Option<MuxWriter>,
    records: JsonList,
    phases: JsonList,
    stats: RecordStats,
    total: usize,
    codec: Codec,
}
impl RecordSink {
    fn new(o: &Options, total: usize) -> Result<Self, String> {
        Ok(Self {
            raw: RawWriter::new(&o.output.join("stream.bin"))?,
            mux: if o.native_mux {
                Some(MuxWriter::new(o)?)
            } else {
                None
            },
            records: JsonList::new(o.output.join(".records.tmp"))?,
            phases: JsonList::new(o.output.join(".phase-hashes.tmp"))?,
            stats: RecordStats::default(),
            total,
            codec: if o.dict4_full_height {
                Codec::FULL_HEIGHT
            } else {
                Codec::LEGACY
            },
        })
    }
    fn group(&mut self, frames: &[u8], source: usize, o: &Options) -> Result<(), String> {
        let mut assets = encode_with_codec(frames, o.dict4_prefetch, self.codec)?;
        // For two frames only the first three records are a transition. For
        // one frame they are the three EOF holds. Preserve the legacy order.
        for mut record in assets.records.drain(..TICKS) {
            let local_index = record.index;
            let payload = &assets.stream[record.offset + 2..record.offset + 2 + record.payload()];
            self.phases.push(&quoted(&sha256(
                &assets.oracles[local_index * FRAME..(local_index + 1) * FRAME],
            )))?;
            record.index = self.stats.count;
            record.source = source;
            record.prepared = (frames.len() == FRAME * 2).then_some(source + 1);
            record.offset = self.raw.packet(payload)?;
            if let Some(mux) = &mut self.mux {
                mux.packet(payload)?;
            }
            if o.dict4_rate_budget
                .is_some_and(|budget| record.cost.slots > budget)
            {
                return Err(format!(
                    "Streaming record {} exceeds requested budget",
                    record.index
                ));
            }
            self.records.push(&record.json(self.total))?;
            self.stats.observe(&record);
        }
        Ok(())
    }
    fn finish(&mut self, o: &Options) -> Result<(), String> {
        if self.stats.count != self.total {
            return Err("Streaming total record count mismatch".into());
        }
        self.raw.finish()?;
        if let Some(mux) = &mut self.mux {
            mux.finish(o)?;
        }
        Ok(())
    }
}

fn audio(o: &Options, duration: f64) -> Result<AudioGain, String> {
    let samples = (duration * o.audio_rate as f64).round() as usize;
    let args = vec![
        "-ss".into(),
        o.start.to_string(),
        "-i".into(),
        o.input.to_string_lossy().into_owned(),
        "-vn".into(),
        "-af".into(),
        format!("aresample={},apad,atrim=end_sample={samples}", o.audio_rate),
        "-ac".into(),
        "2".into(),
        "-ar".into(),
        o.audio_rate.to_string(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-f".into(),
        "s16le".into(),
    ];
    let mut pipe = DecodePipe::spawn(&args)?;
    let mut file =
        BufWriter::new(File::create(o.output.join("audio.pcm")).map_err(|e| e.to_string())?);
    let mut stats = AudioGain {
        multiplier: audio_gain_multiplier(o.audio_gain_db)?,
        frames: 0,
        before: AudioLevels::default(),
        after: AudioLevels::default(),
        clipped_samples: [0; 2],
    };
    let mut buffer = vec![0; 65536];
    let mut remaining = samples * 4;
    while remaining != 0 {
        let count = remaining.min(buffer.len());
        let bytes = &mut buffer[..count];
        pipe.stdout
            .read_exact(bytes)
            .map_err(|e| format!("Truncated streaming PCM: {e}"))?;
        let block = apply_audio_gain(bytes, o.audio_gain_db)?;
        stats.frames += block.frames;
        for channel in 0..2 {
            stats.before.peak[channel] = stats.before.peak[channel].max(block.before.peak[channel]);
            stats.after.peak[channel] = stats.after.peak[channel].max(block.after.peak[channel]);
            stats.before.full_scale_samples[channel] += block.before.full_scale_samples[channel];
            stats.after.full_scale_samples[channel] += block.after.full_scale_samples[channel];
            stats.before.sum_squared[channel] += block.before.sum_squared[channel];
            stats.after.sum_squared[channel] += block.after.sum_squared[channel];
            stats.clipped_samples[channel] += block.clipped_samples[channel];
        }
        file.write_all(bytes).map_err(|e| e.to_string())?;
        remaining -= count;
    }
    file.flush().map_err(|e| e.to_string())?;
    pipe.finish()?;
    Ok(stats)
}

fn frame_count(duration: f64, fps: f64) -> Result<usize, String> {
    let count = (duration * fps).round();
    // MUP9 has room for wider future counts, but the current generic player
    // consumes a u16 total-record count. Reject before extracting any media.
    let max_frames = usize::from(u16::MAX) / TICKS;
    if !count.is_finite() || count < 1.0 || count > max_frames as f64 {
        return Err(format!("Streaming v8 current player supports1..{max_frames} content frames (at most65535 PAL tick records); requested {count}"));
    }
    Ok(count as usize)
}

fn video_duration(input: &Path) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(input)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

pub(super) fn run(o: &Options) -> Result<(), String> {
    if o.output.exists() {
        return Err("Streaming v8 output must be a new directory".into());
    }
    let palette = read_palette(&o.palette)?;
    let geometry = fitted_geometry_canvas(
        probe_aspect(&o.input)?,
        o.pixel_aspect,
        &o.align,
        1,
        360,
        200,
    )?;
    let codec = if o.dict4_full_height {
        Codec::FULL_HEIGHT
    } else {
        Codec::LEGACY
    };
    if o.dict4_full_height {
        if (geometry.width, geometry.height, geometry.left, geometry.top) != (356, 200, 2, 0) {
            return Err("Native SD codec9 requires centered356x200 content, two black edge pixels, and pixel-aspect1".into());
        }
    } else if (geometry.width, geometry.height, geometry.left, geometry.top) != (360, 190, 0, 5) {
        return Err("Streaming v8 requires active360x190 centered in360x200".into());
    }
    let rgb_frame = geometry.width * geometry.height * 3;
    let source_pixels = geometry.width * geometry.height;
    let count = frame_count(o.duration, o.fps)?;
    let total = count * TICKS;
    let duration = count as f64 / o.fps;
    let source_video_duration = video_duration(&o.input);
    fs::create_dir_all(&o.output).map_err(|e| e.to_string())?;
    println!("Streaming codec{} {count} PAL/3 frames ({duration:.3}s), active{}x{}@{},{} RGB batch{RGB_BATCH}, compact SHA256 oracles", codec.version, geometry.width, geometry.height, geometry.left, geometry.top);
    let gain = audio(o, duration)?;
    let threads = o
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));
    let mut author = AuthorDither::new(&palette, o.neutral_guard)?;
    author.set_threads(threads);
    let metric = dict4_rate::Prepared::with_geometry(&palette, codec.top, codec.rows, codec.active);
    let args = vec![
        "-ss".into(),
        o.start.to_string(),
        "-i".into(),
        o.input.to_string_lossy().into_owned(),
        "-an".into(),
        "-vf".into(),
        format!(
            "tpad=stop_mode=clone:stop_duration={duration},fps={},scale={}:{}:flags=lanczos,setsar=1,format=rgb24",
            o.fps, geometry.width, geometry.height,
        ),
        "-frames:v".into(),
        count.to_string(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgb24".into(),
    ];
    let mut pipe = DecodePipe::spawn(&args)?;
    let mut sink = RecordSink::new(o, total)?;
    let mut frames = JsonList::new(o.output.join(".frame-hashes.tmp"))?;
    let mut exact_frames = JsonList::new(o.output.join(".author-hashes.tmp"))?;
    let mut rate_rows = JsonList::new(o.output.join(".rate-rows.tmp"))?;
    let mut rate = RateStats {
        pixels: 360 * codec.active,
        ..RateStats::default()
    };
    let mut batch = vec![0; rgb_frame * RGB_BATCH];
    let mut previous = Vec::new();
    let mut pair = Vec::with_capacity(FRAME * 2);
    let mut index = 0;
    let mut mixed = 0usize;
    let mut error = 0.0;
    while index < count {
        let n = RGB_BATCH.min(count - index);
        let bytes = &mut batch[..n * rgb_frame];
        pipe.stdout
            .read_exact(bytes)
            .map_err(|e| format!("Truncated RGB before frame {index}: {e}"))?;
        author.preplan_rgb(bytes);
        for rgb in bytes.chunks_exact(rgb_frame) {
            let plans = author.frame_plans(rgb);
            let mut exact = vec![0; FRAME];
            for y in 0..geometry.height {
                for x in 0..geometry.width {
                    let at = y * geometry.width + x;
                    let canvas_x = x + geometry.left;
                    let canvas_y = y + geometry.top;
                    let plan = plans[at];
                    let colour = author.colour(plan, canvas_x, canvas_y);
                    exact[(canvas_x / 2) * 256 + canvas_y] |= colour << ((canvas_x % 2) * 4);
                    mixed += usize::from(author.is_mixed(plan));
                    let target = lab([
                        linear(rgb[at * 3]),
                        linear(rgb[at * 3 + 1]),
                        linear(rgb[at * 3 + 2]),
                    ]);
                    let actual = lab(author.mean_linear(plan));
                    error += (0..3)
                        .map(|k| (target[k] - actual[k]).powi(2))
                        .sum::<f64>()
                        .sqrt();
                }
            }
            exact_frames.push(&quoted(&sha256(&exact)))?;
            let after = if index == 0 {
                write(&o.output.join("initial.bin"), &exact)?;
                exact
            } else {
                pair.clear();
                pair.extend_from_slice(&previous);
                pair.extend_from_slice(&exact);
                let after = if let Some(budget) = o.dict4_rate_budget {
                    let (projected, report) = metric
                        .convert(&pair, budget, |a, b| {
                            peak_cost_codec(a, b, o.dict4_prefetch, codec)
                        })
                        .map_err(|e| format!("Streaming source frame {index}: {e}"))?;
                    rate.observe(&report, index, &mut rate_rows)?;
                    projected[FRAME..].to_vec()
                } else {
                    exact
                };
                pair.truncate(FRAME);
                pair.extend_from_slice(&after);
                sink.group(&pair, index - 1, o)?;
                after
            };
            frames.push(&quoted(&sha256(&after)))?;
            previous = after;
            index += 1;
        }
        eprintln!(
            "Streaming DICT4: {index}/{count} frames, {} adapted, peak {}/{}, author cache ready",
            rate.adapted, sink.stats.slots, o.tick_budget
        );
    }
    pipe.finish()?;
    sink.group(&previous, count - 1, o)?;
    sink.finish(o)?;
    let s = &sink.stats;
    let aspect = geometry.source_aspect.as_ref().unwrap();
    let entries=vec![
        ("format",quoted(&format!("MUpic progressive360 word dictionary v{}", codec.version))),("span_version",codec.version.to_string()),("dictionary4","true".into()),("progressive","true".into()),("buffered","false".into()),("raw_active_columns","false".into()),
        ("upic_width","360".into()),("width","360".into()),("height","200".into()),("storage_height","256".into()),("input",quoted(&o.input.to_string_lossy())),("start_seconds",o.start.to_string()),("source_fps",o.fps.to_string()),("source_frames",count.to_string()),("source_conversion_frames",count.to_string()),("logical_oracle_frames",count.to_string()),("fresh_source_conversion","true".into()),("source_asset_directory","null".into()),
        ("frame_ticks","3".into()),("ticks_per_content_frame","3".into()),("phase_fps",pal_tick_hz().to_string()),("display_fps",pal_tick_hz().to_string()),("logical_content_completion_fps",o.fps.to_string()),("atomic_content_publication","false".into()),("loop_transition","false".into()),("duration_seconds",duration.to_string()),("phases",total.to_string()),("display_ticks",total.to_string()),
        ("colour_mode",quoted("hardware")),("temporal_mixing","false".into()),("temporal_pixels","0".into()),("palette_rgb",format!("{:?}",palette)),("colour_pair_count","16".into()),("spatial_dither",quoted(&o.dither)),("neutral_guard",o.neutral_guard.to_string()),("author_dither_adaptation",author.metadata()),("spatial_mix_pixels",mixed.to_string()),("pre_rate_control_mean_oklab_error",(error/(count*source_pixels)as f64).to_string()),
        ("fit_canvas","true".into()),("align",quoted(&o.align)),("active_width",geometry.width.to_string()),("active_height",geometry.height.to_string()),("active_top",geometry.top.to_string()),("active_left",geometry.left.to_string()),("display_pixel_aspect_ratio",o.pixel_aspect.to_string()),("original_width",aspect.width.to_string()),("original_height",aspect.height.to_string()),("original_sample_aspect_ratio",aspect.sample_aspect.to_string()),("original_display_aspect_ratio",aspect.display_aspect.to_string()),("active_display_aspect_ratio",(geometry.width as f64*o.pixel_aspect/geometry.height as f64).to_string()),("aspect_rounding_error_relative",(geometry.width as f64*o.pixel_aspect/geometry.height as f64/aspect.display_aspect-1.0).to_string()),
        ("initial_frame_bytes",FRAME.to_string()),("framebuffer_bytes",FRAME.to_string()),("logical_oracle_file","null".into()),("compact_oracle_file",quoted("oracle-hashes.json")),("phase_oracle_semantics",quoted("SHA256 of front framebuffer BEFORE applying each tick record; intermediate progressive images")),("progressive_memory_map",quoted("180 page-stride columns, each256 rows:0..7 E000..E7FF;8..179 1800..C3FF; logical canvas360x200 padded zero through row255")),("framebuffer_regions","[{\"compact_offset\":0,\"length\":2048,\"address\":57344},{\"compact_offset\":2048,\"length\":44032,\"address\":6144}]".into()),("stream_block_bytes","65536".into()),
        ("stream_bytes",sink.raw.bytes.to_string()),("stream_blocks",(sink.raw.bytes/65536).to_string()),("record_bytes_total",s.wire.to_string()),("video_record_bytes_per_second",(s.wire as f64/duration).to_string()),("max_packet_bytes",s.max_payload.to_string()),("max_tick_payload_bytes",s.max_payload.to_string()),("max_chunk_bytes",s.max_chunk.to_string()),("buffered_chunk_capacity",(codec.words()*6).to_string()),("chunk_count_total",s.chunks.to_string()),("completion_count",(count-1).to_string()),("dictionary_loads_total",(count-1).to_string()),("dictionary_raw_columns_total",s.raw.to_string()),("dictionary_columns_total",s.dictionary.to_string()),
        ("dictionary_entries","254".into()),("dictionary_entry_bytes","4".into()),("dictionary_word_rows",codec.rows.to_string()),("dictionary_planar_bytes","1024".into()),("dictionary_address","50688".into()),("dictionary_stage_address","56320".into()),("dictionary_stage_capacity",(codec.words()*6).to_string()),("raw_literal_column_bytes",codec.rows.to_string()),("row_prefetch_max_bytes",(o.dict4_prefetch*codec.active).to_string()),("row_prefetch_bytes_per_line",o.dict4_prefetch.to_string()),("row_prefetch_semantics",quoted(&format!("Body only after flags/count; P={}*floor(min(bodybytes,{})/{}). Player must use the same bytes per row.",o.dict4_prefetch,o.dict4_prefetch*codec.active,o.dict4_prefetch))),
        ("estimated_tick_cpu_slots_max",s.slots.to_string()),("tick_budget_cpu_slots",o.tick_budget.to_string()),("fits_tick_budget",(s.slots<=o.tick_budget).to_string()),("max_estimated_decoder_cycles",s.decoder.to_string()),("max_written_bytes",s.max_written.to_string()),("reference_roundtrip_verified","true".into()),("native_cache_ticks",o.cache_ticks.unwrap().to_string()),
        ("audio_encoding",quoted("signed PCM16 stereo little-endian interleaved")),("audio_sample_rate",o.audio_rate.to_string()),("audio_gain_db",o.audio_gain_db.to_string()),("audio_gain",gain.metadata()),("audio_samples",gain.frames.to_string()),("audio_bytes",(gain.frames*4).to_string()),("threads",o.threads.map_or_else(||quoted("auto"),|v|v.to_string())),
        ("lossy_rate_control",o.dict4_rate_budget.is_some().to_string()),("rate_control_file",quoted("rate-control.json")),("reference_author_frames","null".into()),("streaming_encoding","true".into()),("rgb_batch_frames",RGB_BATCH.to_string()),("large_rgb_and_frame_oracles_written","false".into()),
        ("phase_order",quoted("Three progressive PAL ticks per genuine source sample; perceptual word rate control within requested CPU/DMA budget; final tick completes; EOF holds final frame")),("source_conversion_metrics_scope",quoted("Exact author/neutral-guard conversion, then same DICT4 palette-word rate control; compact SHA256 oracles replace RGB/frame files")),("progressive_tick_cost_model",quoted("NMOS dictionary primitive and parser/setup allowances plus63 slots per DMA byte; exact minimax three-record partition; word rate control verified on reconstructed previous frame. Integrated actual6502/native timing remains authoritative.")),
        ("dictionary_frequency_ties",quoted("Changed NEW4byteword descendingfrequency; tiesbyfirstencounterinascendingcolumnthenrow. Slots254/255reservedzero. Unusedentrieszero.")),("progressive_wire_format",quoted(&format!("OuterLE16length; flags bit1reset(always1024planarLUT) andbit0complete,countu8. Eachsparseascendingcolumn:indexu8,escapebytesu8(0..{}multiple5) OR255RAW;DICT{}indices+escaped(rowoffsetu8,4bytes),RAW{}bytes. Index0..253dictionary,254SKIPoldword,255escapedword. Rows{}..{}. Emptycompletion flags1count0 legal;EOF[0,0].", codec.words()*5,codec.words(),codec.rows,codec.top,codec.top+codec.rows-1))),
    ];
    let mut fields: BTreeMap<String, String> =
        entries.into_iter().map(|(k, v)| (k.into(), v)).collect();
    fields.insert("requested_duration_seconds".into(), o.duration.to_string());
    fields.insert(
        "source_video_duration_seconds".into(),
        source_video_duration.map_or_else(|| "null".into(), |v| v.to_string()),
    );
    fields.insert("source_video_tail_padding".into(), quoted("FFmpeg tpad stop_mode=clone before fps; preserve samples before original EOF and hold final RGB to requested frame count when video ends before audio/container. Exact -frames:v bound; maximum pad duration equals quantized requested duration."));
    if let Some(budget) = o.dict4_rate_budget {
        let summary = rate.fields(count, budget);
        write_object(
            &o.output.join("rate-control.json"),
            &summary,
            &mut [("frames", &mut rate_rows)],
        )?;
        // Small summary in manifest; the full per-frame report remains separate.
        fields.insert(
            "rate_control".into(),
            format!(
                "{{{}}}",
                summary
                    .iter()
                    .map(|(k, v)| format!("{}:{v}", quoted(k)))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        );
    }
    write_object(
        &o.output.join("manifest.json"),
        &fields,
        &mut [("records", &mut sink.records)],
    )?;
    let oracle_fields=[("format".into(),quoted("MUpic compact SHA256 oracles v1")),("frame_bytes".into(),FRAME.to_string()),("source_frames".into(),count.to_string()),("phases".into(),total.to_string()),("phase_semantics".into(),quoted("Front framebuffer before each record, same logical page-stride packing as initial.bin"))].into_iter().collect();
    write_object(
        &o.output.join("oracle-hashes.json"),
        &oracle_fields,
        &mut [
            ("logical_frame_sha256", &mut frames),
            ("phase_sha256", &mut sink.phases),
            ("exact_author_frame_sha256", &mut exact_frames),
        ],
    )?;
    // Remove only private spool files generated by this new invocation.
    for name in [
        ".records.tmp",
        ".phase-hashes.tmp",
        ".frame-hashes.tmp",
        ".author-hashes.tmp",
        ".rate-rows.tmp",
        ".mux-offsets.tmp",
    ] {
        let path = o.output.join(name);
        if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    println!("Streaming codec{} complete: {count}frames/{total}ticks, {}wire bytes, modelpeak {}/{}, {} adapted",codec.version,s.wire,s.slots,o.tick_budget,rate.adapted);
    if o.strict_budget && s.max_payload > o.packet_budget {
        return Err("Streaming v8 exceeds packet budget; statistics retained".into());
    }
    Ok(())
}

// SHA256 is confined to compact audit artifacts. No additional dependency and
// no per-frame subprocess; known standard vectors are checked below.
fn sha256(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut data = bytes.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&((bytes.len() as u64) * 8).to_be_bytes());
    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, b) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(b.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_player_tick_limit_is_checked_before_decoding() {
        let fps = pal_tick_hz() / TICKS as f64;
        assert_eq!(frame_count(21845.0 / fps, fps).unwrap(), 21845);
        assert!(frame_count(21846.0 / fps, fps)
            .unwrap_err()
            .contains("65535"));
        assert_eq!(frame_count(634.624580, fps).unwrap(), 10603);
        assert!(frame_count(0.0, fps).is_err());
        assert!(frame_count(f64::INFINITY, fps).is_err());
    }
    #[test]
    fn incremental_mux_crosses_255_blocks_and_preserves_partial_eof() {
        let directory =
            std::env::temp_dir().join(format!("mupic-streaming-mux-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let args = [
            "--input",
            "unused",
            "--output",
            directory.to_str().unwrap(),
            "--wide360-dict",
            "--streaming-dict4",
            "--frame-ticks",
            "3",
            "--colour-mode",
            "hardware",
            "--fit-canvas",
            "--pixel-aspect",
            "15:16",
            "--dither",
            "yliluoma-author",
            "--audio-offset",
            "225280",
        ];
        let o = parse_options_args(args.into_iter().map(String::from))
            .unwrap()
            .unwrap();
        let ticks = 255 * 20 + 1;
        let pcm_bytes = sample_at_tick(ticks, o.audio_divider) * 4;
        let file = File::create(directory.join("audio.pcm")).unwrap();
        file.set_len(pcm_bytes as u64).unwrap();
        let mut mux = MuxWriter::new(&o).unwrap();
        for _ in 0..ticks {
            mux.packet(&[0, 0]).unwrap();
        }
        mux.finish(&o).unwrap();
        assert_eq!(mux.meta.block_records.len(), 256);
        assert_eq!(mux.meta.block_records[255], 1);
        assert_eq!(mux.meta.padded_audio_bytes, 0);
        let fields =
            json_fields(&fs::read_to_string(directory.join("cache-mux.json")).unwrap()).unwrap();
        assert_eq!(
            scalar::<usize>(&fields, "file_bytes").unwrap(),
            256 * FIXED_CACHE_BLOCK_BYTES
        );
        assert_eq!(
            scalar::<usize>(&fields, "useful_audio_bytes").unwrap(),
            pcm_bytes
        );
        assert_eq!(
            fs::metadata(directory.join("stream.mup")).unwrap().len(),
            (256 * FIXED_CACHE_BLOCK_BYTES) as u64
        );
        drop(mux);
        fs::remove_dir_all(directory).unwrap();
        assert!(parse_options_args(["--streaming-dict4"].into_iter().map(String::from)).is_err());
    }
    #[test]
    fn sha256_standard_vectors_cover_padding_and_multiple_blocks() {
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }
    #[test]
    fn prepared_single_transitions_preserve_accumulated_rate_control() {
        let palette = read_palette(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/palette.json"),
        )
        .unwrap();
        let mut source = vec![0; FRAME * 4];
        for i in 1..4 {
            for c in 0..COLS {
                for y in TOP..TOP + 190 {
                    source[i * FRAME + c * 256 + y] = ((c + y + i * 7) % 16) as u8;
                }
            }
        }
        let cost = |a: &[u8], b: &[u8]| peak_cost(a, b, 13);
        let (whole, _) = dict4_rate::convert(&source, &palette, 450000, cost).unwrap();
        let prepared = dict4_rate::Prepared::new(&palette);
        let mut current = source[..FRAME].to_vec();
        let mut output = current.clone();
        for target in source[FRAME..].chunks_exact(FRAME) {
            let pair = [current.as_slice(), target].concat();
            let (converted, _) = prepared.convert(&pair, 450000, cost).unwrap();
            current = converted[FRAME..].to_vec();
            output.extend_from_slice(&current);
        }
        assert_eq!(whole, output);
    }
}
