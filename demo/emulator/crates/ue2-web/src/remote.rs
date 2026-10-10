//! Read-only USB storage supplied by the browser's bounded HTTP range cache.
//! No remote image is registered in or duplicated into the Rust filesystem.

use std::{cell::Cell, io::{self, ErrorKind}};
use ue2_core::devices::usb::{block::BlockBackend, BLOCK_SIZE};

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[link(wasm_import_module = "ue2_host")]
extern "C" {
    /// 0 = destination filled, 1 = asynchronous cache miss, -1 = fatal I/O.
    fn remote_read(offset: u32, length: u32, destination: *mut u8) -> i32;
}

thread_local! {
    static CAPACITY: Cell<u32> = const { Cell::new(0) };
    static WAITING: Cell<bool> = const { Cell::new(false) };
    #[cfg(test)]
    static TEST_READER: Cell<Option<fn(u32, &mut [u8]) -> i32>> = const { Cell::new(None) };
}

pub(crate) fn configure(bytes: u32) -> io::Result<()> {
    if bytes == 0 || u64::from(bytes) % BLOCK_SIZE != 0 {
        return Err(io::Error::new(ErrorKind::InvalidInput, "Remote USB capacity must contain whole 512-byte blocks"));
    }
    CAPACITY.with(|capacity| capacity.set(bytes));
    resume();
    Ok(())
}

pub(crate) fn waiting() -> bool { WAITING.with(Cell::get) }
pub(crate) fn resume() { WAITING.with(|waiting| waiting.set(false)); }

pub(crate) fn backend() -> io::Result<Box<dyn BlockBackend>> {
    let bytes = CAPACITY.with(Cell::get);
    if bytes == 0 {
        return Err(io::Error::new(ErrorKind::InvalidInput, "Configure remote USB before initializing MUpic"));
    }
    resume();
    Ok(Box::new(RemoteDisk { bytes }))
}

struct RemoteDisk { bytes: u32 }

fn read_host(offset: u32, destination: &mut [u8]) -> i32 {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    unsafe { return remote_read(offset, destination.len() as u32, destination.as_mut_ptr()); }
    #[cfg(test)]
    if let Some(reader) = TEST_READER.with(Cell::get) { return reader(offset, destination); }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    { let _ = (offset, destination); -1 }
}

impl BlockBackend for RemoteDisk {
    fn blocks(&self) -> u64 { u64::from(self.bytes) / BLOCK_SIZE }
    fn read_only(&self) -> bool { true }
    fn read_blocks(&mut self, lba: u64, destination: &mut [u8]) -> io::Result<()> {
        let Some(offset) = lba.checked_mul(BLOCK_SIZE) else {
            return Err(io::Error::new(ErrorKind::InvalidInput, "Remote USB LBA overflow"));
        };
        if destination.len() as u64 % BLOCK_SIZE != 0
            || offset > u64::from(self.bytes)
            || destination.len() as u64 > u64::from(self.bytes) - offset
        {
            return Err(io::Error::new(ErrorKind::InvalidInput, "Remote USB read outside disk"));
        }
        if destination.is_empty() { return Ok(()); }
        match read_host(offset as u32, destination) {
            0 => Ok(()),
            1 => {
                WAITING.with(|waiting| waiting.set(true));
                Err(io::Error::new(ErrorKind::WouldBlock, "Remote USB range is pending"))
            }
            _ => Err(io::Error::new(ErrorKind::Other, "Remote USB range failed")),
        }
    }
    fn write_blocks(&mut self, _lba: u64, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::new(ErrorKind::PermissionDenied, "Remote USB disk is read-only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_bounds_pending_and_read_only_contract() {
        assert!(configure(0).is_err());
        assert!(configure(513).is_err());
        configure(4 * 512).unwrap();
        let mut disk = backend().unwrap();
        assert_eq!(disk.blocks(), 4);
        assert!(disk.read_only());
        assert!(disk.write_blocks(0, &[0; 512]).is_err());
        assert!(disk.read_blocks(u64::MAX, &mut [0; 512]).is_err());
        assert!(disk.read_blocks(4, &mut [0; 512]).is_err());
        assert!(disk.read_blocks(0, &mut [0; 513]).is_err());
        assert!(!waiting());
        TEST_READER.with(|reader| reader.set(Some(|_, _| 1)));
        assert_eq!(disk.read_blocks(1, &mut [0; 512]).unwrap_err().kind(), ErrorKind::WouldBlock);
        assert!(waiting());
        resume();
        assert!(!waiting());
        TEST_READER.with(|reader| reader.set(Some(|offset, destination| {
            assert_eq!(offset, 512);
            destination.fill(0x6d);
            0
        })));
        let mut data = [0; 512];
        disk.read_blocks(1, &mut data).unwrap();
        assert_eq!(data, [0x6d; 512]);
        assert!(!waiting());
        TEST_READER.with(|reader| reader.set(Some(|_, _| -1)));
        assert_eq!(disk.read_blocks(0, &mut data).unwrap_err().kind(), ErrorKind::Other);
        assert!(!waiting(), "a terminal error must reach SCSI, not hang forever");
        TEST_READER.with(|reader| reader.set(None));
    }

}

#[cfg(test)]
pub(crate) fn hold_for_test() { WAITING.with(|waiting| waiting.set(true)); }
