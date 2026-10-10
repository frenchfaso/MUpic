//! Spec 871 — a second drive on the bus.
//!
//! Every test boots the real machine with the real KERNAL and DOS in both drives and
//! asks its question through the KERNAL: `LOAD"$",n`, `LOAD`, `SAVE`, and a BASIC
//! loop copying a file from 8 to 9. The disks are D64s built here, read back here
//! after the drive's write-back — nothing pokes the IEC core to make a point.
//!
//!   cargo test --release -p trx64-core --test second_drive_gate -- --nocapture
//!
//! Two characterisations run only when asked (`--ignored`): the seven gate games
//! booted from drive 9 (§7.8), and the frame time with B off and on (§7.9).

use std::path::Path;


use trx64_core::drive::{DiskImage, DiskKind, DrivePosition};

use trx64_core::iec::IecbusCallback;

use trx64_core::{Machine, NullSink};

const ROM_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../C64ReverseEngineeringMCP/resources/roms");
const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../C64ReverseEngineeringMCP/samples");
const DOS_ROM: &str = "dos1541-325302-01+901229-05.bin";
const FRAME: u64 = 19_656;

fn roms_present() -> bool {
    let d = Path::new(ROM_DIR);
    d.join("kernal-901227-03.bin").exists() && d.join(DOS_ROM).exists()
}

macro_rules! need_roms {
    () => {
        if !roms_present() {
            eprintln!("[skip] second_drive_gate: ROMs absent at {ROM_DIR}");
            return;
        }
    };
}

// ── D64: build and read back ────────────────────────────────────────────────────

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

/// A formatted 35-track D64, files laid down from track 17 downwards.
struct D64 {
    bytes: Vec<u8>,
    next: (u8, u8),
    dir_slot: usize,
}

impl D64 {
    fn new(disk_name: &[u8]) -> Self {
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
            d[e + 1] = (free & 0xff) as u8;
            d[e + 2] = ((free >> 8) & 0xff) as u8;
            d[e + 3] = ((free >> 16) & 0xff) as u8;
        }
        for i in 0..16 {
            d[bam + 0x90 + i] = *disk_name.get(i).unwrap_or(&0xa0);
        }
        d[bam + 0xa0] = 0xa0;
        d[bam + 0xa1] = 0xa0;
        d[bam + 0xa2] = b'2';
        d[bam + 0xa3] = b'D';
        d[bam + 0xa4] = 0xa0;
        d[bam + 0xa5] = b'2';
        d[bam + 0xa6] = b'A';
        for i in 0xa7..0xab {
            d[bam + i] = 0xa0;
        }
        let dir = sector_offset(18, 1);
        d[dir] = 0x00;
        d[dir + 1] = 0xff;
        D64 { bytes: d, next: (17, 0), dir_slot: 0 }
    }

    fn mark_used(&mut self, t: u8, s: u8) {
        let e = sector_offset(18, 0) + 4 + (t as usize - 1) * 4;
        let bit = 1u32 << s;
        let mut map = self.bytes[e + 1] as u32 | (self.bytes[e + 2] as u32) << 8 | (self.bytes[e + 3] as u32) << 16;
        assert!(map & bit != 0, "sector {t}/{s} already used");
        map &= !bit;
        self.bytes[e] -= 1;
        self.bytes[e + 1] = map as u8;
        self.bytes[e + 2] = (map >> 8) as u8;
        self.bytes[e + 3] = (map >> 16) as u8;
    }

    fn alloc(&mut self) -> (u8, u8) {
        let (t, s) = self.next;
        self.next = if (s as usize) + 1 < sectors_per_track(t) { (t, s + 1) } else { (t - 1, 0) };
        self.mark_used(t, s);
        (t, s)
    }

    /// Add a closed file (`ftype` 0x82 PRG, 0x81 SEQ) holding `data`.
    fn add(mut self, name: &[u8], ftype: u8, data: &[u8]) -> Self {
        let chunks: Vec<&[u8]> = data.chunks(254).collect();
        let secs: Vec<(u8, u8)> = (0..chunks.len()).map(|_| self.alloc()).collect();
        for (i, c) in chunks.iter().enumerate() {
            let o = sector_offset(secs[i].0, secs[i].1);
            if let Some(&(nt, ns)) = secs.get(i + 1) {
                self.bytes[o] = nt;
                self.bytes[o + 1] = ns;
            } else {
                self.bytes[o] = 0;
                self.bytes[o + 1] = (c.len() + 1) as u8;
            }
            self.bytes[o + 2..o + 2 + c.len()].copy_from_slice(c);
        }
        assert!(self.dir_slot < 8);
        let e = sector_offset(18, 1) + self.dir_slot * 32;
        self.dir_slot += 1;
        self.bytes[e + 2] = ftype;
        self.bytes[e + 3] = secs[0].0;
        self.bytes[e + 4] = secs[0].1;
        for i in 0..16 {
            self.bytes[e + 5 + i] = *name.get(i).unwrap_or(&0xa0);
        }
        self.bytes[e + 30] = secs.len() as u8;
        self
    }
}

