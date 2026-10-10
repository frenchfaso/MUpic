//! Shared bounded-memory encoder used by the CLI and isolated browser workers.
use crate::{audio::sample_at_tick, author_dither::AuthorDither, dict4_rate, lut, sd};
use sha2::{Digest, Sha256};

pub const VIDEO_WIDTH: usize = 360;
pub const VIDEO_HEIGHT: usize = 200;
pub const RGB_BYTES: usize = VIDEO_WIDTH * VIDEO_HEIGHT * 3;
pub const PACKED_BYTES: usize = VIDEO_WIDTH * VIDEO_HEIGHT / 2;
pub const PAGE_FRAME_BYTES: usize = 180 * 256;
pub const CACHE_BYTES: usize = 262144;
pub const AUDIO_OFFSET: usize = 225280;
pub const FRAME_TICKS: usize = 3;
pub const CACHE_TICKS: usize = 20;
pub const RATE_BUDGET: usize = 425000;
pub const AUDIO_RATE: u32 = 22085;
pub const AUDIO_DIVIDER: u16 = 281;
pub const MAX_FRAMES: usize = 65535 / FRAME_TICKS;
pub const PALETTE: [[u8; 3]; 16] = [
    [0, 0, 0],
    [247, 247, 247],
    [141, 47, 52],
    [106, 212, 205],
    [152, 53, 164],
    [76, 180, 66],
    [44, 41, 177],
    [239, 239, 93],
    [152, 78, 32],
    [91, 56, 0],
    [209, 103, 109],
    [74, 74, 74],
    [123, 123, 123],
    [159, 239, 147],
    [109, 106, 239],
    [178, 178, 178],
];

pub fn content_fps() -> f64 {
    985248.0 / (63.0 * 312.0 * FRAME_TICKS as f64)
}
pub fn frame_count(seconds: f64) -> Result<usize, String> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err("Duration must be positive and finite".into());
    }
    let frames = (seconds * content_fps()).ceil() as usize;
    if !(2..=MAX_FRAMES).contains(&frames) {
        return Err(format!(
            "The player supports 2..={MAX_FRAMES} frames (up to {:.1} minutes)",
            MAX_FRAMES as f64 / content_fps() / 60.0
        ));
    }
    Ok(frames)
}

