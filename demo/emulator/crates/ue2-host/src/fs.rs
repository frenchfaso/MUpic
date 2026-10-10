//! Native files remain ordinary OS files. The browser's files live entirely in
//! Rust memory, independently in each WASM instance; no POSIX imports are needed.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use std::fs::*;

#[cfg(any(test, all(target_arch = "wasm32", target_os = "unknown")))]
#[path = "vfs.rs"]
mod vfs;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use vfs::*;

/// Install a downloaded asset, creating its parent directories. Existing file
/// handles continue to refer to the same file, as with ordinary write/truncate.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn register<P: AsRef<std::path::Path>>(path: P, bytes: &[u8]) -> std::io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}