/// The contents of the closed file `name` in `img`, following the directory and the
/// sector chain the DOS wrote. `None`: no such file.
fn read_file(img: &[u8], name: &[u8]) -> Option<(u8, Vec<u8>)> {
    let (mut t, mut s) = (18u8, 1u8);
    let mut guard = 0;
    while t != 0 && guard < 20 {
        guard += 1;
        let o = sector_offset(t, s);
        for k in 0..8 {
            let e = o + k * 32;
            let ftype = img[e + 2];
            if ftype & 0x80 == 0 {
                continue;
            }
            let n: Vec<u8> = img[e + 5..e + 21].iter().copied().take_while(|&b| b != 0xa0).collect();
            if n == name {
                let (mut ft, mut fs) = (img[e + 3], img[e + 4]);
                let mut out = Vec::new();
                let mut g = 0;
                while ft != 0 && g < 700 {
                    g += 1;
                    let d = sector_offset(ft, fs);
                    if img[d] == 0 {
                        out.extend_from_slice(&img[d + 2..=d + img[d + 1] as usize]);
                        break;
                    }
                    out.extend_from_slice(&img[d + 2..d + 256]);
                    ft = img[d];
                    fs = img[d + 1];
                }
                return Some((ftype, out));
            }
        }
        t = img[o];
        s = img[o + 1];
    }
    None
}

/// A tokenized BASIC program of REM lines, linked for `$0801`, as a PRG (load address
/// first). Valid, so the relink `LOAD` runs in direct mode leaves every byte as it is.
fn basic_prg(lines: usize) -> Vec<u8> {
    let mut prg = vec![0x01, 0x08];
    let mut addr: u16 = 0x0801;
    for i in 0..lines {
        let mut body = Vec::new();
        let num = 10 * (i as u16 + 1);
        body.extend_from_slice(&num.to_le_bytes());
        body.push(0x8f); // REM
        for k in 0..60u32 {
            body.push(0x21 + ((i as u32 * 7 + k * 3) % 90) as u8);
        }
        body.push(0);
        let next = addr + 2 + body.len() as u16;
        prg.extend_from_slice(&next.to_le_bytes());
        prg.extend_from_slice(&body);
        addr = next;
    }
    prg.extend_from_slice(&[0, 0]);
    prg
}

fn disk(bytes: Vec<u8>) -> DiskImage {
    DiskImage { kind: DiskKind::D64, bytes, backing_path: None, read_only: false }
}

/// The disk in `pos` after its write-back: what persist hands the host.
fn persisted(m: &mut Machine, pos: DrivePosition) -> Vec<u8> {
    let d = m.drive_mut(pos);
    d.flush_disk_writeback();
    d.get_attached_disk().expect("a disk").bytes.clone()
}

// ── the machine through the keyboard ────────────────────────────────────────────

fn frames(m: &mut Machine, n: u32) {
    let mut sink = NullSink;
    for _ in 0..n {
        m.run_for_full(FRAME, &mut sink, |_, _, _, _, _, _, _| {});
    }
}

/// A 1541's DOS takes about a second from power-on to its idle loop; until then it
/// does not see ATN (its VIA1 is not yet set up for the edge) and an ATN that falls
/// in that window leaves it holding DATA — the KERNAL then waits forever. So a drive
/// is switched on with the C64, or given this long before it is used.
const DOS_POWER_ON_FRAMES: u32 = 90;

