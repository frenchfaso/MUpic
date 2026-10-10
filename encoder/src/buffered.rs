//! Isolated v2 transport: prepare only the back framebuffer, publish on SWAP.

pub const FRAME_BYTES: usize = 160 * 169;
pub const CHUNK_CAPACITY: usize = 1024;
const COMMAND_SETUP: usize = 200;
const CHUNK_SETUP: usize = 500;
const DMA_SLOTS_PER_BYTE: usize = 63;
const TICK_WIRE_OVERHEAD: usize = 5; // outer length, flags and chunklen zero
const CHUNK_WIRE_OVERHEAD: usize = 6; // chunk length and span terminator
const SLOT0: [(usize, usize, usize); 1] = [(0, FRAME_BYTES, 0x1800)];
const SLOT1: [(usize, usize, usize); 2] = [(0, 20064, 0x81a0), (20064, 6976, 0xe000)];

fn regions(slot: usize) -> &'static [(usize, usize, usize)] {
    if slot == 0 {
        &SLOT0
    } else {
        &SLOT1
    }
}

pub fn compact(canvas: &[u8], top: usize) -> Result<Vec<u8>, String> {
    if canvas.len() != super::FRAME_BYTES || top > 31 {
        return Err("Buffered compact frame requires a 320x200 canvas and 169 active rows".into());
    }
    let mut result = Vec::with_capacity(FRAME_BYTES);
    for column in 0..160 {
        for y in top..top + 169 {
            let offset = if y < 192 {
                y / 64 * 10240 + column * 64 + y % 64
            } else {
                30720 + column * 8 + y - 192
            };
            result.push(canvas[offset]);
        }
    }
    Ok(result)
}

#[derive(Clone, Debug)]
pub(crate) struct Command {
    bytes: Vec<u8>,
    count: usize,
    tag: u8,
    changed: usize,
}
impl Command {
    fn cpu_cycles(&self) -> usize {
        COMMAND_SETUP
            + self.count
                * match self.tag {
                    0 => 15,
                    1 => 10,
                    2 => 12,
                    _ => unreachable!(),
                }
    }
    fn slots(&self) -> usize {
        self.cpu_cycles() + self.bytes.len() * DMA_SLOTS_PER_BYTE
    }
}

fn commands(before: &[u8], after: &[u8], slot: usize) -> Vec<Command> {
    commands_for_regions(before, after, regions(slot))
}

pub(crate) fn commands_for_regions(
    before: &[u8],
    after: &[u8],
    memory_regions: &[(usize, usize, usize)],
) -> Vec<Command> {
    let mut result = Vec::new();
    for &(start, length, address) in memory_regions {
        let before = &before[start..start + length];
        let after = &after[start..start + length];
        let mut costs = vec![usize::MAX; length + 1];
        let mut choices = vec![(1usize, 255u8); length];
        costs[length] = 0;
        for at in (0..length).rev() {
            let unchanged = before[at] == after[at];
            let mut best = if unchanged { costs[at + 1] } else { usize::MAX };
            let mut selected = (1, 255);
            if !unchanged {
                let mut consider = |count: usize, tag: u8, wire_bytes: usize, per_byte: usize| {
                    let cost = wire_bytes * DMA_SLOTS_PER_BYTE
                        + COMMAND_SETUP
                        + count * per_byte
                        + costs[at + count];
                    if cost < best {
                        best = cost;
                        selected = (count, tag);
                    }
                };
                for count in 1..=(length - at).min(255) {
                    consider(count, 0, 4 + count, 15);
                }
                for count in 1..=super::pattern_length(after, at, 1) {
                    consider(count, 1, 5, 10);
                }
                for count in 2..=super::pattern_length(after, at, 2) {
                    consider(count, 2, 6, 12);
                }
            }
            costs[at] = best;
            choices[at] = selected;
        }
        let mut at = 0;
        while at < length {
            let (count, tag) = choices[at];
            if tag == 255 {
                at += count;
                continue;
            }
            let mut bytes = Vec::with_capacity(4 + count);
            bytes.extend_from_slice(&((address + at - 1) as u16).to_le_bytes());
            bytes.extend_from_slice(&[count as u8, tag]);
            bytes.extend_from_slice(
                &after[at..at
                    + match tag {
                        0 => count,
                        1 => 1,
                        2 => 2,
                        _ => unreachable!(),
                    }],
            );
            result.push(Command {
                bytes,
                count,
                tag,
                changed: before[at..at + count]
                    .iter()
                    .zip(&after[at..at + count])
                    .filter(|(a, b)| a != b)
                    .count(),
            });
            at += count;
        }
    }
    result
}

