//! Exact RGB24 lookup blocks; both storage adapters use this same format.
use crate::author_dither::AuthorDither;
use sha2::{Digest, Sha256};

pub const DOMAIN: usize = 1 << 24;
pub const BLOCK_SIZE: usize = 4096;
pub const BLOCK_COUNT: usize = DOMAIN / BLOCK_SIZE;
const HEADER_BYTES: usize = 80;
pub const BLOCK_BYTES: usize = HEADER_BYTES + BLOCK_SIZE * 4;
pub const ASSET_HEADER_BYTES: usize = 128;
pub const MAX_ASSET_BYTES: usize = ASSET_HEADER_BYTES + DOMAIN * 4;
const ASSET_MAGIC: &[u8; 8] = b"MUPLUT1\0";
const MAGIC: &[u8; 4] = b"MULT";
const SCHEMA: u32 = 1;
include!(concat!(env!("OUT_DIR"), "/lut_id.rs"));
include!(concat!(env!("OUT_DIR"), "/embedded_lut.rs"));

pub fn identity() -> &'static str {
    env!("MUP_LUT_ID")
}
pub fn block_identity() -> &'static str {
    env!("MUP_LUT_BLOCK_ID")
}

/// A complete asset becomes visible only after its encoded bytes and IDs pass.
pub struct AssetImporter {
    plans: Vec<u32>,
    encoding: u8,
    candidate_count: usize,
    expected_bytes: usize,
    received: usize,
    expected_hash: [u8; 32],
    hash: Sha256,
    full_hash: Sha256,
    carry: Vec<u8>,
}
impl AssetImporter {
    pub fn begin(header: &[u8], author: &AuthorDither) -> Result<Self, String> {
        if header.len() != ASSET_HEADER_BYTES {
            return Err("Invalid LUT asset header size".into());
        }
        let schema = u32::from_le_bytes(header[8..12].try_into().unwrap());
        let encoding = header[12];
        let domain = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let candidate_count = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
        let payload = u64::from_le_bytes(header[24..32].try_into().unwrap());
        let width = match encoding {
            0 => 4,
            1 => 3,
            _ => return Err("Unsupported LUT asset encoding".into()),
        };
        let expected_identity: Vec<u8> = identity()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        if &header[..8] != ASSET_MAGIC
            || schema != 1
            || header[13..16] != [0; 3]
            || domain != DOMAIN
            || candidate_count != author.candidate_count()
            || payload != (DOMAIN * width) as u64
            || header[32..64] != expected_identity
            || header[64..96] != author.pattern_fingerprint()
        {
            return Err("Stale or incompatible LUT asset header".into());
        }
        let mut full_hash = Sha256::new();
        full_hash.update(header);
        Ok(Self {
            plans: Vec::with_capacity(DOMAIN),
            encoding,
            candidate_count,
            expected_bytes: payload as usize,
            received: 0,
            expected_hash: header[96..128].try_into().unwrap(),
            hash: Sha256::new(),
            full_hash,
            carry: Vec::with_capacity(4),
        })
    }
    pub fn append(&mut self, chunk: &[u8]) -> Result<(), String> {
        if chunk.len() > self.expected_bytes - self.received {
            return Err("LUT asset payload exceeds its declared size".into());
        }
        self.hash.update(chunk);
        self.full_hash.update(chunk);
        self.received += chunk.len();
        let width = if self.encoding == 0 { 4 } else { 3 };
        let mut bytes = chunk;
        if !self.carry.is_empty() {
            let needed = (width - self.carry.len()).min(bytes.len());
            self.carry.extend_from_slice(&bytes[..needed]);
            bytes = &bytes[needed..];
            if self.carry.len() == width {
                let plan = decode_plan(&self.carry);
                self.push(plan)?;
                self.carry.clear();
            }
        }
        let mut pairs = bytes.chunks_exact(width);
        for pair in pairs.by_ref() {
            self.push(decode_plan(pair))?;
        }
        self.carry.extend_from_slice(pairs.remainder());
        Ok(())
    }
    fn push(&mut self, plan: u32) -> Result<(), String> {
        if plan as usize >= self.candidate_count {
            return Err("Invalid LUT asset candidate id".into());
        }
        self.plans.push(plan);
        Ok(())
    }
    pub fn finish(self, expected_full_hash: Option<&[u8]>) -> Result<Vec<u32>, String> {
        if self.received != self.expected_bytes
            || self.plans.len() != DOMAIN
            || !self.carry.is_empty()
        {
            return Err("LUT asset payload is incomplete".into());
        }
        if self.hash.finalize().as_slice() != self.expected_hash {
            return Err("LUT asset payload checksum mismatch".into());
        }
        if expected_full_hash
            .is_some_and(|expected| self.full_hash.finalize().as_slice() != expected)
        {
            return Err("LUT asset manifest checksum mismatch".into());
        }
        Ok(self.plans)
    }
}
fn decode_plan(bytes: &[u8]) -> u32 {
    let mut value = [0; 4];
    value[..bytes.len()].copy_from_slice(bytes);
    u32::from_le_bytes(value)
}