/// Booted to READY with `a` in drive 8 and, when given, `b` in drive 9 — B switched
/// on together with the C64, as a user with two drives does.
fn booted(a: Vec<u8>, b: Option<Vec<u8>>) -> Machine {
    let mut m = Machine::new();
    m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
    if let Some(b) = b {
        m.drive_b.attach_disk(disk(b));
        m.set_drive_power(DrivePosition::B, true).expect("B on at 9");
    }
    frames(&mut m, 130);
    m.drive8.attach_disk(disk(a));
    frames(&mut m, 40);
    m
}

fn screen(m: &Machine) -> String {
    let mut s = String::new();
    for row in 0..25u16 {
        let mut line = String::new();
        for col in 0..40u16 {
            let v = m.read_full(0x0400 + row * 40 + col) & 0x7f;
            line.push(match v {
                1..=26 => (b'A' + (v - 1)) as char,
                48..=57 => (b'0' + (v - 48)) as char,
                0x2e => '.',
                0x24 => '$',
                0x22 => '"',
                0x3f => '?',
                _ => ' ',
            });
        }
        s.push_str(line.trim_end());
        s.push('\n');
    }
    s
}

/// Type `s` through the keyboard buffer, ten characters at a time.
fn type_in(m: &mut Machine, s: &[u8]) {
    for chunk in s.chunks(10) {
        let mut waited = 0;
        while m.read_full(0x00c6) != 0 && waited < 200 {
            frames(m, 1);
            waited += 1;
        }
        for (i, b) in chunk.iter().enumerate() {
            m.poke(0x0277 + i as u16, &[*b]);
        }
        m.poke(0x00c6, &[chunk.len() as u8]);
        frames(m, 2);
    }
}

/// Clear the screen, type `cmd`, run until the editor prints READY again.
fn command(m: &mut Machine, cmd: &[u8], max_frames: u32) -> String {
    type_in(m, b"\x93");
    frames(m, 3);
    type_in(m, cmd);
    for _ in 0..max_frames {
        frames(m, 1);
        if screen(m).contains("READY.") {
            break;
        }
    }
    screen(m)
}

fn load_dir(m: &mut Machine, unit: u8) -> String {
    let out = command(m, format!("LOAD\"$\",{unit}\r").as_bytes(), 1500);
    if out.contains("ERROR") {
        return out;
    }
    let list = command(m, b"LIST\r", 200);
    format!("{out}{list}")
}

// ── §7.2 — two disks, two drives ────────────────────────────────────────────────

#[test]
fn two_drives_list_their_own_disks() {
    need_roms!();
    let a = D64::new(b"DISKEIGHT").add(b"ONLYONA", 0x82, &basic_prg(2)).bytes;
    let b = D64::new(b"DISKNINE").add(b"ONLYONB", 0x82, &basic_prg(2)).bytes;
    let mut m = booted(a, Some(b));
    assert_eq!(m.iec.iecbus_callback, IecbusCallback::Conf3, "two true drives: the mixed callback");
    let d8 = load_dir(&mut m, 8);
    assert!(d8.contains("DISKEIGHT") && d8.contains("ONLYONA") && !d8.contains("ONLYONB"), "drive 8 listed:\n{d8}");
    let d9 = load_dir(&mut m, 9);
    assert!(d9.contains("DISKNINE") && d9.contains("ONLYONB") && !d9.contains("ONLYONA"), "drive 9 listed:\n{d9}");
    // And 8 again, after 9 has talked: the order they answered in changes nothing.
    let d8 = load_dir(&mut m, 8);
    assert!(d8.contains("DISKEIGHT") && d8.contains("ONLYONA"), "drive 8 again:\n{d8}");
}

// ── §7.3 — KERNAL load and save on 9 ────────────────────────────────────────────