/// Bayer positions are physical canvas positions and never depend on time.
pub struct Quantizer {
    author: AuthorDither,
    lut: Vec<u32>,
    lut_blocks: Vec<bool>,
    lut_loaded: usize,
    lut_asset: Option<lut::AssetImporter>,
}
impl Quantizer {
    pub fn new(threads: usize) -> Result<Self, String> {
        if threads == 0 {
            return Err("Thread count must be positive".into());
        }
        let mut author = AuthorDither::new(&PALETTE, true)?;
        author.set_threads(threads);
        Ok(Self {
            author,
            lut: Vec::new(),
            lut_blocks: vec![false; lut::BLOCK_COUNT],
            lut_loaded: 0,
            lut_asset: None,
        })
    }
    fn cache_limit() -> usize {
        if cfg!(target_arch = "wasm32") {
            262_144
        } else {
            1_048_576
        }
    }
    /// Return distinct missing keys for shared work. Eviction counts actual
    /// colors, so repeated frames keep their exact plans without re-searching.
    pub fn prepare_keys(&mut self, rgb: &[u8]) -> Result<Vec<u32>, String> {
        if rgb.len() % RGB_BYTES != 0 || rgb.is_empty() {
            return Err("RGB batch must contain whole 360x200 RGB24 frames".into());
        }
        if self.lut_complete() {
            return Ok(Vec::new());
        }
        let mut missing = self.author.missing_rgb_keys(rgb);
        missing.retain(|&key| !self.lut_has(key));
        let limit = Self::cache_limit();
        if missing.len() > limit {
            return Err("RGB planning batch exceeds the bounded color cache".into());
        }
        if self.author.cached_rgb_count() + missing.len() > limit {
            self.author.clear_cache();
            missing = self.author.missing_rgb_keys(rgb);
            missing.retain(|&key| !self.lut_has(key));
            if missing.len() > limit {
                return Err("RGB planning batch exceeds the bounded color cache".into());
            }
        }
        Ok(missing)
    }
    pub fn import_plans(&mut self, keys: &[u32], plans: &[u32]) -> Result<(), String> {
        self.author.validate_plans(keys, plans)?;
        if self.author.cached_rgb_count() + self.author.additional_key_count(keys)
            > Self::cache_limit()
        {
            return Err("Imported plans exceed the bounded color cache".into());
        }
        self.author.import_plans(keys, plans)
    }
    pub fn preplan(&mut self, rgb: &[u8]) -> Result<(), String> {
        let missing = self.prepare_keys(rgb)?;
        let plans = self.author.plan_keys(&missing)?;
        self.import_plans(&missing, &plans)
    }
    pub fn quantize(&mut self, rgb: &[u8]) -> Result<Vec<u8>, String> {
        if rgb.len() != RGB_BYTES {
            return Err("RGB input must be exactly 360x200 RGB24".into());
        }
        self.preplan(rgb)?;
        Ok(self.pack_plans(rgb))
    }
    pub fn pack_plans(&mut self, rgb: &[u8]) -> Vec<u8> {
        if !self.lut.is_empty() {
            let mut packed = vec![0; PACKED_BYTES];
            for y in 0..VIDEO_HEIGHT {
                for x in 2..VIDEO_WIDTH - 2 {
                    let offset = (y * VIDEO_WIDTH + x) * 3;
                    let pixel = [rgb[offset], rgb[offset + 1], rgb[offset + 2]];
                    let key = (u32::from(pixel[0]) << 16
                        | u32::from(pixel[1]) << 8
                        | u32::from(pixel[2])) as usize;
                    let stored = self.lut[key];
                    let plan = if stored == u32::MAX {
                        self.author.plan(pixel)
                    } else {
                        stored as usize
                    };
                    packed[y * 180 + x / 2] |= self.author.colour(plan, x, y) << ((x & 1) * 4);
                }
            }
            return packed;
        }
        let plans = self.author.frame_plans(rgb);
        let mut packed = vec![0; PACKED_BYTES];
        for y in 0..VIDEO_HEIGHT {
            for x in 2..VIDEO_WIDTH - 2 {
                let color = self.author.colour(plans[y * VIDEO_WIDTH + x], x, y);
                packed[y * 180 + x / 2] |= color << ((x & 1) * 4);
            }
        }
        packed
    }
    fn lut_has(&self, key: u32) -> bool {
        !self.lut.is_empty() && self.lut[key as usize] != u32::MAX
    }
    pub fn generate_lut_block(&self, number: usize) -> Result<Vec<u8>, String> {
        lut::generate_block(number, &self.author)
    }
    pub fn import_lut_block(&mut self, bytes: &[u8]) -> Result<usize, String> {
        let block = lut::decode_block(bytes, &self.author)?;
        if self.lut.is_empty() {
            self.lut = vec![u32::MAX; lut::DOMAIN];
        }
        let start = block.number * lut::BLOCK_SIZE;
        self.lut[start..start + lut::BLOCK_SIZE].copy_from_slice(&block.plans);
        if !self.lut_blocks[block.number] {
            self.lut_blocks[block.number] = true;
            self.lut_loaded += 1;
        }
        Ok(block.number)
    }
    pub fn lut_loaded_blocks(&self) -> usize {
        self.lut_loaded
    }
    #[cfg(test)]
    pub(crate) fn lut_allocated_colors(&self) -> usize {
        self.lut.len()
    }
    pub fn lut_complete(&self) -> bool {
        self.lut_loaded == lut::BLOCK_COUNT
    }
    pub fn clear_lut(&mut self) {
        self.lut = Vec::new();
        self.lut_blocks.fill(false);
        self.lut_loaded = 0;
        self.lut_asset = None;
    }
    pub fn begin_lut_asset(&mut self, header: &[u8]) -> Result<(), String> {
        if self.lut_asset.is_some() {
            return Err("A LUT asset import is already running".into());
        }
        self.lut_asset = Some(lut::AssetImporter::begin(header, &self.author)?);
        Ok(())
    }
    pub fn append_lut_asset(&mut self, chunk: &[u8]) -> Result<(), String> {
        let result = self
            .lut_asset
            .as_mut()
            .ok_or("Begin LUT asset import before appending data")?
            .append(chunk);
        if result.is_err() {
            self.lut_asset = None;
        }
        result
    }
    pub fn finish_lut_asset(&mut self) -> Result<(), String> {
        self.finish_lut_asset_inner(None)
    }
    pub fn finish_lut_asset_verified(&mut self, expected_hash: &[u8]) -> Result<(), String> {
        if expected_hash.len() != 32 {
            return Err("Expected LUT asset checksum must contain 32 bytes".into());
        }
        self.finish_lut_asset_inner(Some(expected_hash))
    }
    fn finish_lut_asset_inner(&mut self, expected_hash: Option<&[u8]>) -> Result<(), String> {
        let importer = self
            .lut_asset
            .take()
            .ok_or("Begin LUT asset import before finishing")?;
        let plans = importer.finish(expected_hash)?;
        self.lut = plans;
        self.lut_blocks.fill(true);
        self.lut_loaded = lut::BLOCK_COUNT;
        Ok(())
    }
    pub fn abort_lut_asset(&mut self) {
        self.lut_asset = None;
    }
    pub fn author_mut(&mut self) -> &mut AuthorDither {
        &mut self.author
    }
}

