//! Isolated standard UPIC384x256 front-page transport. Adjacent content deltas
//! are applied progressively during N blank intervals; bit0 marks completion,
//! not an atomic framebuffer swap. Conversion is identical author RGB planning.
use super::*;

pub const WIDTH: usize = 384;
pub const HEIGHT: usize = 256;
pub const FRAME_BYTES: usize = WIDTH / 2 * HEIGHT;
pub const REGIONS: [(usize, usize, usize); 2] = [(0, 2048, 0xe000), (2048, 47104, 0x1800)];

fn pack(pixels: &[u8]) -> Result<Vec<u8>, String> {
    if pixels.len() != WIDTH * HEIGHT || pixels.iter().any(|&p| p > 15) {
        return Err("Wide packing requires384x256 hardware indices0..15".into());
    }
    let mut result = Vec::with_capacity(FRAME_BYTES);
    for column in 0..WIDTH / 2 {
        for y in 0..HEIGHT {
            let at = y * WIDTH + column * 2;
            result.push(pixels[at] | pixels[at + 1] << 4);
        }
    }
    Ok(result)
}

fn ram_frame(ram: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(FRAME_BYTES);
    for &(_, length, address) in &REGIONS {
        frame.extend_from_slice(&ram[address..address + length]);
    }
    frame
}

fn decode(ram: &mut [u8], payload: &[u8]) -> Result<bool, String> {
    if ram.len() != 65536 {
        return Err("Wide reference requires64KiB RAM".into());
    }
    let flags = *payload.first().ok_or("Missing wide completion flags")?;
    if flags & !1 != 0 {
        return Err("Unknown wide flags".into());
    }
    let mut at = 1;
    let mut commands = Vec::new();
    loop {
        let len = payload
            .get(at..at + 2)
            .ok_or("Truncated wide chunk length")?;
        at += 2;
        let len = usize::from(u16::from_le_bytes([len[0], len[1]]));
        if len == 0 {
            if at != payload.len() {
                return Err("Trailing wide payload".into());
            }
            break;
        }
        if !(4..=buffered::CHUNK_CAPACITY).contains(&len) {
            return Err("Invalid wide chunk length".into());
        }
        let chunk = payload.get(at..at + len).ok_or("Truncated wide chunk")?;
        at += len;
        let mut cursor = 0;
        loop {
            let header = chunk
                .get(cursor..cursor + 4)
                .ok_or("Missing wide terminator")?;
            cursor += 4;
            let count = usize::from(header[2]);
            if count == 0 {
                if header != [0; 4] || cursor != chunk.len() {
                    return Err("Invalid wide terminator".into());
                }
                break;
            }
            let address = usize::from(u16::from_le_bytes([header[0], header[1]])) + 1;
            if !REGIONS
                .iter()
                .any(|&(_, length, base)| address >= base && address + count <= base + length)
            {
                return Err("Wide command outside framebuffer or crossing I/O gap".into());
            }
            let tag = header[3];
            let bytes = match tag {
                0 => count,
                1 => 1,
                2 => 2,
                _ => return Err("Unsupported wide tag".into()),
            };
            let data = chunk
                .get(cursor..cursor + bytes)
                .ok_or("Truncated wide command")?;
            cursor += bytes;
            commands.push((address, count, tag, data));
        }
    }
    // Fail without modifying RAM if any later chunk is malformed.
    for (address, count, tag, data) in commands {
        for i in 0..count {
            ram[address + i] = data[match tag {
                0 => i,
                1 => 0,
                2 => i & 1,
                _ => unreachable!(),
            }];
        }
    }
    Ok(flags & 1 != 0)
}

struct Assets {
    stream: Vec<u8>,
    oracles: Vec<u8>,
    records: Vec<buffered::RecordStats>,
    max_slots: usize,
    max_payload: usize,
    total_record_bytes: usize,
}

