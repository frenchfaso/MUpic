//! Load the bundled lossless RGB24 asset and cache its verified decoded form.
use flate2::read::MultiGzDecoder;
use mupic_encoder::{lut, Quantizer};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};

pub fn default_directory() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    return env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches/MUpic"));
    #[cfg(target_os = "windows")]
    return env::var_os("LOCALAPPDATA").map(|root| PathBuf::from(root).join("MUpic/Cache"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .map(|root| root.join("mupic"))
    }
}

pub struct LutCache {
    directory: Option<PathBuf>,
}

impl LutCache {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self {
            directory: directory.map(|root| root.join(lut::identity())),
        }
    }

    pub fn prepare(&self, quantizer: &mut Quantizer, require_cache: bool) -> Result<(), String> {
        let compressed = lut::EMBEDDED_LUT.ok_or("Bundled RGB24 lookup asset is missing")?;
        self.prepare_asset(quantizer, compressed, require_cache)?;
        eprintln!("RGB24 lookup table ready.");
        Ok(())
    }

    fn prepare_asset(
        &self,
        quantizer: &mut Quantizer,
        compressed: &[u8],
        require_cache: bool,
    ) -> Result<(), String> {
        quantizer.clear_lut();
        let result = self.prepare_inner(quantizer, compressed, require_cache);
        if result.is_err() {
            quantizer.abort_lut_asset();
            quantizer.clear_lut();
        }
        result
    }

    fn prepare_inner(
        &self,
        quantizer: &mut Quantizer,
        compressed: &[u8],
        require_cache: bool,
    ) -> Result<(), String> {
        let mut header = [0; lut::ASSET_HEADER_BYTES];
        MultiGzDecoder::new(compressed)
            .read_exact(&mut header)
            .map_err(|error| format!("Cannot read bundled RGB24 lookup asset: {error}"))?;
        let payload = u64::from_le_bytes(header[24..32].try_into().unwrap());
        if payload > (lut::MAX_ASSET_BYTES - lut::ASSET_HEADER_BYTES) as u64 {
            return Err("Bundled RGB24 lookup asset exceeds the size limit".into());
        }
        if let Some(directory) = &self.directory {
            let restored = (|| {
                let file = File::open(directory.join("rgb24-lut.bin")).ok()?;
                let metadata = file.metadata().ok()?;
                if !metadata.is_file() || metadata.len() != payload + lut::ASSET_HEADER_BYTES as u64
                {
                    return None;
                }
                load_stream(file, quantizer, &header, &mut Storage::default()).ok()
            })();
            if restored.is_some() {
                return Ok(());
            }
            quantizer.abort_lut_asset();
            quantizer.clear_lut();
        }
        let mut storage = match &self.directory {
            Some(directory) => match PendingCache::new(directory.clone()) {
                Ok(writer) => Storage {
                    writer: Some(writer),
                    error: None,
                },
                Err(error) => Storage {
                    writer: None,
                    error: Some(error),
                },
            },
            None => Storage {
                writer: None,
                error: Some("Cannot determine the user cache; provide --cache-dir".into()),
            },
        };
        if require_cache {
            if let Some(error) = &storage.error {
                return Err(error.clone());
            }
        }
        load_stream(
            MultiGzDecoder::new(compressed),
            quantizer,
            &header,
            &mut storage,
        )?;
        match storage.finish() {
            Ok(()) => Ok(()),
            Err(error) if require_cache => Err(error),
            Err(error) => {
                eprintln!("MUpic: lookup cache unavailable ({error}); using the bundled table.");
                Ok(())
            }
        }
    }
}