fn partition(commands: &[Command], ticks: usize, ceiling: usize) -> Vec<std::ops::Range<usize>> {
    let mut result = Vec::new();
    let mut begin = 0;
    let mut cost = TICK_WIRE_OVERHEAD * DMA_SLOTS_PER_BYTE;
    let mut chunk_bytes = 0;
    for (index, command) in commands.iter().enumerate() {
        let new_chunk = chunk_bytes == 0 || chunk_bytes + command.bytes.len() > CHUNK_CAPACITY - 4;
        let extra = command.slots()
            + usize::from(new_chunk) * (CHUNK_SETUP + CHUNK_WIRE_OVERHEAD * DMA_SLOTS_PER_BYTE);
        if index > begin && cost + extra > ceiling {
            result.push(begin..index);
            if result.len() >= ticks {
                return result;
            }
            begin = index;
            cost = TICK_WIRE_OVERHEAD * DMA_SLOTS_PER_BYTE;
            chunk_bytes = 0;
        }
        let new_chunk = chunk_bytes == 0 || chunk_bytes + command.bytes.len() > CHUNK_CAPACITY - 4;
        cost += command.slots()
            + usize::from(new_chunk) * (CHUNK_SETUP + CHUNK_WIRE_OVERHEAD * DMA_SLOTS_PER_BYTE);
        chunk_bytes = if new_chunk {
            command.bytes.len()
        } else {
            chunk_bytes + command.bytes.len()
        };
    }
    if !commands.is_empty() {
        result.push(begin..commands.len());
    }
    result
}