pub struct Block {
    pub number: usize,
    pub plans: Vec<u32>,
}

fn checksum(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(&bytes[..48]);
    hash.update(&bytes[HEADER_BYTES..]);
    hash.finalize().into()
}

fn keys(number: usize) -> Result<Vec<u32>, String> {
    if number >= BLOCK_COUNT {
        return Err("LUT block number is outside the RGB24 domain".into());
    }
    let begin = (number * BLOCK_SIZE) as u32;
    Ok((begin..begin + BLOCK_SIZE as u32).collect())
}

pub fn generate_block(number: usize, author: &AuthorDither) -> Result<Vec<u8>, String> {
    let plans = author.plan_keys(&keys(number)?)?;
    let mut bytes = Vec::with_capacity(BLOCK_BYTES);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&SCHEMA.to_le_bytes());
    bytes.extend_from_slice(&(number as u32).to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&IDENTITY);
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend(plans.into_iter().flat_map(u32::to_le_bytes));
    let digest = checksum(&bytes);
    bytes[48..80].copy_from_slice(&digest);
    Ok(bytes)
}

pub fn decode_block(bytes: &[u8], author: &AuthorDither) -> Result<Block, String> {
    if bytes.len() != BLOCK_BYTES {
        return Err("Invalid LUT block size".into());
    }
    let schema = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let number = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if &bytes[..4] != MAGIC
        || schema != SCHEMA
        || number >= BLOCK_COUNT
        || bytes[12..16] != [0; 4]
        || bytes[16..48] != IDENTITY
    {
        return Err("Stale or invalid LUT block header".into());
    }
    if bytes[48..80] != checksum(bytes) {
        return Err("LUT block checksum mismatch".into());
    }
    let plans: Vec<u32> = bytes[HEADER_BYTES..]
        .chunks_exact(4)
        .map(|plan| u32::from_le_bytes(plan.try_into().unwrap()))
        .collect();
    author.validate_plans(&keys(number)?, &plans)?;
    Ok(Block { number, plans })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Quantizer, PALETTE, RGB_BYTES};

    fn key(rgb: [u8; 3]) -> u32 {
        (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2])
    }
    fn resign(bytes: &mut [u8]) {
        let digest = checksum(bytes);
        bytes[48..80].copy_from_slice(&digest);
    }
    fn asset_header(author: &AuthorDither, encoding: u8) -> Vec<u8> {
        let width = if encoding == 0 { 4 } else { 3 };
        let mut hash = Sha256::new();
        let zeros = [0; 65_536];
        for _ in 0..DOMAIN * width / zeros.len() {
            hash.update(zeros);
        }
        let mut header = vec![0; ASSET_HEADER_BYTES];
        header[..8].copy_from_slice(ASSET_MAGIC);
        header[8..12].copy_from_slice(&1u32.to_le_bytes());
        header[12] = encoding;
        header[16..20].copy_from_slice(&(DOMAIN as u32).to_le_bytes());
        header[20..24].copy_from_slice(&(author.candidate_count() as u32).to_le_bytes());
        header[24..32].copy_from_slice(&((DOMAIN * width) as u64).to_le_bytes());
        for (index, byte) in identity().as_bytes().chunks_exact(2).enumerate() {
            header[32 + index] =
                u8::from_str_radix(std::str::from_utf8(byte).unwrap(), 16).unwrap();
        }
        header[64..96].copy_from_slice(&author.pattern_fingerprint());
        header[96..128].copy_from_slice(&hash.finalize());
        header
    }
    fn stream_zero_asset(owner: &mut Quantizer, header: &[u8]) -> [u8; 32] {
        owner.begin_lut_asset(header).unwrap();
        let mut hash = Sha256::new();
        hash.update(header);
        let mut remaining = u64::from_le_bytes(header[24..32].try_into().unwrap()) as usize;
        let zeros = [0; 65_536];
        // Deliberately split inside a u24/u32 word, then cross chunk boundaries.
        owner.append_lut_asset(&zeros[..1]).unwrap();
        hash.update(&zeros[..1]);
        remaining -= 1;
        while remaining != 0 {
            let chunk = &zeros[..remaining.min(zeros.len())];
            owner.append_lut_asset(chunk).unwrap();
            hash.update(chunk);
            remaining -= chunk.len();
        }
        hash.finalize().into()
    }

    #[test]
    fn streamed_assets_activate_only_after_complete_verified_payload() {
        let mut owner = Quantizer::new(1).unwrap();
        for encoding in [0, 1] {
            let header = asset_header(owner.author_mut(), encoding);
            let expected = stream_zero_asset(&mut owner, &header);
            assert!(!owner.lut_complete());
            assert_eq!(owner.lut_loaded_blocks(), 0);
            owner.finish_lut_asset_verified(&expected).unwrap();
            assert!(owner.lut_complete());
            assert!(owner.prepare_keys(&vec![51; RGB_BYTES]).unwrap().is_empty());
            assert_eq!(
                owner.quantize(&vec![51; RGB_BYTES]).unwrap(),
                vec![0; crate::PACKED_BYTES]
            );
            owner.clear_lut();
        }
    }

    #[test]
    fn invalid_streams_and_manifest_hash_never_activate_or_replace_lookup() {
        let mut owner = Quantizer::new(1).unwrap();
        let header = asset_header(owner.author_mut(), 1);
        for offset in [0, 8, 12, 13, 16, 20, 24, 32, 64] {
            let mut invalid = header.clone();
            invalid[offset] ^= 128;
            assert!(owner.begin_lut_asset(&invalid).is_err(), "offset {offset}");
        }
        owner.begin_lut_asset(&header).unwrap();
        owner.append_lut_asset(&[0, 0]).unwrap();
        assert!(owner.finish_lut_asset().is_err());
        owner.begin_lut_asset(&header).unwrap();
        assert!(owner.append_lut_asset(&[255, 255, 255]).is_err());
        owner.begin_lut_asset(&header).unwrap();
        owner.abort_lut_asset();
        assert!(owner.append_lut_asset(&[0, 0, 0]).is_err());
        let mut bad_hash = header.clone();
        bad_hash[96] ^= 1;
        stream_zero_asset(&mut owner, &bad_hash);
        assert!(owner.finish_lut_asset().is_err());
        assert_eq!(owner.lut_loaded_blocks(), 0);
        let digest = stream_zero_asset(&mut owner, &header);
        owner.finish_lut_asset_verified(&digest).unwrap();
        let mut expected = stream_zero_asset(&mut owner, &header);
        expected[0] ^= 1;
        assert!(owner.finish_lut_asset_verified(&expected).is_err());
        assert!(
            owner.lut_complete(),
            "A failed update must preserve the previous complete table"
        );
    }

    #[test]
    fn generated_block_matches_scalar_search_and_worker_counts() {
        let mut scalar = Quantizer::new(1).unwrap();
        let parallel = Quantizer::new(3).unwrap();
        assert_eq!(scalar.lut_allocated_colors(), 0);
        let bytes = scalar.generate_lut_block(0).unwrap();
        assert_eq!(bytes, parallel.generate_lut_block(0).unwrap());
        assert_eq!(bytes.len(), BLOCK_BYTES);
        let block = decode_block(&bytes, scalar.author_mut()).unwrap();
        assert_eq!(block.number, 0);
        for key in [0u32, 1, 8, 255, 0x0808, 0x0f00, 4095] {
            let rgb = [(key >> 16) as u8, (key >> 8) as u8, key as u8];
            assert_eq!(
                block.plans[key as usize] as usize,
                scalar.author_mut().plan(rgb)
            );
        }
        assert_eq!(block.plans[0], 0);
        assert_eq!(scalar.lut_loaded_blocks(), 0);
        assert_eq!(
            scalar.lut_allocated_colors(),
            0,
            "Generation must not allocate the dense LUT"
        );
        assert!(scalar.generate_lut_block(BLOCK_COUNT).is_err());
        assert_eq!(identity().len(), 64);
    }

    #[test]
    fn malformed_blocks_are_rejected_before_import() {
        let generator = Quantizer::new(2).unwrap();
        let bytes = generator.generate_lut_block(0).unwrap();
        let mut owner = Quantizer::new(1).unwrap();
        for offset in [0, 4, 8, 12, 16, 48, 80] {
            let mut invalid = bytes.clone();
            invalid[offset] ^= 1;
            assert!(owner.import_lut_block(&invalid).is_err(), "offset {offset}");
        }
        for length in [0, HEADER_BYTES - 1, BLOCK_BYTES - 1] {
            assert!(owner.import_lut_block(&bytes[..length]).is_err());
        }
        let mut invalid = bytes.clone();
        invalid[8..12].copy_from_slice(&(BLOCK_COUNT as u32).to_le_bytes());
        resign(&mut invalid);
        assert!(owner.import_lut_block(&invalid).is_err());
        let mut invalid = bytes;
        invalid[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
        resign(&mut invalid);
        assert!(owner.import_lut_block(&invalid).is_err());
        assert_eq!(owner.lut_loaded_blocks(), 0);
        assert_eq!(
            owner.lut_allocated_colors(),
            0,
            "Invalid input must not allocate the dense LUT"
        );
    }

    #[test]
    fn imported_blocks_preserve_all_bayer_positions_palette_and_fallback() {
        let mut owner = Quantizer::new(2).unwrap();
        let mut reference = Quantizer::new(2).unwrap();
        let grey = PALETTE[12];
        let grey_block = key(grey) as usize / BLOCK_SIZE;
        for number in [0, grey_block] {
            let bytes = owner.generate_lut_block(number).unwrap();
            assert_eq!(owner.import_lut_block(&bytes).unwrap(), number);
            assert_eq!(owner.import_lut_block(&bytes).unwrap(), number);
        }
        assert_eq!(owner.lut_loaded_blocks(), 2);
        assert_eq!(owner.lut_allocated_colors(), DOMAIN);
        assert!(!owner.lut_complete());
        for color in [[0, 0, 127], grey, [123, 122, 125], [239, 42, 57]] {
            let rgb: Vec<u8> = (0..RGB_BYTES / 3).flat_map(|_| color).collect();
            let missing = owner.prepare_keys(&rgb).unwrap();
            if matches!(key(color) as usize / BLOCK_SIZE, number if number == 0 || number == grey_block)
            {
                assert!(missing.is_empty());
            } else {
                assert_eq!(missing, [key(color)]);
            }
            let actual = owner.quantize(&rgb).unwrap();
            assert_eq!(
                actual,
                reference.quantize(&rgb).unwrap(),
                "color {color:?}, all 64 Bayer positions"
            );
            for row in actual.chunks_exact(180) {
                assert_eq!(row[0], 0);
                assert_eq!(row[179], 0);
            }
        }
        owner.clear_lut();
        assert_eq!(owner.lut_loaded_blocks(), 0);
        assert_eq!(owner.lut_allocated_colors(), 0);
        assert!(!owner.lut_complete());
    }
}