fn encode(frames: &[u8], ticks: usize) -> Result<Assets, String> {
    if frames.is_empty() || !frames.len().is_multiple_of(FRAME_BYTES) || ticks == 0 {
        return Err("Invalid wide logical frames or ticks".into());
    }
    let contents = frames.len() / FRAME_BYTES;
    let mut ram = vec![0u8; 65536];
    for &(start, length, address) in &REGIONS {
        ram[address..address + length].copy_from_slice(&frames[start..start + length]);
    }
    let mut assets = Assets {
        stream: Vec::new(),
        oracles: Vec::with_capacity(contents * ticks * FRAME_BYTES),
        records: Vec::new(),
        max_slots: 0,
        max_payload: 0,
        total_record_bytes: 0,
    };
    for content in 0..contents {
        let before = &frames[content * FRAME_BYTES..(content + 1) * FRAME_BYTES];
        let next = (content + 1).min(contents - 1);
        let after = &frames[next * FRAME_BYTES..(next + 1) * FRAME_BYTES];
        let commands = buffered::commands_for_regions(before, after, &REGIONS);
        let partitions = buffered::balanced_partition(&commands, ticks);
        for (tick, partition) in partitions.into_iter().enumerate() {
            // Oracle is the actually displayed front before this tick's update.
            assets.oracles.extend(ram_frame(&ram));
            let complete = content + 1 < contents && tick + 1 == ticks;
            let (payload, mut stats) = buffered::packet(&commands[partition], complete);
            if payload.len() > u16::MAX as usize {
                return Err("Wide outer payload exceedsu16".into());
            }
            if assets.stream.len() % 65536 + payload.len() + 2 > 65534 {
                assets
                    .stream
                    .resize((assets.stream.len() / 65536 + 1) * 65536, 0);
            }
            stats.index = assets.records.len();
            stats.offset = assets.stream.len();
            stats.target_slot = 0;
            stats.source_content = content;
            stats.prepared_content = (content + 1 < contents).then_some(next);
            stats.back_contents_before = content;
            let decoded_complete = decode(&mut ram, &payload)?;
            if decoded_complete != complete {
                return Err("Wide completion flag mismatch".into());
            }
            if tick + 1 == ticks && ram_frame(&ram) != after {
                return Err(format!("Wide reference mismatch at content{content}"));
            }
            assets.max_slots = assets.max_slots.max(stats.tick_slots);
            assets.max_payload = assets.max_payload.max(payload.len());
            assets.total_record_bytes += payload.len() + 2;
            assets
                .stream
                .extend_from_slice(&(payload.len() as u16).to_le_bytes());
            assets.stream.extend(payload);
            assets.records.push(stats);
        }
    }
    assets
        .stream
        .resize((assets.stream.len() / 65536 + 1) * 65536, 0);
    Ok(assets)
}