#[test]
fn kernal_load_and_save_on_nine() {
    need_roms!();
    let prg = basic_prg(11); // 3 blocks
    let a = D64::new(b"DISKEIGHT").bytes;
    let b = D64::new(b"DISKNINE").add(b"FILE", 0x82, &prg).bytes;
    let mut m = booted(a.clone(), Some(b));

    let out = command(&mut m, b"LOAD\"FILE\",9\r", 3000);
    assert!(out.contains("LOADING") && !out.contains("ERROR"), "LOAD from 9:\n{out}");
    let end = 0x0801 + (prg.len() - 2) as u16;
    let loaded: Vec<u8> = (0x0801..end).map(|a| m.read_full(a)).collect();
    assert_eq!(loaded, prg[2..], "the PRG in RAM is B's file");
    assert_eq!(m.read_full(0x2d) as u16 | (m.read_full(0x2e) as u16) << 8, end, "end of load");

    let out = command(&mut m, b"SAVE\"NEW\",9\r", 3000);
    assert!(out.contains("SAVING") && !out.contains("ERROR"), "SAVE to 9:\n{out}");
    let b_img = persisted(&mut m, DrivePosition::B);
    let (ftype, saved) = read_file(&b_img, b"NEW").expect("NEW in B's image");
    assert_eq!(ftype, 0x82, "a closed PRG");
    assert_eq!(saved, prg, "NEW is byte-identical to the PRG");
    assert_eq!(read_file(&b_img, b"FILE").map(|f| f.1), Some(prg.clone()), "FILE untouched");
    assert_eq!(persisted(&mut m, DrivePosition::A), a, "A's image unchanged");
}

// ── §7.4 — a copy between them ──────────────────────────────────────────────────

const COPY_PROGRAM: &[&[u8]] = &[
    b"10 OPEN2,8,2,\"F,S,R\"\r",
    b"20 OPEN3,9,3,\"F,S,W\"\r",
    b"30 GET#2,A$:S=ST\r",
    b"40 IFA$=\"\"THENA$=CHR$(0)\r",
    b"50 PRINT#3,A$;\r",
    b"60 IFS=0THEN30\r",
    b"70 CLOSE3:CLOSE2\r",
];

fn seq_data() -> Vec<u8> {
    // Every byte value, CHR$(0) and CR included, over two blocks.
    (0..300u32).map(|i| ((i * 37 + 11) % 256) as u8).collect()
}

fn enter_copy_program(m: &mut Machine) {
    type_in(m, b"NEW\r");
    frames(m, 10);
    for line in COPY_PROGRAM {
        type_in(m, line);
        frames(m, 5);
    }
}

fn copy_disks() -> (Vec<u8>, Vec<u8>) {
    (D64::new(b"SOURCE").add(b"F", 0x81, &seq_data()).bytes, D64::new(b"TARGET").bytes)
}

#[test]
fn basic_copy_from_eight_to_nine() {
    need_roms!();
    let (a, b) = copy_disks();
    let mut m = booted(a.clone(), Some(b));
    enter_copy_program(&mut m);
    let out = command(&mut m, b"RUN\r", 20_000);
    assert!(out.contains("READY.") && !out.contains("ERROR"), "the copy ran:\n{out}");
    let b_img = persisted(&mut m, DrivePosition::B);
    let (ftype, copied) = read_file(&b_img, b"F").expect("F in B's image");
    assert_eq!(ftype, 0x81, "a closed SEQ");
    assert_eq!(copied, seq_data(), "the copy is byte-identical");
    assert_eq!(persisted(&mut m, DrivePosition::A), a, "the source disk is unchanged");
}


// ── Spec 870 D3 for position B — its ROM comes into force at its power-on ────────

#[test]
fn position_b_takes_its_rom_at_its_own_power_on() {
    need_roms!();
    let mut m = Machine::new();
    m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
    let dos = std::fs::read(Path::new(ROM_DIR).join(DOS_ROM)).expect("DOS ROM");
    let marked = |b: u8| {
        let mut r = dos.clone();
        r[0] = b; // $C000
        r
    };
    // Off since the machine came up: the DOS it was given waits for its power-on.
    m.drive_b.set_rom(&marked(0x42)).unwrap();
    m.set_drive_power(DrivePosition::B, true).unwrap();
    assert_eq!(m.drive_b.drive_peek(0xc000), 0x42, "in force at B's power-on");
    // A reset keeps the ROM a powered drive has.
    m.drive_b.set_rom(&marked(0x43)).unwrap();
    m.drive_b.reset();
    assert_eq!(m.drive_b.drive_peek(0xc000), 0x42, "a reset is not a power-on");
    // Off and on again: now the new one.
    m.set_drive_power(DrivePosition::B, false).unwrap();
    m.set_drive_power(DrivePosition::B, true).unwrap();
    assert_eq!(m.drive_b.drive_peek(0xc000), 0x43, "the next power-on brings it in");
    // A's ROM is not B's business.
    assert_eq!(m.drive8.drive_peek(0xc000), dos[0], "A keeps the stock DOS");
}

