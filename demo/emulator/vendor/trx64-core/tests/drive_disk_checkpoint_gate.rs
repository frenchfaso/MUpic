//! Regression for dirty GCR tracks folded into the persistent drive image.

use trx64_core::drive::{DiskImage, DiskKind};
use trx64_core::Machine;

// ── the media ───────────────────────────────────────────────────────────────────

fn sectors_per_track(t: u8) -> usize {
    match t {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        _ => 17,
    }
}

fn sector_offset(t: u8, s: u8) -> usize {
    ((1..t).map(sectors_per_track).sum::<usize>() + s as usize) * 256
}

/// A blank, formatted 35-track D64: BAM on 18/0, an empty directory on 18/1.
fn blank_d64() -> Vec<u8> {
    let mut d = vec![0u8; 174_848];
    let bam = sector_offset(18, 0);
    d[bam] = 18;
    d[bam + 1] = 1;
    d[bam + 2] = 0x41;
    for t in 1..=35u8 {
        let n = sectors_per_track(t);
        let e = bam + 4 + (t as usize - 1) * 4;
        let used: u32 = if t == 18 { 0b11 } else { 0 };
        let free: u32 = ((1u32 << n) - 1) & !used;
        d[e] = free.count_ones() as u8;
        d[e + 1] = free as u8;
        d[e + 2] = (free >> 8) as u8;
        d[e + 3] = (free >> 16) as u8;
    }
    for i in 0..16 {
        d[bam + 0x90 + i] = *b"CHECKPOINT".get(i).unwrap_or(&0xa0);
    }
    d[bam + 0xa0] = 0xa0;
    d[bam + 0xa1] = 0xa0;
    d[bam + 0xa2] = b'C';
    d[bam + 0xa3] = b'P';
    d[bam + 0xa4] = 0xa0;
    d[bam + 0xa5] = b'2';
    d[bam + 0xa6] = b'A';
    for i in 0xa7..0xab {
        d[bam + i] = 0xa0;
    }
    d[bam + 256 + 1] = 0xff;
    d
}

/// A head move folds the dirty track into the drive's write-back image; the next
/// flush must bring the drive's `DiskImage` — what a persist writes — up to it even
/// though nothing is dirty any more. It did not: the flush asked only "is a track
/// dirty?", so a track written and then left stayed out of the persisted file until
/// some later write happened to be pending at a flush.
#[test]
fn a_track_the_head_left_reaches_the_persisted_image() {
    let blank = blank_d64();
    let mut m = Machine::new();
    m.drive8.attach_disk(DiskImage { kind: DiskKind::D64, bytes: blank.clone(), backing_path: None, read_only: false });
    let sector: Vec<u8> = (0..256).map(|i| (i as u8) ^ 0xa5).collect();
    {
        let r = &mut m.drive8.rotation;
        let ht = r.current_half_track as usize;
        assert_eq!(ht, 36, "parked on track 18");
        let img = r.image.as_mut().unwrap();
        assert_eq!(trx64_core::gcr::gcr_write_sector(&mut img.tracks[ht - 2], &sector, 5), trx64_core::gcr::CBMDOS_FDC_ERR_OK);
        r.write_one_bit_for_test(1);
        assert!(r.has_dirty_track());
        r.move_head(2); // to track 19: track 18 is folded in, nothing is dirty
        assert!(!r.has_dirty_track());
    }
    let as_written = m.drive8.disk_as_written().unwrap().bytes;
    assert_eq!(&as_written[sector_offset(18, 5)..sector_offset(18, 6)], &sector[..], "the write-back image has it");
    assert!(m.drive8.flush_disk_writeback(), "the flush brings the disk image up to date");
    let persisted = &m.drive8.get_attached_disk().unwrap().bytes;
    assert_eq!(&persisted[sector_offset(18, 5)..sector_offset(18, 6)], &sector[..], "a persist writes it");
    assert!(!m.drive8.flush_disk_writeback(), "and only once");
}