pub(crate) fn balanced_partition(
    commands: &[Command],
    ticks: usize,
) -> Vec<std::ops::Range<usize>> {
    if commands.is_empty() {
        return vec![0..0; ticks];
    }
    let overhead = TICK_WIRE_OVERHEAD * DMA_SLOTS_PER_BYTE;
    let mut low = overhead
        + commands.iter().map(Command::slots).max().unwrap()
        + CHUNK_SETUP
        + CHUNK_WIRE_OVERHEAD * DMA_SLOTS_PER_BYTE;
    let mut high = overhead
        + commands.iter().map(Command::slots).sum::<usize>()
        + commands.len() * (CHUNK_SETUP + CHUNK_WIRE_OVERHEAD * DMA_SLOTS_PER_BYTE);
    // Sequential next-fit packing, followed by binary search of its maximum
    // tick cost. Commands and their order remain intact; unused ticks hold.
    while low < high {
        let middle = low + (high - low) / 2;
        let candidate = partition(commands, ticks + 1, middle);
        if candidate.len() <= ticks {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    let mut result = partition(commands, ticks + 1, low);
    let end = commands.len();
    result.resize(ticks, end..end);
    result
}

#[derive(Clone, Debug, Default)]
pub struct RecordStats {
    pub index: usize,
    pub offset: usize,
    pub payload_bytes: usize,
    pub chunks: usize,
    pub max_chunk_bytes: usize,
    pub commands: usize,
    pub changed: usize,
    pub writes: usize,
    pub copied: usize,
    pub filled: usize,
    pub alternating: usize,
    pub copy_commands: usize,
    pub fill_commands: usize,
    pub alternating_commands: usize,
    pub decoder_cycles: usize,
    pub tick_slots: usize,
    pub target_slot: usize,
    pub source_content: usize,
    pub prepared_content: Option<usize>,
    pub back_contents_before: usize,
    pub swap: bool,
}
impl RecordStats {
    pub fn json(&self, ticks_total: usize) -> String {
        let Self {
            index,
            offset,
            payload_bytes,
            chunks,
            max_chunk_bytes,
            commands,
            changed,
            writes,
            copied,
            filled,
            alternating,
            copy_commands,
            fill_commands,
            alternating_commands,
            decoder_cycles,
            tick_slots,
            target_slot,
            source_content,
            prepared_content,
            back_contents_before,
            swap,
        } = self;
        let prepared = prepared_content.map_or_else(|| "null".into(), |v| v.to_string());
        format!("    {{\"index\":{index},\"from_phase\":{index},\"to_phase\":{},\"offset\":{offset},\"payload_offset\":{},\"payload_bytes\":{payload_bytes},\"record_bytes\":{},\"chunk_count\":{chunks},\"max_chunk_bytes\":{max_chunk_bytes},\"command_count\":{commands},\"copy_commands\":{copy_commands},\"fill_commands\":{fill_commands},\"alternating_commands\":{alternating_commands},\"runs\":{commands},\"changed_bytes\":{changed},\"written_bytes\":{writes},\"copy_bytes\":{copied},\"fill_bytes\":{filled},\"alternating_bytes\":{alternating},\"estimated_decoder_cycles\":{decoder_cycles},\"wire_dma_bytes\":{},\"estimated_tick_cpu_slots\":{tick_slots},\"target_slot\":{target_slot},\"source_content\":{source_content},\"prepared_content\":{prepared},\"back_contents_before\":{back_contents_before},\"swap\":{swap}}}",(index+1).min(ticks_total-1),offset+2,payload_bytes+2,payload_bytes+2)
    }
}

pub(crate) fn packet(commands: &[Command], swap: bool) -> (Vec<u8>, RecordStats) {
    let mut result = vec![u8::from(swap)];
    let mut chunk = Vec::new();
    let mut stats = RecordStats {
        swap,
        ..Default::default()
    };
    let flush = |result: &mut Vec<u8>, chunk: &mut Vec<u8>, stats: &mut RecordStats| {
        if chunk.is_empty() {
            return;
        }
        chunk.extend_from_slice(&[0; 4]);
        stats.max_chunk_bytes = stats.max_chunk_bytes.max(chunk.len());
        result.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        result.extend_from_slice(chunk);
        chunk.clear();
        stats.chunks += 1;
    };
    for command in commands {
        if chunk.len() + command.bytes.len() > CHUNK_CAPACITY - 4 {
            flush(&mut result, &mut chunk, &mut stats);
        }
        chunk.extend_from_slice(&command.bytes);
        stats.commands += 1;
        stats.changed += command.changed;
        stats.writes += command.count;
        stats.decoder_cycles += command.cpu_cycles();
        match command.tag {
            0 => {
                stats.copied += command.count;
                stats.copy_commands += 1;
            }
            1 => {
                stats.filled += command.count;
                stats.fill_commands += 1;
            }
            2 => {
                stats.alternating += command.count;
                stats.alternating_commands += 1;
            }
            _ => unreachable!(),
        }
    }
    flush(&mut result, &mut chunk, &mut stats);
    result.extend_from_slice(&[0; 2]);
    stats.decoder_cycles += stats.chunks * CHUNK_SETUP;
    stats.payload_bytes = result.len();
    stats.tick_slots = stats.decoder_cycles + (result.len() + 2) * DMA_SLOTS_PER_BYTE;
    (result, stats)
}

struct DecodedCommand {
    address: usize,
    count: usize,
    tag: u8,
    data: Vec<u8>,
}
fn decode_record(ram: &mut [u8], front: &mut usize, payload: &[u8]) -> Result<(), String> {
    if ram.len() != 65536 || *front > 1 {
        return Err("Invalid buffered RAM or front slot".into());
    }
    let flags = *payload.first().ok_or("Missing buffered flags")?;
    if flags & !1 != 0 {
        return Err("Unknown buffered flags".into());
    }
    let mut at = 1;
    let mut decoded = Vec::new();
    loop {
        let length = payload.get(at..at + 2).ok_or("Truncated chunk length")?;
        at += 2;
        let length = usize::from(u16::from_le_bytes([length[0], length[1]]));
        if length == 0 {
            if at != payload.len() {
                return Err("Trailing buffered data".into());
            }
            break;
        }
        if !(4..=CHUNK_CAPACITY).contains(&length) {
            return Err("Invalid span chunk length".into());
        }
        let chunk = payload.get(at..at + length).ok_or("Truncated span chunk")?;
        at += length;
        let mut cursor = 0;
        loop {
            let header = chunk
                .get(cursor..cursor + 4)
                .ok_or("Missing span terminator")?;
            cursor += 4;
            let count = usize::from(header[2]);
            if count == 0 {
                if header != [0, 0, 0, 0] || cursor != chunk.len() {
                    return Err("Invalid span terminator".into());
                }
                break;
            }
            let address = usize::from(u16::from_le_bytes([header[0], header[1]])) + 1;
            if !regions(*front ^ 1)
                .iter()
                .any(|(_, length, base)| address >= *base && address + count <= base + length)
            {
                return Err("Command outside back slot or crossing shadow gap".into());
            }
            let tag = header[3];
            let bytes = match tag {
                0 => count,
                1 => 1,
                2 => 2,
                _ => return Err("Unsupported buffered span tag".into()),
            };
            let data = chunk
                .get(cursor..cursor + bytes)
                .ok_or("Truncated span command")?;
            cursor += bytes;
            decoded.push(DecodedCommand {
                address,
                count,
                tag,
                data: data.to_vec(),
            });
        }
    }
    // Parse every command before changing either framebuffer or publishing.
    for command in decoded {
        for i in 0..command.count {
            ram[command.address + i] = command.data[match command.tag {
                0 => i,
                1 => 0,
                2 => i & 1,
                _ => unreachable!(),
            }];
        }
    }
    if flags & 1 != 0 {
        *front ^= 1;
    }
    Ok(())
}
fn read_slot(ram: &[u8], slot: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(FRAME_BYTES);
    for &(_, length, address) in regions(slot) {
        result.extend_from_slice(&ram[address..address + length]);
    }
    result
}
fn load_initial(ram: &mut [u8], initial: &[u8]) {
    for slot in 0..2 {
        for &(start, length, address) in regions(slot) {
            ram[address..address + length].copy_from_slice(&initial[start..start + length]);
        }
    }
}

pub struct Encoded {
    pub stream: Vec<u8>,
    pub initial: Vec<u8>,
    pub records: Vec<RecordStats>,
    pub max_chunk_bytes: usize,
    pub max_tick_slots: usize,
    pub fits_tick_budget: bool,
    pub total_record_bytes: usize,
}
pub fn encode(
    canvases: &[u8],
    top: usize,
    ticks: usize,
    tick_budget: usize,
) -> Result<Encoded, String> {
    if canvases.is_empty() || !canvases.len().is_multiple_of(super::FRAME_BYTES) || ticks == 0 {
        return Err("Invalid buffered logical frames or tick count".into());
    }
    let frames = canvases
        .chunks_exact(super::FRAME_BYTES)
        .map(|frame| compact(frame, top))
        .collect::<Result<Vec<_>, _>>()?;
    let count = frames.len();
    let mut stream = Vec::new();
    let mut records = Vec::with_capacity(count * ticks);
    let mut contents = [0usize; 2];
    let mut front = 0;
    let mut ram = vec![0; 65536];
    load_initial(&mut ram, &frames[0]);
    for current in 0..count {
        let back = front ^ 1;
        let before = contents[back];
        let target = (current + 1 < count).then_some(current + 1);
        let commands = target.map_or_else(Vec::new, |target| {
            commands(&frames[before], &frames[target], back)
        });
        let schedule = balanced_partition(&commands, ticks);
        for (tick, range) in schedule.iter().enumerate() {
            let swap = target.is_some() && tick + 1 == ticks;
            let (payload, mut stats) = packet(&commands[range.clone()], swap);
            if payload.len() > u16::MAX as usize {
                return Err("Buffered tick payload exceeds u16".into());
            }
            if stream.len() % 65536 + payload.len() + 2 > 65534 {
                stream.resize((stream.len() / 65536 + 1) * 65536, 0);
            }
            stats.index = records.len();
            stats.offset = stream.len();
            stats.target_slot = back;
            stats.source_content = current;
            stats.prepared_content = target;
            stats.back_contents_before = before;
            stream.extend_from_slice(&(payload.len() as u16).to_le_bytes());
            stream.extend_from_slice(&payload);
            let old_front = front;
            let snapshot = read_slot(&ram, old_front);
            decode_record(&mut ram, &mut front, &payload)?;
            if read_slot(&ram, old_front) != snapshot {
                return Err("Buffered encoder wrote front slot".into());
            }
            let expected = if swap { target.unwrap() } else { current };
            if read_slot(&ram, front) != frames[expected] {
                return Err(format!(
                    "Buffered reference mismatch at tick {}",
                    stats.index
                ));
            }
            records.push(stats);
        }
        if let Some(target) = target {
            contents[back] = target;
        }
    }
    stream.resize((stream.len() / 65536 + 1) * 65536, 0);
    let max_chunk_bytes = records.iter().map(|r| r.max_chunk_bytes).max().unwrap_or(0);
    let max_tick_slots = records.iter().map(|r| r.tick_slots).max().unwrap_or(0);
    let total_record_bytes = records.iter().map(|r| r.payload_bytes + 2).sum();
    Ok(Encoded {
        stream,
        initial: frames[0].clone(),
        records,
        max_chunk_bytes,
        max_tick_slots,
        fits_tick_budget: max_tick_slots <= tick_budget,
        total_record_bytes,
    })
}

pub fn memory_map_json() -> String {
    "[{\"slot\":0,\"regions\":[{\"compact_offset\":0,\"address\":6144,\"length\":27040}]},{\"slot\":1,\"regions\":[{\"compact_offset\":0,\"address\":33184,\"length\":20064},{\"compact_offset\":20064,\"address\":57344,\"length\":6976}]}]".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn canvases(frames: &[Vec<u8>], top: usize) -> Vec<u8> {
        let mut result = Vec::new();
        for frame in frames {
            let mut indices = vec![0; 320 * 200];
            for column in 0..160 {
                for y in 0..169 {
                    let value = frame[column * 169 + y];
                    let pos = (top + y) * 320 + column * 2;
                    indices[pos] = value & 15;
                    indices[pos + 1] = value >> 4;
                }
            }
            result.extend(super::super::pack(&indices));
        }
        result
    }
    #[test]
    fn compact_preserves_canvas_pixels_at_all_alignment_positions() {
        let frame = (0..FRAME_BYTES)
            .map(|i| (i * 37 + i / 251) as u8)
            .collect::<Vec<_>>();
        for top in [0, 15, 31] {
            assert_eq!(
                compact(&canvases(std::slice::from_ref(&frame), top), top).unwrap(),
                frame
            );
        }
    }
    #[test]
    fn both_buffers_reconstruct_back_history_publish_only_last_tick_and_hold_eof() {
        let first = vec![0; FRAME_BYTES];
        let mut second = first.clone();
        second[..256].fill(0x11);
        second[20060..20070].fill(0x44);
        let mut third = second.clone();
        third[500..900].fill(0x66);
        let mut fourth = third.clone();
        for (i, value) in fourth[FRAME_BYTES - 512..].iter_mut().enumerate() {
            *value = (i * 23) as u8;
        }
        let frames = vec![first, second, third, fourth];
        let encoded = encode(&canvases(&frames, 15), 15, 4, 1).unwrap();
        assert_eq!(encoded.records.len(), 16);
        assert!(!encoded.fits_tick_budget);
        assert!(encoded.max_chunk_bytes <= 1024);
        let mut ram = vec![0; 65536];
        load_initial(&mut ram, &encoded.initial);
        let mut front = 0;
        assert_eq!(read_slot(&ram, 0), frames[0]);
        assert_eq!(read_slot(&ram, 1), frames[0]);
        for record in &encoded.records {
            let before = read_slot(&ram, front);
            let payload =
                &encoded.stream[record.offset + 2..record.offset + 2 + record.payload_bytes];
            decode_record(&mut ram, &mut front, payload).unwrap();
            let expected = ((record.index + 1) / 4).min(frames.len() - 1);
            assert_eq!(read_slot(&ram, front), frames[expected]);
            if record.index % 4 != 3 {
                assert_eq!(read_slot(&ram, front), before);
                assert!(!record.swap);
            }
            if record.index >= 12 {
                assert_eq!(payload, [0, 0, 0]);
                assert!(!record.swap);
            }
            assert_eq!(
                record.tick_slots,
                record.decoder_cycles + 63 * (record.payload_bytes + 2)
            );
        }
        assert_eq!(encoded.records[4].back_contents_before, 0);
        assert_eq!(encoded.records[8].back_contents_before, 1);
        assert_eq!(ram[0xd000..0xe000], vec![0; 4096]);
    }
    #[test]
    fn chunk_commands_are_whole_and_count255_is_supported() {
        let before = vec![0; FRAME_BYTES];
        let after = (0..FRAME_BYTES)
            .map(|i| (i * 53 + i / 251 + 1) as u8)
            .collect::<Vec<_>>();
        let commands = commands(&before, &after, 1);
        assert!(commands.iter().any(|c| c.tag == 0 && c.count == 255));
        let schedule = balanced_partition(&commands, 4);
        assert_eq!(schedule.last().unwrap().end, commands.len());
        let mut ram = vec![0; 65536];
        load_initial(&mut ram, &before);
        let mut front = 0;
        let mut maxima = 0;
        for (tick, range) in schedule.iter().enumerate() {
            let (payload, stats) = packet(&commands[range.clone()], tick == 3);
            assert!(stats.max_chunk_bytes <= 1024);
            maxima = maxima.max(stats.tick_slots);
            decode_record(&mut ram, &mut front, &payload).unwrap();
        }
        assert_eq!(read_slot(&ram, front), after);
        assert!(maxima < packet(&commands, true).1.tick_slots);
    }
    #[test]
    fn decoder_rejects_front_writes_shadow_gap_bad_lengths_tags_and_terminators() {
        let mut cases = vec![vec![], vec![2, 0, 0], vec![0, 1, 4], vec![0, 0, 0, 0]];
        for command in [
            vec![0xff, 0x17, 1, 1, 5],
            vec![0xff, 0xcf, 1, 1, 5],
            vec![0xfe, 0xcf, 2, 1, 5],
            vec![0x3f, 0xfb, 1, 1, 5],
            vec![0x9f, 0x81, 1, 3, 5],
            vec![0x9f, 0x81, 0, 0],
        ] {
            let mut chunk = command;
            chunk.extend([0; 4]);
            let mut payload = vec![1];
            payload.extend((chunk.len() as u16).to_le_bytes());
            payload.extend(chunk);
            payload.extend([0; 2]);
            cases.push(payload);
        }
        for payload in cases {
            let mut ram = vec![0; 65536];
            let snapshot = ram.clone();
            let mut front = 0;
            assert!(
                decode_record(&mut ram, &mut front, &payload).is_err(),
                "accepted {payload:?}"
            );
            assert_eq!(front, 0);
            assert_eq!(ram, snapshot);
        }
        let mut oversized = vec![0];
        oversized.extend(1025u16.to_le_bytes());
        oversized.extend(vec![0; 1025]);
        oversized.extend([0; 2]);
        assert!(decode_record(&mut vec![0; 65536], &mut 0, &oversized).is_err());
    }
    #[test]
    fn empty_held_ticks_remain_nonzero_outer_records_and_mux_roundtrip() {
        let frames = vec![vec![0; FRAME_BYTES]; 2];
        let encoded = encode(&canvases(&frames, 15), 15, 3, 490000).unwrap();
        assert_eq!(encoded.records.len(), 6);
        assert!(encoded
            .records
            .iter()
            .all(|r| r.payload_bytes == 3 && r.chunks == 0));
        assert_eq!(encoded.records.iter().filter(|r| r.swap).count(), 1);
        let records = super::super::records_from_raw(&encoded.stream).unwrap();
        assert_eq!(records.len(), 6);
        let audio = vec![0; super::super::sample_at_tick(6, 281) * 4];
        let mux = super::super::mux_cache_blocks(&encoded.stream, &audio, 6, 281, 32768).unwrap();
        assert_eq!(mux.record_offsets.len(), 6);
        for (original, offset) in records.iter().zip(mux.record_offsets) {
            assert_eq!(&mux.file[offset..offset + original.len()], original);
        }
    }
}