// ── §7.6 — same number refused ──────────────────────────────────────────────────

#[test]
fn the_same_unit_twice_is_refused_naming_the_other() {
    need_roms!();
    let mut m = booted(D64::new(b"DISKEIGHT").bytes, None);
    m.set_drive_unit(DrivePosition::B, 8).expect("jumpers of an off drive may be set");
    let e = m.set_drive_power(DrivePosition::B, true).expect_err("B at 8 beside A at 8");
    assert!(e.contains("position A") && e.contains("position B") && e.contains("unit 8"), "{e}");
    assert!(!m.drive_b.powered(), "refused means nothing changed");
    assert_eq!(m.iec.drive_slot_b, None);

    m.set_drive_unit(DrivePosition::B, 9).unwrap();
    m.set_drive_power(DrivePosition::B, true).expect("B at 9");
    frames(&mut m, DOS_POWER_ON_FRAMES);
    let e = m.set_drive_unit(DrivePosition::A, 9).expect_err("A's jumpers onto B's unit");
    assert!(e.contains("position B") && e.contains("unit 9"), "{e}");
    let e = m.set_drive_unit(DrivePosition::B, 8).expect_err("B's jumpers onto A's unit");
    assert!(e.contains("position A") && e.contains("unit 8"), "{e}");
    // Both still answer where they were.
    assert!(load_dir(&mut m, 8).contains("DISKEIGHT"));
    assert!(!load_dir(&mut m, 9).contains("DEVICE NOT PRESENT"));
}




// ── §7.8 — characterisation: the seven games booted from 9 ──────────────────────

fn in_game_ram(pc: u16) -> bool {
    // The seven-game gate's `game_running`: RAM outside the stack page's neighbourhood
    // of the KERNAL's own buffers.
    (0x0200..0xA000).contains(&pc) && !(0x0300..0x0400).contains(&pc)
}

struct BootFrom9 {
    kernal_stage: String,
    fast_stage: String,
    stop: String,
    /// The seven-game gate's PASS: game code sustained in RAM, or a title of ≥ 8
    /// colours — here with drive 9's head having moved, so the game came from 9.
    passes: bool,
}

fn distinct_colours(rgba: &[u8]) -> usize {
    rgba.chunks(4).map(|p| (p[0], p[1], p[2])).collect::<std::collections::HashSet<_>>().len()
}