pub fn to_page_frame(packed: &[u8]) -> Result<Vec<u8>, String> {
    if packed.len() != PACKED_BYTES {
        return Err("Packed input must contain 36000 row-major bytes".into());
    }
    let mut frame = vec![0; PAGE_FRAME_BYTES];
    for y in 0..VIDEO_HEIGHT {
        if packed[y * 180] != 0 || packed[y * 180 + 179] != 0 {
            return Err("Native SD requires two black pixels at each side".into());
        }
        for x in 0..180 {
            frame[x * 256 + y] = packed[y * 180 + x];
        }
    }
    Ok(frame)
}
pub fn to_row_frame(frame: &[u8]) -> Vec<u8> {
    (0..VIDEO_HEIGHT)
        .flat_map(|y| (0..180).map(move |x| frame[x * 256 + y]))
        .collect()
}
pub fn unpack_rgb(packed: &[u8]) -> Result<Vec<u8>, String> {
    if packed.len() != PACKED_BYTES {
        return Err("Invalid packed preview size".into());
    }
    Ok(packed
        .iter()
        .flat_map(|v| {
            [v & 15, v >> 4]
                .into_iter()
                .flat_map(|c| PALETTE[c as usize])
        })
        .collect())
}

/// Only the current 256 KiB cache block, last frame and PCM are kept in memory.
/// Drain finished blocks after each push; finish returns the independent prefix.
pub struct Encoder {
    frames: usize,
    received: usize,
    title: String,
    pcm: Vec<u8>,
    initial: Vec<u8>,
    previous: Vec<u8>,
    metric: dict4_rate::Prepared,
    block: Vec<u8>,
    cursor: usize,
    ticks_in_block: usize,
    total_ticks: usize,
    block_count: usize,
    stream_hash: Sha256,
    output: Vec<u8>,
    preview: Vec<u8>,
    finished: bool,
    pub peak_slots: usize,
    pub adapted_frames: usize,
}
impl Encoder {
    pub fn new(frames: usize, pcm: Vec<u8>, title: String) -> Result<Self, String> {
        if !(2..=MAX_FRAMES).contains(&frames) {
            return Err(format!("Frame count must be 2..={MAX_FRAMES}"));
        }
        if pcm.len() % 4 != 0 {
            return Err("PCM must be stereo S16LE (four bytes per sample)".into());
        }
        if !title.is_ascii() || title.contains('\0') || title.len() > 63 {
            return Err("Title must contain at most 63 ASCII bytes".into());
        }
        Ok(Self {
            frames,
            received: 0,
            title,
            pcm,
            initial: vec![],
            previous: vec![],
            metric: dict4_rate::Prepared::new(&PALETTE),
            block: vec![0; CACHE_BYTES],
            cursor: 0,
            ticks_in_block: 0,
            total_ticks: 0,
            block_count: 0,
            stream_hash: Sha256::new(),
            output: vec![],
            preview: vec![],
            finished: false,
            peak_slots: 0,
            adapted_frames: 0,
        })
    }
    pub fn push(&mut self, packed: &[u8]) -> Result<(), String> {
        if self.finished || self.received == self.frames {
            return Err("Encoder received too many frames or was already finished".into());
        }
        let exact = to_page_frame(packed)?;
        let reconstructed = if self.received == 0 {
            self.initial = exact.clone();
            exact
        } else {
            let after = self
                .metric
                .project(&self.previous, &exact, RATE_BUDGET, sd::peak)?;
            self.adapted_frames += usize::from(after != exact);
            let (records, peak) = sd::group(&self.previous, Some(&after))?;
            if peak > RATE_BUDGET {
                return Err(format!(
                    "Qualified cost model exceeds budget: {peak}/{RATE_BUDGET}"
                ));
            }
            self.peak_slots = self.peak_slots.max(peak);
            for record in records {
                self.packet(&record)?;
            }
            after
        };
        self.preview = to_row_frame(&reconstructed);
        self.previous = reconstructed;
        self.received += 1;
        Ok(())
    }
    fn packet(&mut self, payload: &[u8]) -> Result<(), String> {
        if self.cursor + payload.len() + 4 > AUDIO_OFFSET {
            return Err("Video cache area overflow".into());
        }
        self.block[self.cursor..self.cursor + 2]
            .copy_from_slice(&(payload.len() as u16).to_le_bytes());
        self.cursor += 2;
        self.block[self.cursor..self.cursor + payload.len()].copy_from_slice(payload);
        self.cursor += payload.len();
        self.ticks_in_block += 1;
        self.total_ticks += 1;
        if self.ticks_in_block == CACHE_TICKS {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), String> {
        if self.ticks_in_block == 0 {
            return Ok(());
        }
        let begin = sample_at_tick(self.total_ticks - self.ticks_in_block, AUDIO_DIVIDER) * 4;
        let end = sample_at_tick(self.total_ticks, AUDIO_DIVIDER) * 4;
        let length = end - begin;
        if length > CACHE_BYTES - AUDIO_OFFSET {
            return Err("PCM cache area overflow".into());
        }
        let available = self.pcm.len().min(end).saturating_sub(begin);
        if available != 0 {
            self.block[AUDIO_OFFSET..AUDIO_OFFSET + available]
                .copy_from_slice(&self.pcm[begin..begin + available]);
        }
        self.stream_hash.update(&self.block);
        self.output.extend_from_slice(&self.block);
        self.block.fill(0);
        self.cursor = 0;
        self.ticks_in_block = 0;
        self.block_count += 1;
        Ok(())
    }
    pub fn output(&self) -> &[u8] {
        &self.output
    }
    pub fn clear_output(&mut self) {
        self.output.clear();
    }
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }
    pub fn preview(&self) -> &[u8] {
        &self.preview
    }
    pub fn received(&self) -> usize {
        self.received
    }
    pub fn stream_offset(&self) -> usize {
        stream_offset(self.frames)
    }
    pub fn finish(&mut self) -> Result<Vec<u8>, String> {
        if self.finished || self.received != self.frames {
            return Err(format!(
                "Expected {} frames, received {} (or already finished)",
                self.frames, self.received
            ));
        }
        let (holds, peak) = sd::group(&self.previous, None)?;
        self.peak_slots = self.peak_slots.max(peak);
        for record in holds {
            self.packet(&record)?;
        }
        self.flush()?;
        let records = self.frames * FRAME_TICKS;
        let blocks = records.div_ceil(CACHE_TICKS);
        if self.total_ticks != records || self.block_count != blocks {
            return Err("Container cadence mismatch".into());
        }
        let initial_offset = align(256 + blocks * 3, 256);
        let stream_offset = stream_offset(self.frames);
        let mut prefix = vec![0; stream_offset];
        prefix[..4].copy_from_slice(b"MUP9");
        let values: &[(usize, usize, usize)] = &[
            (4, 1, 9),
            (5, 1, 9),
            (6, 1, 13),
            (7, 1, 1),
            (8, 2, 256),
            (10, 2, 360),
            (12, 2, 200),
            (14, 2, 356),
            (16, 2, 200),
            (18, 2, 0),
            (20, 1, 3),
            (21, 1, 0),
            (22, 1, 13),
            (24, 4, records),
            (28, 4, blocks),
            (32, 2, AUDIO_DIVIDER as usize),
            (34, 2, 20),
            (36, 4, AUDIO_OFFSET),
            (40, 4, CACHE_BYTES),
            (44, 4, 256),
            (48, 4, initial_offset),
            (52, 4, PAGE_FRAME_BYTES),
            (56, 4, stream_offset),
            (60, 4, blocks * CACHE_BYTES),
            (64, 4, stream_offset + blocks * CACHE_BYTES),
            (68, 4, self.frames),
            (72, 2, records - (blocks - 1) * CACHE_TICKS),
            (76, 4, AUDIO_RATE as usize),
        ];
        for &(at, count, value) in values {
            prefix[at..at + count].copy_from_slice(&(value as u32).to_le_bytes()[..count]);
        }
        prefix[96..128].copy_from_slice(&Sha256::digest(&self.initial));
        prefix[128..160].copy_from_slice(&self.stream_hash.clone().finalize());
        prefix[160..160 + self.title.len()].copy_from_slice(self.title.as_bytes());
        let mut crc = crc32fast::Hasher::new();
        crc.update(&prefix[..224]);
        crc.update(&prefix[228..256]);
        prefix[224..228].copy_from_slice(&crc.finalize().to_le_bytes());
        for block in 0..blocks {
            let start = block * CACHE_TICKS;
            let end = (start + CACHE_TICKS).min(records);
            let bytes =
                (sample_at_tick(end, AUDIO_DIVIDER) - sample_at_tick(start, AUDIO_DIVIDER)) * 4;
            prefix[256 + block * 3..259 + block * 3]
                .copy_from_slice(&(bytes as u32).to_le_bytes()[..3]);
        }
        prefix[initial_offset..initial_offset + PAGE_FRAME_BYTES].copy_from_slice(&self.initial);
        self.finished = true;
        Ok(prefix)
    }
}
fn align(n: usize, alignment: usize) -> usize {
    n.div_ceil(alignment) * alignment
}
fn stream_offset(frames: usize) -> usize {
    align(
        align(256 + (frames * FRAME_TICKS).div_ceil(CACHE_TICKS) * 3, 256) + PAGE_FRAME_BYTES,
        65536,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draining_cache_blocks_preserves_pcm_timing_and_zero_pads_the_tail() {
        let frames = 9;
        let ticks = frames * FRAME_TICKS;
        let pcm: Vec<u8> = (0..sample_at_tick(ticks - 1, AUDIO_DIVIDER) * 2)
            .flat_map(|n| (n as u16).to_le_bytes())
            .collect();
        let mut encoder = Encoder::new(frames, pcm.clone(), "Cache boundary".into()).unwrap();
        let mut stream = Vec::new();
        for _ in 0..frames {
            encoder.push(&vec![0; PACKED_BYTES]).unwrap();
            stream.extend(encoder.take_output());
        }
        let prefix = encoder.finish().unwrap();
        stream.extend(encoder.take_output());
        assert_eq!(stream.len(), 2 * CACHE_BYTES);
        assert_eq!(&prefix[128..160], &Sha256::digest(&stream)[..]);
        let mut reconstructed_pcm = Vec::new();
        for (block, count) in [CACHE_TICKS, ticks - CACHE_TICKS].into_iter().enumerate() {
            let length_at = 256 + block * 3;
            let length = usize::from(prefix[length_at])
                | usize::from(prefix[length_at + 1]) << 8
                | usize::from(prefix[length_at + 2]) << 16;
            let expected = (sample_at_tick(block * CACHE_TICKS + count, AUDIO_DIVIDER)
                - sample_at_tick(block * CACHE_TICKS, AUDIO_DIVIDER))
                * 4;
            assert_eq!(length, expected);
            let audio_at = block * CACHE_BYTES + AUDIO_OFFSET;
            reconstructed_pcm.extend_from_slice(&stream[audio_at..audio_at + length]);
            let mut cursor = block * CACHE_BYTES;
            for _ in 0..count {
                let bytes =
                    u16::from_le_bytes(stream[cursor..cursor + 2].try_into().unwrap()) as usize;
                assert!(bytes >= 2);
                cursor += 2 + bytes;
            }
            assert_eq!(&stream[cursor..cursor + 2], &[0, 0]);
        }
        assert_eq!(&reconstructed_pcm[..pcm.len()], &pcm);
        assert!(reconstructed_pcm[pcm.len()..].iter().all(|&v| v == 0));
        assert!(encoder.finish().is_err());
    }
    #[test]
    fn emitted_mup_has_valid_hashes_crc_pcm_and_sd_geometry() {
        let mut a = vec![0; PACKED_BYTES];
        let mut b = a.clone();
        for y in 0..200 {
            for x in 1..179 {
                a[y * 180 + x] = 0x55;
                b[y * 180 + x] = 0xaa;
            }
        }
        let pcm: Vec<_> = (0..12000u32)
            .flat_map(|n| (n as u16).to_le_bytes())
            .collect();
        let mut encoder = Encoder::new(2, pcm.clone(), "Test".into()).unwrap();
        encoder.push(&a).unwrap();
        encoder.push(&b).unwrap();
        let prefix = encoder.finish().unwrap();
        let stream = encoder.take_output();
        assert_eq!(&prefix[..4], b"MUP9");
        assert_eq!(prefix[5], 9);
        assert_eq!(prefix[22], 13);
        assert_eq!(&prefix[128..160], &Sha256::digest(&stream)[..]);
        let mut crc = crc32fast::Hasher::new();
        crc.update(&prefix[..224]);
        crc.update(&prefix[228..256]);
        assert_eq!(
            u32::from_le_bytes(prefix[224..228].try_into().unwrap()),
            crc.finalize()
        );
        let length = sample_at_tick(6, AUDIO_DIVIDER) * 4;
        assert_eq!(&stream[AUDIO_OFFSET..AUDIO_OFFSET + length], &pcm[..length]);
        assert!(encoder.peak_slots <= RATE_BUDGET);
        assert_eq!(encoder.preview(), b);
    }
    #[test]
    fn cache_eviction_changes_work_but_preserves_exact_pixels() {
        let mut q = Quantizer::new(1).unwrap();
        let rgb: Vec<u8> = (0..VIDEO_WIDTH * VIDEO_HEIGHT)
            .flat_map(|_| [100, 120, 140])
            .collect();
        let a = q.quantize(&rgb).unwrap();
        assert!(q.author_mut().cached_rgb_count() > 0);
        // Fill the remaining cache slots without spending time searching a
        // million colors; only the eviction decision is under test here.
        let keys: Vec<u32> = (0..Quantizer::cache_limit() as u32 - 1).collect();
        q.import_plans(&keys, &vec![0; keys.len()]).unwrap();
        assert_eq!(q.author_mut().cached_rgb_count(), Quantizer::cache_limit());
        assert!(q.prepare_keys(&rgb).unwrap().is_empty());
        assert_eq!(q.author_mut().cached_rgb_count(), Quantizer::cache_limit());
        let next: Vec<u8> = (0..VIDEO_WIDTH * VIDEO_HEIGHT)
            .flat_map(|_| [100, 120, 141])
            .collect();
        assert_eq!(q.prepare_keys(&next).unwrap(), [0x64788d]);
        assert_eq!(q.author_mut().cached_rgb_count(), 0);
        let b = q.quantize(&rgb).unwrap();
        assert_eq!(a, b);
    }
    #[test]
    fn repeated_frame_batches_are_bounded_by_unique_colors() {
        let mut q = Quantizer::new(1).unwrap();
        let rgb: Vec<u8> = (0..VIDEO_WIDTH * VIDEO_HEIGHT)
            .flat_map(|_| [100, 120, 140])
            .collect();
        let expected = q.quantize(&rgb).unwrap();
        let frames = Quantizer::cache_limit().div_ceil(VIDEO_WIDTH * VIDEO_HEIGHT) + 1;
        let batch: Vec<u8> = rgb
            .iter()
            .copied()
            .cycle()
            .take(frames * RGB_BYTES)
            .collect();
        assert!(q.prepare_keys(&batch).unwrap().is_empty());
        assert_eq!(q.author_mut().cached_rgb_count(), 1);
        assert_eq!(q.quantize(&rgb).unwrap(), expected);
        assert!(q.prepare_keys(&batch[..batch.len() - 1]).is_err());
    }
    #[test]
    fn rejects_bad_bounds_and_nonblack_edges() {
        assert!(Encoder::new(1, vec![], "test".into()).is_err());
        assert!(Encoder::new(MAX_FRAMES + 1, vec![], "test".into()).is_err());
        assert!(Encoder::new(2, vec![1], "test".into()).is_err());
        let mut frame = vec![0; PACKED_BYTES];
        frame[0] = 1;
        assert!(to_page_frame(&frame).is_err());
        assert!(frame_count(0.0).is_err());
    }
}