fn load_stream(
    mut source: impl Read,
    quantizer: &mut Quantizer,
    expected_header: &[u8; lut::ASSET_HEADER_BYTES],
    storage: &mut Storage,
) -> Result<(), String> {
    let mut header = [0; lut::ASSET_HEADER_BYTES];
    source
        .read_exact(&mut header)
        .map_err(|error| format!("Truncated RGB24 lookup asset header: {error}"))?;
    // Pin cached data to the published asset, including its encoding and hash.
    if &header != expected_header {
        return Err("Cached lookup asset does not match the bundled table".into());
    }
    quantizer.begin_lut_asset(&header)?;
    storage.write(&header);
    let mut remaining = u64::from_le_bytes(header[24..32].try_into().unwrap());
    let mut buffer = [0; 65_536];
    while remaining != 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        source
            .read_exact(&mut buffer[..count])
            .map_err(|error| format!("Truncated RGB24 lookup asset: {error}"))?;
        quantizer.append_lut_asset(&buffer[..count])?;
        storage.write(&buffer[..count]);
        remaining -= count as u64;
    }
    // Reading through EOF also verifies the gzip trailer and rejects extras.
    let mut extra = [0];
    if source
        .read(&mut extra)
        .map_err(|error| format!("Invalid RGB24 lookup asset trailer: {error}"))?
        != 0
    {
        return Err("RGB24 lookup asset has trailing data".into());
    }
    quantizer.finish_lut_asset()
}

#[derive(Default)]
struct Storage {
    writer: Option<PendingCache>,
    error: Option<String>,
}
impl Storage {
    fn write(&mut self, bytes: &[u8]) {
        if let Some(writer) = &mut self.writer {
            if let Err(error) = writer.file.as_mut().unwrap().write_all(bytes) {
                self.error = Some(format!("Cannot write lookup cache: {error}"));
                self.writer = None;
            }
        }
    }
    fn finish(self) -> Result<(), String> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.writer
            .ok_or("Lookup cache is unavailable")?
            .commit()
            .map_err(|error| format!("Cannot publish lookup cache: {error}"))
    }
}

struct PendingCache {
    file: Option<File>,
    temporary: PathBuf,
    directory: PathBuf,
    committed: bool,
    _lock: File,
}
impl PendingCache {
    fn new(directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Cannot create lookup cache: {error}"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("cache.lock"))
            .map_err(|error| format!("Cannot open lookup cache lock: {error}"))?;
        lock.try_lock()
            .map_err(|error| format!("Cannot acquire lookup cache lock: {error}"))?;
        let temporary = directory.join(".rgb24-lut.partial");
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Cannot recover lookup cache: {error}")),
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("Cannot create lookup cache: {error}"))?;
        Ok(Self {
            file: Some(file),
            temporary,
            directory,
            committed: false,
            _lock: lock,
        })
    }
    fn commit(mut self) -> std::io::Result<()> {
        self.file.as_ref().unwrap().sync_all()?;
        self.file = None;
        fs::rename(&self.temporary, self.directory.join("rgb24-lut.bin"))?;
        self.committed = true;
        let _ = File::open(&self.directory).and_then(|directory| directory.sync_all());
        Ok(())
    }
}
impl Drop for PendingCache {
    fn drop(&mut self) {
        self.file = None;
        if !self.committed {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use mupic_encoder::RGB_BYTES;
    use sha2::{Digest, Sha256};
    use std::{
        io::{Seek, SeekFrom},
        sync::{
            atomic::{AtomicU64, Ordering},
            OnceLock,
        },
    };

    static DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);
    static ASSET: OnceLock<Vec<u8>> = OnceLock::new();
    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "mupic-native-asset-test-{}-{}",
                std::process::id(),
                DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn asset() -> &'static [u8] {
        ASSET.get_or_init(|| {
            // A synthetic all-black plan table tests storage/transport only.
            // The shared core tests validate numeric output independently.
            let mut owner = Quantizer::new(1).unwrap();
            let zeros = [0; 65_536];
            let bytes = lut::DOMAIN * 4;
            let mut payload_hash = Sha256::new();
            for _ in 0..bytes / zeros.len() {
                payload_hash.update(zeros);
            }
            let mut header = [0; lut::ASSET_HEADER_BYTES];
            header[..8].copy_from_slice(b"MUPLUT1\0");
            header[8..12].copy_from_slice(&1u32.to_le_bytes());
            header[16..20].copy_from_slice(&(lut::DOMAIN as u32).to_le_bytes());
            header[20..24]
                .copy_from_slice(&(owner.author_mut().candidate_count() as u32).to_le_bytes());
            header[24..32].copy_from_slice(&(bytes as u64).to_le_bytes());
            for (index, pair) in lut::identity().as_bytes().chunks_exact(2).enumerate() {
                header[32 + index] =
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
            }
            header[64..96].copy_from_slice(&owner.author_mut().pattern_fingerprint());
            header[96..128].copy_from_slice(&payload_hash.finalize());
            let mut gzip = GzEncoder::new(vec![], Compression::fast());
            gzip.write_all(&header).unwrap();
            for _ in 0..bytes / zeros.len() {
                gzip.write_all(&zeros).unwrap();
            }
            gzip.finish().unwrap()
        })
    }