/// §7.8 — A off, the game's disk in B at unit 9, `LOAD"*",9,1` + `RUN`, judged the
/// way the seven-game gate judges (game code sustained in RAM, or a title of ≥ 8
/// colours), with drive 9's own evidence beside it: did code run in its RAM (a
/// fastloader's drive half uploaded), and did its head move.
fn boot_from_nine(file: &str) -> Option<BootFrom9> {
    let path = format!("{SAMPLES}/{file}");
    let bytes = std::fs::read(&path).ok()?;
    let kind = if file.ends_with(".g64") { DiskKind::G64 } else { DiskKind::D64 };
    let mut m = Machine::new();
    m.boot_from_dir(Path::new(ROM_DIR)).expect("boot ROMs");
    m.drive8.set_power(false);
    m.drive_b.attach_disk(DiskImage { kind, bytes, backing_path: None, read_only: false });
    m.set_drive_power(DrivePosition::B, true).expect("B on at 9");
    frames(&mut m, 170);
    type_in(&mut m, b"\x93LOAD\"*\",9,1\r");
    let mut sink = NullSink;
    // The KERNAL stage, as the gate drives it: until the editor is idle again or a cap.
    let mut streak = 0;
    let mut returned = false;
    for _ in 0..400 {
        m.run_for_full(50_000, &mut sink, |_, _, _, _, _, _, _| {});
        let pc = m.cpu6510.reg_pc;
        if (0xE5C0..=0xE5F0).contains(&pc) && m.read_full(0x00c6) == 0 {
            streak += 1;
            if streak >= 3 {
                returned = true;
                break;
            }
        } else {
            streak = 0;
        }
    }
    let load_screen = screen(&m);
    let end = m.read_full(0xae) as u16 | (m.read_full(0xaf) as u16) << 8;
    let kernal_stage = if load_screen.contains("ERROR") {
        let line = load_screen.lines().find(|l| l.contains("ERROR")).unwrap_or("").trim().to_string();
        format!("no — {line}")
    } else if returned && load_screen.contains("LOADING") {
        format!("yes — back at READY, end ${end:04X}")
    } else if returned {
        "no — READY without LOADING".to_string()
    } else {
        format!("yes — did not return to READY (C64 at ${:04X})", m.cpu6510.reg_pc)
    };
    type_in(&mut m, b"RUN\r");
    let mut prev = false;
    let mut live = false;
    let mut drive_ram_code = false;
    let mut head_moves = 0u32;
    let mut last_ht = m.drive_b.half_track();
    let mut best = 0usize;
    let mut total = 0u64;
    while total < 100_000_000 {
        m.run_for_full(100_000, &mut sink, |_, _, _, _, _, _, _| {});
        total += 100_000;
        if m.drive_b.core.reg_pc < 0x0800 {
            drive_ram_code = true;
        }
        let ht = m.drive_b.half_track();
        if ht != last_ht {
            head_moves += 1;
            last_ht = ht;
        }
        let now = in_game_ram(m.cpu6510.reg_pc);
        if now && prev {
            live = true;
        }
        prev = now;
        if total % 1_000_000 == 0 {
            let (_, _, rgba) = m.render_canvas_rgba();
            let c = distinct_colours(&rgba);
            best = best.max(c);
        }
        // The gate's own early-out: game live and a coherent picture.
        if live && best > 4 && total >= 12_000_000 {
            break;
        }
    }
    let (_, _, rgba) = m.render_canvas_rgba();
    best = best.max(distinct_colours(&rgba));
    let verdict = if live && best >= 8 {
        "game live + title rendered"
    } else if live {
        "game code live in RAM"
    } else if best >= 8 {
        "title rendered"
    } else {
        "no"
    };
    let fast_stage = format!(
        "{verdict} — drive 9 ran code in its RAM: {}, head moved {head_moves}×, best frame {} colours",
        if drive_ram_code { "yes" } else { "no" },
        best
    );
    let text: String = screen(&m).lines().filter(|l| !l.trim().is_empty()).take(2).collect::<Vec<_>>().join(" / ");
    let stop = format!(
        "C64 ${:04X}, drive 9 ${:04X} on half-track {}; screen: {}",
        m.cpu6510.reg_pc,
        m.drive_b.core.reg_pc,
        m.drive_b.half_track(),
        if text.trim().is_empty() { "-".to_string() } else { text }
    );
    let passes = (live || best >= 8) && head_moves > 0;
    Some(BootFrom9 { kernal_stage, fast_stage, stop, passes })
}

// ── §7.8 — the games that boot from 9: regression tests ─────────────────────────
//
// The characterisation found four of the seven booting fully from drive 9 with
// drive 8 switched off: their loaders take the device from `$BA` (or talk to
// whatever answers). Each is a regression test for a fastloader on a drive other
// than 8. Heavy, like the seven-game gate: run with `--ignored`.

fn boots_from_nine(name: &'static str, file: &str) {
    need_roms!();
    let Some(r) = boot_from_nine(file) else {
        eprintln!("[skip] {name}: sample absent");
        return;
    };
    eprintln!("{name} from 9: {} | {} | {}", r.kernal_stage, r.fast_stage, r.stop);
    assert!(r.passes, "{name} no longer boots from drive 9: {} | {}", r.kernal_stage, r.fast_stage);
}

#[test]
#[ignore = "behavioral boot from drive 9; run with --ignored --nocapture"]
fn from_nine_scramble() {
    boots_from_nine("scramble", "scramble_infinity.d64");
}

#[test]
#[ignore = "behavioral boot from drive 9; run with --ignored --nocapture"]
fn from_nine_polarbear() {
    boots_from_nine("polarbear", "POLARBEAR.d64");
}

#[test]
#[ignore = "behavioral boot from drive 9; run with --ignored --nocapture"]
fn from_nine_greenberet() {
    boots_from_nine("greenberet", "green_beret[ocean_1986](!).g64");
}

#[test]
#[ignore = "behavioral boot from drive 9; run with --ignored --nocapture"]
fn from_nine_lastninja() {
    boots_from_nine("lastninja", "last_ninja_remix_s1[system3_1991].g64");
}