pub fn run(o: &Options) -> Result<(), String> {
    let palette = read_palette(&o.palette)?;
    let g = fitted_geometry_canvas(
        probe_aspect(&o.input)?,
        o.pixel_aspect,
        &o.align,
        1,
        WIDTH,
        HEIGHT,
    )?;
    let count_f = (o.duration * o.fps).round();
    if !(1.0..=10000.0).contains(&count_f) {
        return Err("Clip must contain1..10000 frames".into());
    }
    let count = count_f as usize;
    let ticks = ticks_per_content_frame(o);
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
    println!(
        "Decoding {count} wide source frames at{}fps, {}x{} active in384x256",
        o.fps, g.width, g.height
    );
    let decoded = ffmpeg(&args, &o.input)?;
    let source_bytes = g.width * g.height * 3;
    if decoded.len() != count * source_bytes {
        return Err("Wide decoded frame count mismatch".into());
    }
    let mut audio_args = common;
    audio_args.extend([
        "-t".into(),
        duration.to_string(),
        "-vn".into(),
        "-ac".into(),
        "2".into(),
        "-ar".into(),
        o.audio_rate.to_string(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-f".into(),
        "s16le".into(),
    ]);
    let mut audio = ffmpeg(&audio_args, &o.input)?;
    if audio
        .len()
        .abs_diff((duration * o.audio_rate as f64).round() as usize * 4)
        > 4
    {
        return Err("Wide audio duration mismatch".into());
    }
    let gain = apply_audio_gain(&mut audio, o.audio_gain_db)?;
    let threads = o
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));
    let mut author = AuthorDither::new(&palette, o.neutral_guard)?;
    author.set_threads(threads);
    println!("Preplanning all distinct wide RGB with{threads}author workers");
    author.preplan_rgb(&decoded);
    println!("Wide RGB planning complete; converting and scheduling adjacent deltas");
    let mut frames = Vec::with_capacity(count * FRAME_BYTES);
    let mut source = Vec::with_capacity(count * WIDTH * HEIGHT * 3);
    let mut mixed = Vec::with_capacity(count * WIDTH * HEIGHT * 3);
    let mut error = 0.;
    let mut mixed_pixels = 0;
    for frame in decoded.chunks_exact(source_bytes) {
        let plans = author.frame_plans(frame);
        let mut pixels = vec![0; WIDTH * HEIGHT];
        let mut src = vec![0; WIDTH * HEIGHT * 3];
        let mut rgb = vec![0; WIDTH * HEIGHT * 3];
        for y in 0..g.height {
            for x in 0..g.width {
                let i = y * g.width + x;
                let pos = (y + g.top) * WIDTH + x + g.left;
                let plan = plans[i];
                let colour = author.colour(plan, x + g.left, y + g.top);
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
        source.extend(src);
        mixed.extend(rgb);
    }
    let assets = encode(&frames, ticks)?;
    fs::create_dir_all(&o.output).map_err(|e| e.to_string())?;
    write(&o.output.join("initial.bin"), &frames[..FRAME_BYTES])?;
    write(&o.output.join("oracle.frames"), &frames)?;
    write(&o.output.join("oracle.phases"), &assets.oracles)?;
    write(&o.output.join("stream.bin"), &assets.stream)?;
    write(&o.output.join("source.rgb"), source)?;
    write(&o.output.join("mixed.rgb"), mixed)?;
    write(&o.output.join("audio.pcm"), &audio)?;
    let maxchunk = assets
        .records
        .iter()
        .map(|r| r.max_chunk_bytes)
        .max()
        .unwrap_or(0);
    let cycles = assets
        .records
        .iter()
        .map(|r| r.decoder_cycles)
        .max()
        .unwrap_or(0);
    let nrecords = assets.records.len();
    let palettejson = palette
        .iter()
        .map(|p| format!("{p:?}"))
        .collect::<Vec<_>>()
        .join(",");
    let fields=vec![
        ("format",quoted("MUpic progressive wide v3")),("span_version","3".into()),("progressive","true".into()),("buffered","false".into()),("upic_width","384".into()),("width",WIDTH.to_string()),("height",HEIGHT.to_string()),
        ("input",quoted(&o.input.to_string_lossy())),("start_seconds",o.start.to_string()),("duration_seconds",duration.to_string()),("source_fps",o.fps.to_string()),("source_frames",count.to_string()),("frame_ticks",ticks.to_string()),("ticks_per_content_frame",ticks.to_string()),("phase_fps",display_hz(o).to_string()),("display_fps",display_hz(o).to_string()),("phases",nrecords.to_string()),("display_ticks",nrecords.to_string()),("logical_content_completion_fps",o.fps.to_string()),("phase_order",quoted("Progressive adjacent-frame deltas across N front-buffer ticks; last tick marks completion; EOF holds final image")),("atomic_content_publication","false".into()),("loop_transition","false".into()),
        ("colour_mode",quoted("hardware")),("temporal_mixing","false".into()),("temporal_pixels","0".into()),("palette_rgb",format!("[{palettejson}]")),("colour_pair_count","16".into()),("spatial_dither",quoted(&o.dither)),("neutral_guard",o.neutral_guard.to_string()),("author_dither_adaptation",author.metadata()),("spatial_mix_pixels",mixed_pixels.to_string()),("mean_oklab_error",(error/(count*g.width*g.height)as f64).to_string()),
        ("fit_canvas","true".into()),("align",quoted(&o.align)),("active_width",g.width.to_string()),("active_height",g.height.to_string()),("active_top",g.top.to_string()),("active_left",g.left.to_string()),("display_pixel_aspect_ratio",o.pixel_aspect.to_string()),("original_width",g.source_aspect.as_ref().unwrap().width.to_string()),("original_height",g.source_aspect.as_ref().unwrap().height.to_string()),("original_sample_aspect_ratio",g.source_aspect.as_ref().unwrap().sample_aspect.to_string()),("original_display_aspect_ratio",g.source_aspect.as_ref().unwrap().display_aspect.to_string()),("active_display_aspect_ratio",(g.width as f64*o.pixel_aspect/g.height as f64).to_string()),("aspect_rounding_error_relative",(g.width as f64*o.pixel_aspect/g.height as f64/g.source_aspect.as_ref().unwrap().display_aspect-1.).to_string()),
        ("initial_frame_bytes",FRAME_BYTES.to_string()),("framebuffer_bytes",FRAME_BYTES.to_string()),("logical_oracle_file",quoted("oracle.frames")),("logical_oracle_frames",count.to_string()),("phase_oracle_semantics",quoted("Front framebuffer displayed BEFORE applying each tick record; includes intermediate progressively updated images")),("progressive_memory_map",quoted("Column-major256 rows; columns0..7 atE000..E7FF; columns8..191 at1800..CFFF; D000..DFFF gap excluded")),("framebuffer_regions","[{\"compact_offset\":0,\"length\":2048,\"address\":57344},{\"compact_offset\":2048,\"length\":47104,\"address\":6144}]".into()),
        ("stream_bytes",assets.stream.len().to_string()),("stream_block_bytes","65536".into()),("stream_blocks",(assets.stream.len()/65536).to_string()),("record_bytes_total",assets.total_record_bytes.to_string()),("video_record_bytes_per_second",(assets.total_record_bytes as f64/duration).to_string()),("max_packet_bytes",assets.max_payload.to_string()),("max_tick_payload_bytes",assets.max_payload.to_string()),("max_chunk_bytes",maxchunk.to_string()),("buffered_chunk_capacity",buffered::CHUNK_CAPACITY.to_string()),("chunk_count_total",assets.records.iter().map(|r|r.chunks).sum::<usize>().to_string()),("completion_count",assets.records.iter().filter(|r|r.swap).count().to_string()),("estimated_tick_cpu_slots_max",assets.max_slots.to_string()),("tick_budget_cpu_slots",o.tick_budget.to_string()),("fits_tick_budget",(assets.max_slots<=o.tick_budget).to_string()),("max_estimated_decoder_cycles",cycles.to_string()),("max_written_bytes",assets.records.iter().map(|r|r.writes).max().unwrap_or(0).to_string()),
        ("progressive_wire_format",quoted("OuterLE16 payload length; flags bit0=logical completion, not SWAP; chunksLE16<=1024 with COPY0/FILL1/ALT2 and four zero terminator; finalLE16zero. Absolute destinationminus1/count; update front progressively.")),("progressive_tick_cost_model",quoted("63*all wire bytes including outer length, flags, chunk lengths and terminators +200/command +COPY15/FILL10/ALT12 per output byte +500/chunk; same sequential balanced partition as buffered v2; native timing authoritative")),
        ("audio_encoding",quoted("signed PCM16 stereo little-endian interleaved")),("audio_sample_rate",o.audio_rate.to_string()),("audio_samples",(audio.len()/4).to_string()),("audio_bytes",audio.len().to_string()),("audio_gain_db",o.audio_gain_db.to_string()),("audio_gain",gain.metadata()),("threads",o.threads.map_or_else(||quoted("auto"),|v|v.to_string())),("native_cache_ticks",o.cache_ticks.map_or_else(||"null".into(),|v|v.to_string())),("reference_roundtrip_verified","true".into()),
        ("records",format!("[\n{}\n]",assets.records.iter().map(|r|r.json(nrecords)).collect::<Vec<_>>().join(",\n"))),
    ];
    let manifest = format!(
        "{{\n{}\n}}\n",
        fields
            .into_iter()
            .map(|(k, v)| format!("  {}:{v}", quoted(k)))
            .collect::<Vec<_>>()
            .join(",\n")
    );
    write(&o.output.join("manifest.json"), manifest)?;
    println!("Wide {count}contents/{nrecords}ticks: maxpacket{}B, maxchunk{maxchunk}B, modelmax{} /{}slots, completion{}fps (progressive display)",assets.max_payload,assets.max_slots,o.tick_budget,o.fps);
    if o.native_mux {
        let mux = if let Some(ticks) = o.cache_ticks {
            mux_fixed_tick_blocks(
                &assets.stream,
                &audio,
                nrecords,
                o.audio_divider,
                o.audio_offset,
                ticks,
            )?
        } else {
            mux_cache_blocks(
                &assets.stream,
                &audio,
                nrecords,
                o.audio_divider,
                o.audio_offset,
            )?
        };
        write(&o.output.join("stream.mup"), &mux.file)?;
        write(
            &o.output.join("cache-mux.json"),
            mux.metadata(o.audio_offset, o.audio_divider, o.audio_rate),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_pack_and_map_across_top_pages_and_shadow_gap() {
        let mut pixels = vec![0; WIDTH * HEIGHT];
        for (x, y, v) in [(0, 0, 3), (15, 255, 9), (16, 0, 12), (383, 255, 7)] {
            pixels[y * WIDTH + x] = v;
        }
        let frame = pack(&pixels).unwrap();
        assert_eq!(frame[0], 3);
        assert_eq!(frame[2047], 0x90);
        assert_eq!(frame[2048], 12);
        assert_eq!(frame[FRAME_BYTES - 1], 0x70);
        let mut ram = vec![0xaa; 65536];
        for &(at, n, address) in &REGIONS {
            ram[address..address + n].copy_from_slice(&frame[at..at + n]);
        }
        assert_eq!(ram_frame(&ram), frame);
        assert!(ram[0xd000..0xe000].iter().all(|&p| p == 0xaa));
    }
    #[test]
    fn adjacent_progressive_records_reconstruct_partials_complete_and_hold_eof() {
        let mut frames = vec![0; FRAME_BYTES * 3];
        for i in 0..FRAME_BYTES {
            frames[FRAME_BYTES + i] = (i * 73 + 5) as u8;
            frames[FRAME_BYTES * 2 + i] = (i * 31 + 7) as u8;
        }
        let assets = encode(&frames, 5).unwrap();
        assert_eq!(assets.records.len(), 15);
        assert_eq!(assets.records.iter().filter(|r| r.swap).count(), 2);
        assert!(assets.records[10..].iter().all(|r| r.writes == 0));
        let mut ram = vec![0xaa; 65536];
        for &(_, n, a) in &REGIONS {
            ram[a..a + n].fill(0);
        }
        for r in &assets.records {
            assert_eq!(
                ram_frame(&ram),
                assets.oracles[r.index * FRAME_BYTES..(r.index + 1) * FRAME_BYTES]
            );
            let p = &assets.stream[r.offset + 2..r.offset + 2 + r.payload_bytes];
            let done = decode(&mut ram, p).unwrap();
            assert_eq!(done, r.swap);
            if done {
                let next = r.source_content + 1;
                assert_eq!(
                    ram_frame(&ram),
                    frames[next * FRAME_BYTES..(next + 1) * FRAME_BYTES]
                );
            }
        }
        assert!(ram[0xd000..0xe000].iter().all(|&p| p == 0xaa));
        assert_eq!(ram_frame(&ram), frames[FRAME_BYTES * 2..]);
        assert!(assets.records.iter().all(|r| r.max_chunk_bytes <= 1024));
    }
    #[test]
    fn wide_rejects_bad_flags_lengths_count_tags_and_gaps_without_mutation() {
        let mut ram = vec![0xaa; 65536];
        for bad in [
            vec![],
            vec![2, 0, 0],
            vec![0, 4, 0, 0, 0, 0],
            vec![0, 8, 0, 0xff, 0xcf, 1, 1, 9, 0, 0, 0, 0, 0, 0],
            vec![0, 8, 0, 0xff, 0xdf, 1, 7, 9, 0, 0, 0, 0, 0, 0],
            vec![0, 0, 0, 1],
        ] {
            let before = ram.clone();
            assert!(decode(&mut ram, &bad).is_err());
            assert_eq!(before, ram);
        }
    }
    #[test]
    fn wide_fit_has_correct_aspect_and_bar_anchors() {
        for (align, top) in [("start", 0), ("center", 26), ("end", 53)] {
            let g = fitted_geometry_canvas(
                InputAspect {
                    width: 1280,
                    height: 720,
                    sample_aspect: 1.,
                    display_aspect: 16. / 9.,
                },
                15. / 16.,
                align,
                1,
                WIDTH,
                HEIGHT,
            )
            .unwrap();
            assert_eq!((g.width, g.height, g.left, g.top), (384, 203, 0, top));
        }
    }
}