    #[test]
    fn cache_roundtrip_recovers_corruption_and_preserves_black_frame_output() {
        let directory = TestDirectory::new();
        let cache = LutCache::new(Some(directory.0.clone()));
        let mut quantizer = Quantizer::new(2).unwrap();
        cache.prepare_asset(&mut quantizer, asset(), true).unwrap();
        assert!(quantizer.lut_complete());
        let rgb = vec![0; RGB_BYTES];
        let expected = Quantizer::new(1).unwrap().quantize(&rgb).unwrap();
        assert_eq!(quantizer.quantize(&rgb).unwrap(), expected);
        cache.prepare_asset(&mut quantizer, asset(), true).unwrap();
        assert!(quantizer.lut_complete());
        let path = cache.directory.as_ref().unwrap().join("rgb24-lut.bin");
        let mut corrupt = OpenOptions::new().write(true).open(&path).unwrap();
        corrupt
            .seek(SeekFrom::Start(lut::ASSET_HEADER_BYTES as u64))
            .unwrap();
        corrupt.write_all(&1u32.to_le_bytes()).unwrap();
        drop(corrupt);
        cache.prepare_asset(&mut quantizer, asset(), true).unwrap();
        assert_eq!(quantizer.quantize(&rgb).unwrap(), expected);
        assert!(!cache
            .directory
            .as_ref()
            .unwrap()
            .join(".rgb24-lut.partial")
            .exists());
    }

    #[test]
    fn unavailable_cache_uses_asset_and_prewarm_reports_failure() {
        let directory = TestDirectory::new();
        let blocked = directory.0.join("regular-file");
        fs::write(&blocked, b"preserved").unwrap();
        let cache = LutCache::new(Some(blocked.clone()));
        let mut quantizer = Quantizer::new(1).unwrap();
        cache.prepare_asset(&mut quantizer, asset(), false).unwrap();
        assert!(quantizer.lut_complete());
        assert!(cache.prepare_asset(&mut quantizer, asset(), true).is_err());
        assert!(!quantizer.lut_complete());
        assert_eq!(fs::read(blocked).unwrap(), b"preserved");
    }

    #[test]
    fn invalid_gzip_never_activates_lut_or_publishes_cache() {
        let directory = TestDirectory::new();
        let cache = LutCache::new(Some(directory.0.clone()));
        let mut compressed = asset().to_vec();
        let offset = compressed.len() - 8;
        compressed[offset] ^= 1;
        let mut quantizer = Quantizer::new(1).unwrap();
        assert!(cache
            .prepare_asset(&mut quantizer, &compressed, true)
            .is_err());
        assert!(!quantizer.lut_complete());
        let path = cache.directory.as_ref().unwrap();
        assert!(!path.join("rgb24-lut.bin").exists());
        assert!(!path.join(".rgb24-lut.partial").exists());
    }
}
