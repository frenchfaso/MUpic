//! The U64's SIDs on TRX64's reSID: the firmware's SID decode (sockets, UltiSIDs and their split instances), one reSID
//! per receiver, the audio mixer, and an ARMSID identity in socket 1. Spec: docs/specs/S17-ultisid.md (after S14
//! §W4-SID) and TRX64 Spec 855; status: docs/status/sid-audio.md.
//!
//! - **Decode.** A decoder hits an address in $D400-$D7FF or $DE00-$DFFF when `(A11..A4 & MASK) == BASE`
//!   (sid_editor.cc:162-165), from the core config latches the firmware writes (u64.h:110-128). A socket also needs its
//!   enable (u64_config.cc:815-818) and a fitted chip (`--sid-socket1 armsid`, [`Sid::set_socket1`]). C64_EMUSID_SPLIT
//!   picks an UltiSID's register set A-D from address bits (u64_config.cc:320; sid_editor.cc:166-181).
//! - **Routing.** TRX64 routes an address to one chip, the U64 a write to every decoder that hits. So a TRX64 chip is a
//!   group, one distinct set of receivers, and each traced write fans out to its group (S17 §2.1).
//! - **Engines.** One reSID per receiver that received a write, all clocked with the same cycle deltas and mixed with the
//!   mixer's channel gains (S17 §2.2, §2.5).
//! - **Readback.** TRX64 answers the 6502 for chip 0 (855 D3). For the other chips, whose models TRX64 does not tick,
//!   and for the ARMSID in configuration mode, UE2 answers $1B/$1C through TRX64's host door (855 D5). Firmware DMA
//!   reads go through UE2's own decode.
//! - **Timing.** TRX64's write trace (855 D4) queues every write with its chip and cycle. With an [`AudioSink`] each is
//!   applied at its cycle and reSID follows every `advance_to`; without one, CPU writes only set registers and a DMA
//!   access clocks the gap.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use trx64_core::resid_ffi::{MODEL_6581, MODEL_8580, PAL_CLOCK_FREQ, SAMPLE_INTERPOLATE};
use trx64_core::sid::SidMapping;
use trx64_core::{BusKind, Machine, Observer, Resid, ResidConfig};

/// The stereo sample stream of the emulated SIDs, in emulated-time order at the rate given to [`Sid::set_audio`]: frames
/// of a left and a right sample, interleaved (S29).
pub trait AudioSink {
    fn samples(&mut self, pcm: &[i16]);
}

impl<T: AudioSink + ?Sized> AudioSink for Box<T> {
    fn samples(&mut self, pcm: &[i16]) {
        (**self).samples(pcm);
    }
}

/// Core config offsets (0x10180000 + off, u64.h:110-128).
const SID1_BASE: u8 = 0x08;
const EMUSID2_BASE: u8 = 0x0B;
const SID1_MASK: u8 = 0x0C;
const EMUSID2_MASK: u8 = 0x0F;
const SID1_EN: u8 = 0x11;
const SID2_EN: u8 = 0x12;
const STEREO_ADDRSEL: u8 = 0x14;
const EMUSID1_WAVES: u8 = 0x20;
const EMUSID2_WAVES: u8 = 0x21;
const EMUSID_SPLIT: u8 = 0x29;

/// Decoder index: C64_SID1_BASE + i and C64_SID1_MASK + i (u64.h:110-117), the firmware's SID slots 0-3 (MapSid,
/// u64_config.cc:1943-1947).
const SOCKET1: usize = 0;
const ULTISID1: usize = 2;

/// C64_EMUSID_SPLIT index → the address bits of A11..A4 that pick an instance (split_bits, u64_config.cc:320).
const SPLIT_BITS: [u8; 8] = [0x00, 0x02, 0x04, 0x08, 0x10, 0x06, 0x12, 0x18];

/// Receivers, one bit each in a set: socket 1, then UltiSID 1 and 2 instances A-D. Socket 2 is never fitted.
const RECEIVERS: usize = 9;
const SOCKET1_RX: usize = 0;
const ULTISID1_A: usize = ultisid_rx(0, 0);

/// UltiSID `n` (0 or 1), instance `i` (0-3 = A-D).
const fn ultisid_rx(n: usize, i: usize) -> usize {
    1 + 4 * n + i
}

const fn bit(rx: usize) -> u16 {
    1 << rx
}

/// The 32-byte blocks a SID can occupy (u64_sid_base, u64_config.cc:218-226): 32 in $D400-$D7FF, 16 in $DE00-$DFFF.
const BLOCKS: [u16; 48] = {
    let mut blocks = [0; 48];
    let mut i = 0;
    while i < 48 {
        blocks[i] = if i < 32 { 0xD400 + 32 * i as u16 } else { 0xDE00 + 32 * (i as u16 - 32) };
        i += 1;
    }
    blocks
};

/// A SID mapped into $DE00-$DFFF answers reads ahead of the expansion port. An unverified assumption (S17 §2.1, 855
/// §7): the RTL that would settle it is closed; the firmware offers those addresses as SID addresses, so a SID there is
/// meant to be heard.
const AHEAD_OF_EXPANSION: bool = true;

/// U64_AUDIO_MIXER (0x10100500): two bytes per channel (u64_config.cc:1349-1358).
const MIXER_BYTES: usize = 20;
/// The mixer the default settings give (S17 §1.4), until the firmware writes it.
const MIXER_DEFAULT: [u8; MIXER_BYTES] = [
    0x5A, 0x5A, 0x5A, 0x5A, 0x79, 0x27, 0x27, 0x79, 0x79, 0x27, 0x27, 0x79, 0x19, 0x0D, 0x0D, 0x19, 0x04, 0x04, 0x04, 0x04,
];
/// A channel's two bytes summing to this are unity mono gain: the firmware's 0 dB centre, 0x5A/0x5A (S17 §2.5).
const UNITY: i32 = 180;
/// One side's unity: byte `2i` is the channel's left gain, `2i + 1` its right, 0x5A each at the 0 dB centre (S29 §1).
const SIDE_UNITY: i32 = UNITY / 2;
/// Mixer channels of the receivers (u64_config.cc:436-446).
const CH_ULTISID1: usize = 0;
const CH_ULTISID2: usize = 1;
const CH_SOCKET1: usize = 2;

/// Without an audio sink, a gap longer than this is clocked as this many cycles (1 s PAL): the chip has settled, and a
/// DMA read after minutes of silence does not clock minutes of reSID.
const SILENT_CATCH_UP: u64 = 985_248;
/// Largest cycle count handed to reSID at once (its `clock` takes an `int`).
const CHUNK: u64 = 1 << 20;
/// S24 M3: samples the UDP audio stream's tap may hold before the oldest are dropped — a second of stereo at the
/// PAL rate, which is far more than the emission interval, so nothing is lost while the host drains.
const STREAM_TAP_CAP: usize = 2 * 48_000;

/// reSID's clock until the machine's row says otherwise (TRX64 `ResidConfig` default, PAL).
const CLOCK_HZ: u64 = PAL_CLOCK_FREQ as u64;

/// ARMSID "VI" answer: firmware version, shown as "%d.%d" (readParams, sid_device_armsid.cc:105-175). Any value serves.
const ARMSID_VERSION: [u8; 2] = [3, 0];
/// ARMSID "UI" answer: supply voltage in mV, big endian (readParams). A socket not holding a 6581 gets the 9 V
/// regulator setting (effectuate_settings, u64_config.cc:753-790).
const ARMSID_MILLIVOLTS: u16 = 9000;

/// What TRX64's trace and host door reach from inside a machine run, where [`Sid`] cannot be borrowed. Thread-local,
/// because a machine runs on one thread: one machine per thread.
struct Door {
    /// Traced writes not applied yet: (cycle, chip, register, value).
    writes: Vec<(u64, u8, u8, u8)>,
    /// Socket 1's ARMSID. The trace drives it, so a probe reads its answer in the same run.
    armsid: ArmSid,
    /// Bit `chip` set: that chip's group holds socket 1.
    armsid_chips: u64,
    /// $1B/$1C of every chip from its group's first reSID as of the last catch-up; None for chip 0 and for a group
    /// without an engine.
    readback: Vec<Option<[u8; 2]>>,
}

impl Door {
    const fn new() -> Self {
        Door { writes: Vec::new(), armsid: ArmSid::new(), armsid_chips: 0, readback: Vec::new() }
    }

    fn armsid_on(&self, chip: u8) -> bool {
        self.armsid_chips.checked_shr(u32::from(chip)).is_some_and(|bits| bits & 1 != 0)
    }
}

thread_local! {
    static DOOR: RefCell<Door> = const { RefCell::new(Door::new()) };
}

/// TRX64's SID write trace (855 D4): CPU writes at their cycle, REU and host pokes at `c64_core.clk`, `write_full` at
/// the stale `Machine.clk`.
fn traced(chip: u8, reg: u8, val: u8, clk: u64) {
    DOOR.with(|door| {
        let mut door = door.borrow_mut();
        if door.armsid_on(chip) {
            door.armsid.write(reg, val);
        }
        door.writes.push((clk, chip, reg, val));
    });
}

/// TRX64's host read and peek door (855 D5): $1B/$1C from the ARMSID in configuration mode, else from the reSID of a
/// chip TRX64 does not tick. Side-effect free, so the peek is the same function.
fn answered(chip: u8, reg: usize) -> Option<u8> {
    if !(0x1B..=0x1C).contains(&reg) {
        return None;
    }
    DOOR.with(|door| {
        let door = door.borrow();
        door.armsid_on(chip)
            .then(|| door.armsid.read(reg as u8))
            .flatten()
            .or_else(|| door.readback.get(usize::from(chip)).copied().flatten().map(|r| r[reg - 0x1B]))
    })
}

/// The SID decode latches (docs/hw/10 §SID addressing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Decode {
    /// SID1, SID2, EMUSID1, EMUSID2 BASE and MASK.
    base: [u8; 4],
    mask: [u8; 4],
    socket_en: [bool; 2],
    /// C64_EMUSID1/2_WAVES: 0 = 6581, 1 = 8580 (u64_config.cc:1651-1656).
    waves: [u8; 2],
    /// C64_EMUSID_SPLIT index, both UltiSIDs (get_sid_addresses, u64_config.cc:255-256).
    split: u8,
}

impl Default for Decode {
    /// The firmware's default map until it writes the latches: sockets disabled, all four decoders at $D400 mirrored
    /// through $D7FF (boot log "Resulting address map: Slot1: 40/C0 (Disabled) … Emu1: 40/C0  Emu2: 40/C0",
    /// u64_config.cc:881-887).
    fn default() -> Self {
        Decode { base: [0x40; 4], mask: [0xC0; 4], socket_en: [false; 2], waves: [0; 2], split: 0 }
    }
}

impl Decode {
    fn write(&mut self, off: u8, val: u8) {
        match off {
            SID1_BASE..=EMUSID2_BASE => self.base[usize::from(off - SID1_BASE)] = val,
            SID1_MASK..=EMUSID2_MASK => self.mask[usize::from(off - SID1_MASK)] = val,
            SID1_EN | SID2_EN => self.socket_en[usize::from(off - SID1_EN)] = val & 1 != 0,
            // The address-select pin of a dual chip in a socket (SetSidAddress, u64_config.cc:1889-1891). The emulated
            // ARMSID is one chip, so both halves reach it, and the firmware has already cleared the bit from MASK.
            STEREO_ADDRSEL => {}
            EMUSID1_WAVES | EMUSID2_WAVES => self.waves[usize::from(off - EMUSID1_WAVES)] = val & 1,
            EMUSID_SPLIT => self.split = val & 7,
            _ => {}
        }
    }

    /// Receivers of the block holding `addr`; socket 1 only when `socket1` is fitted. `0x01` in BASE is "unmapped":
    /// `a & MASK` never has bit 0 (u64_config.cc:202-203).
    ///
    /// A4 is not decoded: every MASK the firmware writes leaves bit 0 open (`0xFE & ~other`, u64_config.cc:1905-1918;
    /// 0xC0 at boot, 0xF0/0xFE for detection), and its address editor steps in 32-byte blocks (sid_editor.cc:160-163).
    fn receivers(&self, addr: u16, socket1: bool) -> u16 {
        if !Sid::window(addr) {
            return 0;
        }
        let a = (addr >> 4) as u8 & !0x01;
        let hits = |dev: usize| a & self.mask[dev] == self.base[dev];
        let mut rx = 0;
        if socket1 && self.socket_en[SOCKET1] && hits(SOCKET1) {
            rx |= bit(SOCKET1_RX);
        }
        for n in 0..2 {
            if hits(ULTISID1 + n) {
                rx |= bit(ultisid_rx(n, self.instance(a)));
            }
        }
        rx
    }

    /// The UltiSID instance, 0-3 for A-D, that `a` = A11..A4 selects (sid_editor.cc:166-181).
    fn instance(&self, a: u8) -> usize {
        let bits = SPLIT_BITS[usize::from(self.split)];
        usize::from(match bits {
            0x06 => (a & 0x06) >> 1,
            0x12 => ((a & 0x02) >> 1) + ((a & 0x10) >> 3),
            0x18 => (a & 0x18) >> 3,
            _ => u8::from(a & bits != 0),
        })
    }
}

/// The ARMSID configuration interface on registers $1D-$1F, as the firmware drives it.
///
/// "SID" into $1D/$1E/$1F enters configuration mode (u64_config.cc:599-604); any other $1D exits it
/// (u64_config.cc:629; sid_device_armsid.cc:222-229). In configuration mode a write to $1E or $1F runs the command
/// pair ($1F, $1E):
/// - `($1F, 'I')` answers on $1B/$1C: 'D' → "NO" (the ARMSID reply, u64_config.cc:621), 'I' → not 'L'/'R' (ARMSID,
///   not ARM2SID, u64_config.cc:622-636), 'V' version, 'F' '6'/'8' mode, 'U' voltage, 'H' filter nibbles
///   (readParams, sid_device_armsid.cc:105-175);
/// - `($1F, 'E')` sets: '6'/'8' the mode (S_set_mode, sid_device_armsid.cc:241-250), $8n/$9n/$An/$Bn the four filter
///   nibbles (S_set_filt, 252-278), $C0/$CF save to RAM/flash (280-295, nothing to save here).
#[derive(Clone, Debug)]
struct ArmSid {
    /// Last values written to $1D, $1E, $1F.
    cfg: [u8; 3],
    config: bool,
    answer: [u8; 2],
    model: i32,
    /// 6581 strength, 6581 lowest, 8580 highest, 8580 lowest: signed nibbles around the firmware defaults
    /// (readParams and S_set_filt, sid_device_armsid.cc:105-175, 252-278).
    filter: [u8; 4],
}

impl Default for ArmSid {
    fn default() -> Self {
        Self::new()
    }
}

impl ArmSid {
    const fn new() -> Self {
        ArmSid { cfg: [0; 3], config: false, answer: [0; 2], model: MODEL_6581, filter: [0; 4] }
    }

    /// A write to register `reg` (0x00-0x1F). True when it changed the mode.
    fn write(&mut self, reg: u8, val: u8) -> bool {
        if !(0x1D..=0x1F).contains(&reg) {
            return false;
        }
        self.cfg[usize::from(reg - 0x1D)] = val;
        if reg == 0x1D {
            if val != b'S' {
                self.config = false;
            }
            return false;
        }
        if &self.cfg == b"SID" {
            self.config = true;
        }
        self.config && self.command(self.cfg[2], self.cfg[1])
    }

    /// True when the command changed the mode.
    fn command(&mut self, cmd: u8, sub: u8) -> bool {
        match (cmd, sub) {
            (b'D' | b'I', b'I') => self.answer = *b"NO",
            (b'V', b'I') => self.answer = ARMSID_VERSION,
            (b'F', b'I') => self.answer = [if self.model == MODEL_8580 { b'8' } else { b'6' }, 0],
            (b'U', b'I') => self.answer = ARMSID_MILLIVOLTS.to_be_bytes(),
            (b'H', b'I') => {
                let f = self.filter;
                self.answer = [f[0] << 4 | f[1], f[2] << 4 | f[3]];
            }
            (b'6' | b'8', b'E') => {
                let model = if cmd == b'8' { MODEL_8580 } else { MODEL_6581 };
                let changed = model != self.model;
                self.model = model;
                return changed;
            }
            (0x80..=0xBF, b'E') => self.filter[usize::from((cmd >> 4) - 8)] = cmd & 0x0F,
            _ => {}
        }
        false
    }

    /// The configuration answer on $1B/$1C, None outside configuration mode.
    fn read(&self, reg: u8) -> Option<u8> {
        match reg {
            0x1B | 0x1C if self.config => Some(self.answer[usize::from(reg - 0x1B)]),
            _ => None,
        }
    }
}

/// One receiver's reSID, and registers $00-$18 as written, replayed into a rebuilt engine.
struct Engine {
    resid: Box<Resid>,
    model: i32,
    regs: [u8; 0x19],
}

impl Engine {
    /// TRX64's `ResidConfig` is fixed at construction, so a model or rate change builds the engine again; oscillator and
    /// envelope phase restart. A clock change does not (`Engines::set_clock`).
    fn build(model: i32, rate: u32, clock_hz: u64, sampling_method: i32, regs: [u8; 0x19]) -> Self {
        let rate = f64::from(rate);
        let mut resid = Resid::new(ResidConfig {
            model,
            clock_freq: clock_hz as f64,
            sample_rate: rate,
            sampling_method,
            passband: rate * 90.0 / 200.0,
            filter: true,
            ..ResidConfig::default()
        });
        for (reg, &val) in regs.iter().enumerate() {
            resid.write(reg as u8, val);
        }
        Engine { resid: Box::new(resid), model, regs }
    }
}

/// How [`Sid::drain`] clocks the engines before each queued write.
#[derive(Clone, Copy)]
enum Clocking {
    /// Registers only.
    Untimed,
    /// To the write's own traced cycle.
    Traced,
    /// To this cycle, once, for all of them.
    At(u64),
}

/// The engines, the gains and the sink: everything that turns writes and cycles into samples (S20 §1). It runs on the
/// emulation thread, or on the SID worker when a device listens.
struct Engines {
    engines: [Option<Engine>; RECEIVERS],
    /// Receivers with an engine, in the order the engines were built. The first one's sample count is each chunk's.
    order: Vec<usize>,
    sample_rate: u32,
    /// Audio quality is local to the consumer; it never changes the CPU readback shadow.
    sampling_method: i32,
    /// The C64's clock, Hz: the machine's row (S25 §3).
    clock_hz: u64,
    /// C64 cycle the engines have been clocked to.
    clk: u64,
    /// Cycles × sample rate not yet turned into silence while no engine runs.
    pace: u64,
    /// Left and right gain per receiver in 1/[`SIDE_UNITY`] steps.
    gains: [(i32, i32); RECEIVERS],
    audio: Option<Box<dyn AudioSink>>,
    /// FPGA audio stream tap. Samples land here as well as in a sink, and its presence alone makes
    /// the engines run, so the stream works without `--audio`. Capped: a host that stops draining loses the
    /// oldest samples rather than growing this without bound.
    stream: Option<Arc<Mutex<Vec<i16>>>>,
    /// $1B/$1C per receiver as of the last clocking, bit 16 set where an engine answers. Cached rather than read on
    /// demand: a reSID read of $1B/$1C sets the bus value the write-only registers read back (sid.cc `read`), so it
    /// may only happen where it did before S20 — right after clocking, never after a write.
    cache: [u32; RECEIVERS],
    /// CPU-side digital shadow: advance oscillators/envelopes, skip analogue filter and PCM.
    digital_only: bool,
    mono_scratch: Vec<i16>,
    mix_scratch: Vec<i32>,
    output_scratch: Vec<i16>,
    /// Portable renderer output. No callback or shared-memory requirement in a browser worker.
    captured: Option<Vec<i16>>,
}

impl Engines {
    fn new(sample_rate: u32) -> Self {
        Engines {
            engines: Default::default(),
            order: Vec::new(),
            sample_rate,
            sampling_method: ResidConfig::default().sampling_method,
            clock_hz: CLOCK_HZ,
            clk: 0,
            pace: 0,
            gains: [(SIDE_UNITY, SIDE_UNITY); RECEIVERS],
            audio: None,
            stream: None,
            cache: [0; RECEIVERS],
            digital_only: false,
            mono_scratch: Vec::new(),
            mix_scratch: Vec::new(),
            output_scratch: Vec::new(),
            captured: None,
        }
    }

    fn listening(&self) -> bool {
        self.audio.is_some() || self.stream.is_some() || self.captured.is_some()
    }

    /// Build receiver `rx`'s engine, or rebuild it with its registers when its model changed.
    fn ensure(&mut self, rx: usize, model: i32) {
        match self.engines[rx].as_ref().map(|e| (e.model, e.regs)) {
            Some((built, _)) if built == model => {}
            Some((_, regs)) => self.engines[rx] = Some(Engine::build(model, self.sample_rate, self.clock_hz, self.sampling_method, regs)),
            None => {
                self.engines[rx] = Some(Engine::build(model, self.sample_rate, self.clock_hz, self.sampling_method, [0; 0x19]));
                self.order.push(rx);
            }
        }
        self.cache[rx] = 1 << 16;
    }

    fn drop_engine(&mut self, rx: usize) {
        self.engines[rx] = None;
        self.order.retain(|&r| r != rx);
        self.cache[rx] = 0;
    }

    /// One traced write to every receiver in `mask`.
    fn write(&mut self, mask: u16, reg: u8, val: u8) {
        for rx in (0..RECEIVERS).filter(|&rx| mask & bit(rx) != 0) {
            if let Some(e) = self.engines[rx].as_mut() {
                if let Some(r) = e.regs.get_mut(usize::from(reg)) {
                    *r = val;
                }
                e.resid.write(reg, val);
            }
        }
    }

    /// Clock every engine to cycle `clk` with the same deltas: all of it into the sink, or at most
    /// [`SILENT_CATCH_UP`] cycles without one.
    fn clock_to(&mut self, clk: u64) {
        if clk <= self.clk {
            return;
        }
        let mut delta = clk - self.clk;
        self.clk = clk;
        let listening = self.listening();
        if !listening {
            delta = delta.min(SILENT_CATCH_UP);
        }
        // Always the sampled (per-cycle) clock, samples discarded without a sink: reSID's batch `clock(delta)`
        // (`Resid::clock_silent`) leaves the ENV3 latch alone (envelope.h:118) and, mixed with sampled clocking on
        // one engine, froze the envelope counter (ENV3 70 for 20 ms of attack 0; S14 §W4-SID).
        while delta > 0 {
            let n = delta.min(CHUNK);
            delta -= n;
            self.mix_scratch.clear();
            let mut digital_frames = None;
            for i in 0..self.order.len() {
                let rx = self.order[i];
                let engine = self.engines[rx].as_mut().expect("ordered engines exist");
                if self.digital_only {
                    engine.resid.clock_readback(n as u32);
                    let frames = engine.resid.clock_sample_count(n as u32);
                    if i == 0 { digital_frames = Some(frames); }
                } else {
                    self.mono_scratch.clear();
                    engine.resid.emit_into(n as u32, &mut self.mono_scratch);
                    if listening {
                        if i == 0 {
                            self.mix_scratch.resize(2 * self.mono_scratch.len(), 0);
                        }
                        mix_into(&mut self.mix_scratch, &self.mono_scratch, self.gains[rx]);
                    }
                }
            }
            if !listening {
                continue;
            }
            self.output_scratch.clear();
            if self.digital_only || self.order.is_empty() {
                let frames = digital_frames.unwrap_or_else(|| self.silence(n));
                self.output_scratch.resize(2 * frames, 0);
            } else {
                self.output_scratch.extend(self.mix_scratch.iter().map(|&s|
                    (s / SIDE_UNITY).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16));
            }
            let pcm = &self.output_scratch;
            if let Some(sink) = &mut self.audio {
                sink.samples(pcm);
            }
            if let Some(captured) = &mut self.captured {
                captured.extend_from_slice(pcm);
            }
            if let Some(tap) = &self.stream {
                if let Ok(mut buf) = tap.lock() {
                    buf.extend_from_slice(pcm);
                    let over = buf.len().saturating_sub(STREAM_TAP_CAP).next_multiple_of(2);
                    if over > 0 {
                        buf.drain(..over);
                    }
                }
            }
        }
        self.cache_readback();
    }

    /// Refresh [`Engines::cache`] from the engines.
    fn cache_readback(&mut self) {
        for rx in 0..RECEIVERS {
            self.cache[rx] = self.engines[rx]
                .as_ref()
                .map_or(0, |e| 1 << 16 | u32::from(e.resid.read(0x1B)) << 8 | u32::from(e.resid.read(0x1C)));
        }
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
                Cmd::Ensure { rx, model } => self.ensure(rx, model),
                Cmd::Drop(rx) => self.drop_engine(rx),
                Cmd::Write { at, mask, reg, val } => {
                    if let Some(at) = at {
                        self.clock_to(at);
                    }
                    self.write(mask, reg, val);
                }
                Cmd::ClockTo(clk) => self.clock_to(clk),
                Cmd::Gains(gains) => self.gains = gains,
                Cmd::StreamTap(tap) => self.stream = tap,
                Cmd::Reset => self.reset(),
                Cmd::Reanchor(clk) => { self.clk = clk; self.pace = 0; },
                Cmd::ClockFreq(hz) => self.set_clock(hz),
        }
    }

    /// Samples in `n` cycles while no engine runs, at reSID's cadence.
    fn silence(&mut self, n: u64) -> usize {
        self.pace += n * u64::from(self.sample_rate);
        let samples = self.pace / self.clock_hz;
        self.pace %= self.clock_hz;
        samples as usize
    }

    fn set_rate(&mut self, sample_rate: u32) {
        if sample_rate == self.sample_rate {
            return;
        }
        self.sample_rate = sample_rate;
        for e in self.engines.iter_mut().flatten() {
            *e = Engine::build(e.model, sample_rate, self.clock_hz, self.sampling_method, e.regs);
        }
    }

    /// The C64 runs on another clock (a model switch, S25 §3): reSID re-samples at it, the oscillators run on.
    fn set_clock(&mut self, clock_hz: u64) {
        if clock_hz == self.clock_hz {
            return;
        }
        self.clock_hz = clock_hz;
        self.pace = 0;
        for e in self.engines.iter_mut().flatten() {
            e.resid.set_clock_freq(clock_hz as f64);
        }
    }

    /// The C64 reset line: every engine clears its registers.
    fn reset(&mut self) {
        for e in self.engines.iter_mut().flatten() {
            e.regs = [0; 0x19];
            e.resid.reset();
        }
        self.cache_readback();
    }

    /// The first receiver in `mask` that has an engine.
    fn first(&self, mask: u16) -> Option<&Engine> {
        (0..RECEIVERS).filter(|&r| mask & bit(r) != 0).find_map(|r| self.engines[r].as_ref())
    }

    fn read(&self, mask: u16, reg: u8) -> u8 {
        self.first(mask).map_or(0, |e| e.resid.read(reg))
    }

}

/// What the emulation thread asks the engines to do, in order (S20 §2).
enum Cmd {
    Ensure { rx: usize, model: i32 },
    Drop(usize),
    /// Clock to `at` first when it is given (the write's traced cycle), then write.
    Write { at: Option<u64>, mask: u16, reg: u8, val: u8 },
    ClockTo(u64),
    Gains([(i32, i32); RECEIVERS]),
    /// Install or remove the FPGA audio stream's tap.
    StreamTap(Option<Arc<Mutex<Vec<i16>>>>),
    Reset,
    Reanchor(u64),
    /// The C64's clock in Hz changed.
    ClockFreq(u64),
}

/// Inline audio, or local digital shadow plus a portable audio worker.
enum Host {
    Inline(Engines),
    External(External),
}

/// Browser worker command. Its wire format is four little-endian u32 words:
/// `kind | (register << 8) | (receiver_mask << 16)`, value, cycle low, cycle high.
/// Writes occur at the absolute PAL/NTSC cycle before the register is changed.
/// The final ClockTo event advances the audio through the end of the CPU slice.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SidEvent {
    pub header: u32,
    pub value: u32,
    pub cycle_lo: u32,
    pub cycle_hi: u32,
}

// The portable wire ABI has exactly four u32 words and no padding. This lets
// the little-endian WASM producer expose its typed FIFO directly as bytes.
const _: () = assert!(std::mem::size_of::<SidEvent>() == 16);
const _: () = assert!(std::mem::align_of::<SidEvent>() == 4);

impl SidEvent {
    pub const ENSURE: u8 = 1;
    pub const DROP: u8 = 2;
    pub const WRITE: u8 = 3;
    pub const CLOCK: u8 = 4;
    pub const GAINS: u8 = 5;
    pub const RESET: u8 = 6;
    pub const REANCHOR: u8 = 7;
    pub const CLOCK_HZ: u8 = 8;

    pub const fn new(kind: u8, reg: u8, mask: u16, value: u32, cycle: u64) -> Self {
        Self { header: kind as u32 | ((reg as u32) << 8) | ((mask as u32) << 16),
            value, cycle_lo: cycle as u32, cycle_hi: (cycle >> 32) as u32 }
    }
    pub const fn from_words(words: [u32; 4]) -> Self {
        Self { header: words[0], value: words[1], cycle_lo: words[2], cycle_hi: words[3] }
    }
    pub const fn words(self) -> [u32; 4] { [self.header, self.value, self.cycle_lo, self.cycle_hi] }
    pub fn from_le_bytes(bytes: [u8; 16]) -> Self {
        Self::from_words(std::array::from_fn(|i| u32::from_le_bytes(bytes[4*i..4*i+4].try_into().unwrap())))
    }
    pub fn to_le_bytes(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        for (dst, value) in bytes.chunks_exact_mut(4).zip(self.words()) { dst.copy_from_slice(&value.to_le_bytes()); }
        bytes
    }
    pub const fn kind(self) -> u8 { self.header as u8 }
    pub const fn reg(self) -> u8 { (self.header >> 8) as u8 }
    pub const fn mask(self) -> u16 { (self.header >> 16) as u16 }
    pub const fn cycle(self) -> u64 { self.cycle_lo as u64 | ((self.cycle_hi as u64) << 32) }
}

/// FIFO event producer and local readback shadow. No CPU read waits for the audio worker:
/// the shadow clocks the SID digital state at exactly the same event timestamps.
struct External {
    shadow: Engines,
    events: Vec<SidEvent>,
}

impl External {
    fn command(&mut self, cmd: Cmd) {
        let cycle = self.shadow.clk;
        let event = match &cmd {
            Cmd::Ensure { rx, model } => Some(SidEvent::new(SidEvent::ENSURE, 0, bit(*rx), *model as u32, cycle)),
            Cmd::Drop(rx) => Some(SidEvent::new(SidEvent::DROP, 0, bit(*rx), 0, cycle)),
            Cmd::Write { at, mask, reg, val } => Some(SidEvent::new(SidEvent::WRITE, *reg, *mask, u32::from(*val), at.unwrap_or(cycle))),
            Cmd::ClockTo(clk) => Some(SidEvent::new(SidEvent::CLOCK, 0, 0, 0, *clk)),
            Cmd::Gains(gains) => {
                for (rx, &(left, right)) in gains.iter().enumerate() {
                    self.events.push(SidEvent::new(SidEvent::GAINS, 0, bit(rx), left as u32 | ((right as u32) << 16), cycle));
                }
                None
            }
            Cmd::Reset => Some(SidEvent::new(SidEvent::RESET, 0, 0, 0, cycle)),
            Cmd::Reanchor(clk) => Some(SidEvent::new(SidEvent::REANCHOR, 0, 0, 0, *clk)),
            Cmd::ClockFreq(hz) => Some(SidEvent::new(SidEvent::CLOCK_HZ, 0, 0, *hz as u32, cycle)),
            // Stream taps and synchronous reads belong to the CPU shadow only.
            Cmd::StreamTap(_) => None,
        };
        if let Some(event) = event { self.events.push(event); }
        self.shadow.command(cmd);
    }
}

/// Pure Rust SID/audio consumer, suitable for a separate wasm instance in a Web Worker.
/// No assets, CPU, VIC, filesystem, atomics or pthread-enabled Rust standard library are needed.
pub struct SidRenderer {
    engines: Engines,
}

/// Experimental per-receiver audio renderer. Each consumer receives the entire
/// event stream, but constructs reSID engines only for its assigned receivers.
/// Contributions remain in mixer gain units until the final, common mixer.
pub struct SidPartitionRenderer {
    mask: u16,
    engines: [Option<Engine>; RECEIVERS],
    cadence: [Option<PartitionCadence>; RECEIVERS],
    order: Vec<usize>,
    gains: [(i32, i32); RECEIVERS],
    sample_rate: u32,
    sampling_method: i32,
    clock_hz: u64,
    clk: u64,
    pace: u64,
    mono_scratch: Vec<i16>,
}

/// Exact non-FAST reSID 16.16 sample clock, without oscillator/filter tables.
/// Model/rate rebuilds and clock changes reset this clock just as reSID does.
#[derive(Clone, Copy)]
struct PartitionCadence {
    model: i32,
    cycles_per_sample: u32,
    offset: i32,
}

impl PartitionCadence {
    fn new(model: i32, rate: u32, clock_hz: u64) -> Self {
        Self { model, cycles_per_sample: (clock_hz as f64 / f64::from(rate) * 65_536.0 + 0.5) as u32, offset: 0 }
    }

    fn count(&mut self, cycles: u64) -> usize {
        let elapsed = (cycles as i64) << 16;
        let count = ((elapsed - 1 - i64::from(self.offset)) / i64::from(self.cycles_per_sample)).max(0);
        self.offset = (i64::from(self.offset) + count * i64::from(self.cycles_per_sample) - elapsed) as i32;
        count as usize
    }
}

impl SidPartitionRenderer {
    /// A coordinator can follow global timing before any SID receives a write.
    /// Select its one local receiver before processing that receiver's Ensure.
    pub fn new_metadata(sample_rate: u32) -> Result<Self, String> {
        Self::with_sampling(sample_rate, 0, SAMPLE_INTERPOLATE)
    }

    pub fn new_fast(sample_rate: u32, mask: u16) -> Result<Self, String> {
        if mask == 0 { return Err("SID partition requires a receiver mask".into()); }
        Self::with_sampling(sample_rate, mask, SAMPLE_INTERPOLATE)
    }

    /// FIR reference uses the same partition/mixer protocol for parity checks.
    pub fn new(sample_rate: u32, mask: u16) -> Result<Self, String> {
        if mask == 0 { return Err("SID partition requires a receiver mask".into()); }
        Self::with_sampling(sample_rate, mask, ResidConfig::default().sampling_method)
    }

    fn with_sampling(sample_rate: u32, mask: u16, sampling_method: i32) -> Result<Self, String> {
        if !(8_000..=96_000).contains(&sample_rate) || mask & !((1 << RECEIVERS) - 1) != 0 {
            return Err("Invalid SID partition rate or receiver mask".into());
        }
        Ok(Self { mask, engines: Default::default(), cadence: [None; RECEIVERS], order: Vec::new(),
            gains: [(SIDE_UNITY, SIDE_UNITY); RECEIVERS], sample_rate, sampling_method,
            clock_hz: CLOCK_HZ, clk: 0, pace: 0, mono_scratch: Vec::new() })
    }

    pub fn select_receiver(&mut self, mask: u16) -> Result<(), String> {
        if mask.count_ones() != 1 || mask & !((1 << RECEIVERS) - 1) != 0 {
            return Err("SID partition must select exactly one receiver".into());
        }
        if self.mask == mask { return Ok(()); }
        let rx = mask.trailing_zeros() as usize;
        if self.mask != 0 || self.engines.iter().any(Option::is_some) || self.cadence[rx].is_some() {
            return Err("Select a new SID before its first Ensure; existing SID phase cannot be recreated from metadata".into());
        }
        self.mask = mask;
        Ok(())
    }

    /// Fixed little-endian global metadata ABI, deliberately excluding every
    /// chip's oscillator/envelope/filter state. Import is safe only for a newly
    /// created receiver, at the boundary BEFORE its first Ensure batch.
    pub const SNAPSHOT_BYTES: usize = 64 + RECEIVERS * 20;

    pub fn export_metadata(&self, out: &mut Vec<u8>) {
        out.clear();
        out.resize(Self::SNAPSHOT_BYTES, 0);
        let word = |out: &mut [u8], offset: usize, value: u32| out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        out[..4].copy_from_slice(b"SDP1");
        word(out, 4, 1);
        word(out, 8, self.sample_rate);
        word(out, 12, self.sampling_method as u32);
        word(out, 16, self.clk as u32);
        word(out, 20, (self.clk >> 32) as u32);
        word(out, 24, self.clock_hz as u32);
        word(out, 28, (self.clock_hz >> 32) as u32);
        word(out, 32, self.pace as u32);
        word(out, 36, (self.pace >> 32) as u32);
        word(out, 40, self.order.len() as u32);
        for (i, &rx) in self.order.iter().enumerate() { out[48 + i] = rx as u8; }
        for rx in 0..RECEIVERS {
            let offset = 64 + rx * 20;
            word(out, offset, self.cadence[rx].map_or(u32::MAX, |cadence| cadence.model as u32));
            word(out, offset + 4, self.gains[rx].0 as u32);
            word(out, offset + 8, self.gains[rx].1 as u32);
            word(out, offset + 12, self.cadence[rx].map_or(0, |cadence| cadence.cycles_per_sample));
            word(out, offset + 16, self.cadence[rx].map_or(0, |cadence| cadence.offset as u32));
        }
    }

    /// Validate the entire snapshot before modifying anything. Existing local
    /// synthesis cannot be overwritten: no analog phase is present in SDP1.
    pub fn import_metadata(&mut self, bytes: &[u8]) -> Result<(), String> {
        let invalid = || "Invalid SID partition metadata snapshot".to_string();
        if bytes.len() != Self::SNAPSHOT_BYTES || &bytes[..4] != b"SDP1" { return Err(invalid()); }
        let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let pair = |offset: usize| u64::from(word(offset)) | (u64::from(word(offset + 4)) << 32);
        let rate = word(8);
        let method = word(12) as i32;
        let hz = pair(24);
        let pace = pair(32);
        let len = word(40) as usize;
        if word(4) != 1 || word(44) != 0 || bytes[57..64].iter().any(|&byte| byte != 0)
            || !(8_000..=96_000).contains(&rate) || !(100_000..=2_000_000).contains(&hz)
            || hz < u64::from(rate) || hz as f64 / f64::from(rate) * 125.0 >= 16_384.0
            || ![SAMPLE_INTERPOLATE, ResidConfig::default().sampling_method].contains(&method)
            || pace >= hz || len > RECEIVERS || bytes[48 + len..57].iter().any(|&byte| byte != 0) {
            return Err(invalid());
        }
        let mut order = Vec::with_capacity(len);
        let mut active = 0u16;
        for &rx in &bytes[48..48 + len] {
            let rx = usize::from(rx);
            if rx >= RECEIVERS || active & bit(rx) != 0 { return Err(invalid()); }
            order.push(rx);
            active |= bit(rx);
        }
        let mut cadence = [None; RECEIVERS];
        let mut gains = [(0, 0); RECEIVERS];
        for rx in 0..RECEIVERS {
            let offset = 64 + rx * 20;
            let model = word(offset);
            let left = word(offset + 4);
            let right = word(offset + 8);
            let cycles = word(offset + 12);
            let phase = word(offset + 16) as i32;
            if left > 255 || right > 255 { return Err(invalid()); }
            gains[rx] = (left as i32, right as i32);
            if active & bit(rx) == 0 {
                if model != u32::MAX || cycles != 0 || phase != 0 { return Err(invalid()); }
            } else {
                let expected = PartitionCadence::new(model as i32, rate, hz);
                if model > MODEL_8580 as u32 || cycles != expected.cycles_per_sample
                    || phase > 0 || i64::from(phase) < -i64::from(cycles) { return Err(invalid()); }
                cadence[rx] = Some(PartitionCadence { offset: phase, ..expected });
            }
        }
        if active & self.mask != 0 || self.engines.iter().any(Option::is_some) {
            return Err("SID metadata cannot restore an existing receiver's synthesis phase".into());
        }
        self.sample_rate = rate;
        self.sampling_method = method;
        self.clock_hz = hz;
        self.clk = pair(16);
        self.pace = pace;
        self.order = order;
        self.cadence = cadence;
        self.gains = gains;
        Ok(())
    }

    fn ensure(&mut self, rx: usize, model: i32) {
        match self.cadence[rx] {
            Some(cadence) if cadence.model == model => return,
            None => self.order.push(rx),
            Some(_) => {},
        }
        self.cadence[rx] = Some(PartitionCadence::new(model, self.sample_rate, self.clock_hz));
        if self.mask & bit(rx) != 0 {
            let regs = self.engines[rx].as_ref().map_or([0; 0x19], |engine| engine.regs);
            self.engines[rx] = Some(Engine::build(model, self.sample_rate, self.clock_hz, self.sampling_method, regs));
        }
    }

    fn clock_to(&mut self, clk: u64, out: &mut Vec<i32>) {
        if clk <= self.clk { return; }
        let mut delta = clk - self.clk;
        self.clk = clk;
        while delta != 0 {
            let chunk = delta.min(CHUNK);
            delta -= chunk;
            let mut frames = None;
            for &rx in &self.order {
                let count = self.cadence[rx].as_mut().expect("ordered cadence exists").count(chunk);
                if frames.is_none() { frames = Some(count); }
            }
            let frames = frames.unwrap_or_else(|| {
                self.pace += chunk * u64::from(self.sample_rate);
                let frames = self.pace / self.clock_hz;
                self.pace %= self.clock_hz;
                frames as usize
            });
            let start = out.len();
            out.resize(start + 2 * frames, 0);
            // Preserve the combined renderer's pad/trim at EACH clock chunk,
            // including boundaries introduced by writes to another receiver.
            for &rx in &self.order {
                let Some(engine) = self.engines[rx].as_mut() else { continue; };
                self.mono_scratch.clear();
                engine.resid.emit_into(chunk as u32, &mut self.mono_scratch);
                mix_into(&mut out[start..], &self.mono_scratch, self.gains[rx]);
            }
        }
        // Readback caching changes the SID bus latch. Match its original place
        // after the complete clock gap, although CPU reads use their own shadow.
        for engine in self.engines.iter().flatten() {
            engine.resid.read(0x1B);
            engine.resid.read(0x1C);
        }
    }

    pub fn render_into(&mut self, events: &[SidEvent], out: &mut Vec<i32>) -> Result<(), String> {
        validate_events(events, self.clk, self.clock_hz)?;
        out.clear();
        for &event in events {
            let rx = event.mask().trailing_zeros() as usize;
            match event.kind() {
                SidEvent::ENSURE => self.ensure(rx, event.value as i32),
                SidEvent::DROP => {
                    self.engines[rx] = None;
                    self.cadence[rx] = None;
                    self.order.retain(|&receiver| receiver != rx);
                },
                SidEvent::WRITE => {
                    self.clock_to(event.cycle(), out);
                    for rx in 0..RECEIVERS {
                        if event.mask() & bit(rx) == 0 { continue; }
                        if let Some(engine) = &mut self.engines[rx] {
                            if let Some(reg) = engine.regs.get_mut(usize::from(event.reg())) { *reg = event.value as u8; }
                            engine.resid.write(event.reg(), event.value as u8);
                        }
                    }
                },
                SidEvent::CLOCK => self.clock_to(event.cycle(), out),
                SidEvent::GAINS => self.gains[rx] = ((event.value & 0xffff) as i32, (event.value >> 16) as i32),
                SidEvent::RESET => {
                    for engine in self.engines.iter_mut().flatten() { engine.regs = [0; 0x19]; engine.resid.reset(); }
                    for cadence in self.cadence.iter_mut().flatten() { cadence.offset = 0; }
                },
                SidEvent::REANCHOR => { self.clk = event.cycle(); self.pace = 0; },
                SidEvent::CLOCK_HZ => {
                    let hz = u64::from(event.value);
                    if hz != self.clock_hz {
                        self.clock_hz = hz;
                        self.pace = 0;
                        for engine in self.engines.iter_mut().flatten() { engine.resid.set_clock_freq(hz as f64); }
                        for cadence in self.cadence.iter_mut().flatten() {
                            *cadence = PartitionCadence::new(cadence.model, self.sample_rate, hz);
                        }
                    }
                },
                _ => unreachable!("validated above"),
            }
        }
        Ok(())
    }

    pub fn render(&mut self, events: &[SidEvent]) -> Result<Vec<i32>, String> {
        let mut out = Vec::new();
        self.render_into(events, &mut out)?;
        Ok(out)
    }

    /// Rebuild local engines at the new output rate, preserving registers and
    /// global insertion order exactly like the combined renderer.
    pub fn set_rate(&mut self, sample_rate: u32) -> Result<(), String> {
        if !(8_000..=96_000).contains(&sample_rate) { return Err("Invalid SID partition rate".into()); }
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            for engine in self.engines.iter_mut().flatten() {
                *engine = Engine::build(engine.model, sample_rate, self.clock_hz, self.sampling_method, engine.regs);
            }
            for cadence in self.cadence.iter_mut().flatten() {
                *cadence = PartitionCadence::new(cadence.model, sample_rate, self.clock_hz);
            }
        }
        Ok(())
    }
}

fn validate_events(events: &[SidEvent], mut clk: u64, mut hz: u64) -> Result<(), String> {
    for &event in events {
        let mask = event.mask();
        if mask & !((1 << RECEIVERS) - 1) != 0 { return Err("SID event has an invalid receiver mask".into()); }
        match event.kind() {
            SidEvent::ENSURE if mask.count_ones() == 1 && event.value <= MODEL_8580 as u32 => {},
            SidEvent::DROP if mask.count_ones() == 1 => {},
            SidEvent::GAINS if mask.count_ones() == 1 && (event.value & 0xffff) <= 255 && (event.value >> 16) <= 255 => {},
            SidEvent::WRITE | SidEvent::CLOCK => {
                let at = event.cycle();
                if event.kind() == SidEvent::WRITE && (event.reg() > 31 || event.value > 255) {
                    return Err("SID event has an invalid register write".into());
                }
                if at < clk || at - clk > hz * 2 { return Err("SID event clock regressed or exceeded a two-second slice".into()); }
                clk = at;
            },
            SidEvent::RESET => {},
            SidEvent::REANCHOR => clk = event.cycle(),
            SidEvent::CLOCK_HZ if (100_000..=2_000_000).contains(&event.value) => hz = u64::from(event.value),
            _ => return Err("SID event kind or payload is invalid".into()),
        }
    }
    Ok(())
}

impl SidRenderer {
    /// Full FIR resampling, retained for reference/oracle comparisons.
    pub fn new(sample_rate: u32) -> Self {
        let mut engines = Engines::new(sample_rate);
        engines.captured = Some(Vec::new());
        Self { engines }
    }

    /// Lighter browser audio: linear interpolation without FIR coefficients,
    /// ring writes or convolutions. Digital SID units, analog filters, timestamped
    /// writes and sample cadence are unchanged; high-frequency alias suppression
    /// is weaker than with `new`. Deliberately uses INTERPOLATE rather than FAST:
    /// FAST changes sample boundaries and the MOS8580 register-write pipeline.
    pub fn new_fast(sample_rate: u32) -> Self {
        let mut renderer = Self::new(sample_rate);
        renderer.engines.sampling_method = SAMPLE_INTERPOLATE;
        renderer
    }

    /// Validate the complete batch before changing synthesis state. Monotonic writes,
    /// receiver masks and bounded clock gaps protect against corrupt or out-of-order batches.
    pub fn render_into(&mut self, events: &[SidEvent], out: &mut Vec<i16>) -> Result<(), String> {
        validate_events(events, self.engines.clk, self.engines.clock_hz)?;
        out.clear();
        self.engines.captured = Some(std::mem::take(out));
        for &event in events {
            let rx = event.mask().trailing_zeros() as usize;
            match event.kind() {
                SidEvent::ENSURE => self.engines.ensure(rx, event.value as i32),
                SidEvent::DROP => self.engines.drop_engine(rx),
                SidEvent::WRITE => {
                    self.engines.clock_to(event.cycle());
                    self.engines.write(event.mask(), event.reg(), event.value as u8);
                },
                SidEvent::CLOCK => self.engines.clock_to(event.cycle()),
                SidEvent::GAINS => self.engines.gains[rx] = ((event.value & 0xffff) as i32, (event.value >> 16) as i32),
                SidEvent::RESET => self.engines.reset(),
                SidEvent::REANCHOR => { self.engines.clk = event.cycle(); self.engines.pace = 0; },
                SidEvent::CLOCK_HZ => self.engines.set_clock(u64::from(event.value)),
                _ => unreachable!("validated above"),
            }
        }
        *out = self.engines.captured.take().expect("capture installed above");
        self.engines.captured = Some(Vec::new());
        Ok(())
    }

    pub fn render(&mut self, events: &[SidEvent]) -> Result<Vec<i16>, String> {
        let mut out = Vec::new();
        self.render_into(events, &mut out)?;
        Ok(out)
    }
}

/// The SIDs behind the U64's decoders, their reSID engines and the mixer.
pub struct Sid {
    decode: Decode,
    /// Socket 1 holds the ARMSID; else it is empty and never decodes, whatever C64_SID1_EN says.
    socket1: bool,
    /// Receivers of each TRX64 chip.
    groups: Vec<u16>,
    /// The table TRX64 routes by (855 D2), and whether it changed since [`Sid::take_map`].
    map: Vec<SidMapping>,
    map_changed: bool,
    /// The model of every engine that exists, mirrored here because the engines may be on the worker.
    built: [Option<i32>; RECEIVERS],
    /// The registers written to every engine, for a debugger's peek.
    regs: [[u8; 0x19]; RECEIVERS],
    mixer: [u8; MIXER_BYTES],
    /// Whether a sink listens: without one the engines are not clocked to the C64.
    listening: bool,
    host: Host,
}

impl Sid {
    /// Unused rate while no sink is attached (reSID still needs one to be configured).
    const SILENT_RATE: u32 = 44_100;

    pub fn new() -> Self {
        DOOR.with(|door| *door.borrow_mut() = Door::new());
        let mut sid = Sid {
            decode: Decode::default(),
            socket1: false,
            groups: Vec::new(),
            map: Vec::new(),
            map_changed: false,
            built: [None; RECEIVERS],
            regs: [[0; 0x19]; RECEIVERS],
            mixer: MIXER_DEFAULT,
            listening: false,
            host: Host::Inline(Engines::new(Self::SILENT_RATE)),
        };
        sid.route();
        sid
    }

    /// Hook into `m`: the decode table, TRX64's write trace and its host door. TRX64's resets keep all three (855 D2).
    pub fn install(&mut self, m: &mut Machine) {
        m.set_sid_write_trace(Some(Box::new(traced)));
        m.set_sid_host_access(Some(Box::new(answered)), Some(Box::new(answered)));
        m.set_sid_map(self.map.clone());
        self.map_changed = false;
    }

    /// The decode table for `Machine::set_sid_map` when it changed since the last call.
    pub fn take_map(&mut self) -> Option<Vec<SidMapping>> {
        std::mem::take(&mut self.map_changed).then(|| self.map.clone())
    }

    /// Fit (`true`) or empty socket 1. The firmware detects a fitted ARMSID at its next boot. Changes the table.
    pub fn set_socket1(&mut self, fitted: bool) {
        self.drain(self.clocking());
        self.socket1 = fitted;
        if !fitted && self.built[SOCKET1_RX].take().is_some() {
            self.command(Cmd::Drop(SOCKET1_RX));
        }
        self.route();
    }

    /// Install or remove the FPGA stream tap. A listening sink fixes the cadence;
    /// otherwise the tap enables sample generation at its own rate.
    pub fn set_stream_tap(&mut self, tap: Option<Arc<Mutex<Vec<i16>>>>, sample_rate: u32, clk: u64) {
        let has_sink = match &self.host {
            Host::Inline(engines) => engines.audio.is_some(),
            Host::External(external) => external.shadow.audio.is_some(),
        };
        let on = tap.is_some();
        if on && !has_sink {
            let engines = self.engines_for_audio(clk);
            engines.set_rate(sample_rate);
        }
        self.command(Cmd::StreamTap(tap));
        // `listening` is what makes the writes traced and the engines clocked with the C64 rather than lazily, so
        // the tap needs it as a sink does — and giving it up again is only safe while no sink wants it.
        if on || has_sink {
            self.listening = true;
        } else {
            self.listening = false;
        }
        self.gains();
        self.refresh_readback();
    }

    /// Send samples at `sample_rate` Hz to `sink` from cycle `clk` on, from this thread. Without a sink the engines
    /// lag the C64, so they first catch up silently: the stream starts at `clk`, not with the backlog.
    pub fn set_audio(&mut self, sample_rate: u32, sink: Box<dyn AudioSink>, clk: u64) {
        let engines = self.engines_for_audio(clk);
        engines.set_rate(sample_rate);
        engines.audio = Some(sink);
        self.listening = true;
        self.gains();
        self.refresh_readback();
    }

    /// Move analogue synthesis to the host's portable worker. The local sink receives
    /// silence at the sample cadence, allowing Ultimate PCM voices to be mixed locally.
    /// Installing this mode restarts synthesis phases; attach it before starting the guest.
    pub fn set_audio_external(&mut self, sample_rate: u32, sink: Box<dyn AudioSink>, clk: u64) {
        self.drain(Clocking::Untimed);
        assert!(matches!(self.host, Host::Inline(_)), "audio host already installed");
        let clock_hz = match &self.host { Host::Inline(engines) => engines.clock_hz, _ => unreachable!() };
        let mut external = External { shadow: Engines::new(sample_rate), events: Vec::with_capacity(256) };
        external.shadow.digital_only = true;
        external.shadow.audio = Some(sink);
        external.command(Cmd::Reanchor(clk));
        external.command(Cmd::ClockFreq(clock_hz));
        for rx in 0..RECEIVERS {
            if let Some(model) = self.built[rx] {
                external.command(Cmd::Ensure { rx, model });
                for (reg, &val) in self.regs[rx].iter().enumerate() {
                    external.command(Cmd::Write { at: Some(clk), mask: bit(rx), reg: reg as u8, val });
                }
            }
        }
        self.host = Host::External(external);
        self.listening = true;
        self.gains();
        self.refresh_readback();
    }

    /// Drain commands for the independent audio renderer, preserving FIFO order.
    /// Call once per CPU slice and deliver every batch; never discard SID writes.
    pub fn take_events(&mut self) -> Vec<SidEvent> {
        match &mut self.host {
            Host::External(external) => std::mem::take(&mut external.events),
            _ => Vec::new(),
        }
    }

    /// Drain the portable FIFO into a reusable caller-owned buffer. The two
    /// allocations trade ownership; neither the producer nor consumer copies
    /// the events or loses capacity between audio batches.
    pub fn drain_events_into(&mut self, out: &mut Vec<SidEvent>) {
        out.clear();
        if let Host::External(external) = &mut self.host {
            std::mem::swap(out, &mut external.events);
            external.events.clear();
        }
    }

    /// Catch the engines up to `clk` before a sink is attached, and hand them out for it.
    fn engines_for_audio(&mut self, clk: u64) -> &mut Engines {
        self.drain(self.clocking());
        let engines = match &mut self.host {
            Host::Inline(engines) => engines,
            Host::External(_) => panic!("the portable SID renderer is already installed"),
        };
        engines.clock_to(clk);
        engines.clk = engines.clk.max(clk);
        engines
    }

    /// The observer for a CPU run. Writes come from TRX64's trace now (S17 §2.3); it stays so the run calls do not
    /// change.
    /// What the firmware has built and where it routes, for the monitor's `audio` verb (S23 §5): socket 1, the
    /// engines that exist with their model, and the address windows TRX64 routes by.
    pub fn summary(&self) -> String {
        let model = |m: i32| if m == MODEL_8580 { "8580" } else { "6581" };
        let mut out = format!(
            "  socket 1      {}\n  output        {}\n",
            if self.socket1 { "ARMSID fitted" } else { "empty" },
            if self.listening { "a sink is listening, the engines run to the C64's clock" } else { "no sink" },
        );
        for (n, built) in self.built.iter().enumerate() {
            if let Some(m) = built {
                out.push_str(&format!("  engine {n}      {}\n", model(*m)));
            }
        }
        // The firmware programs one 32-byte window per mirror, so 48 of them cover $D400-$D7FF; runs that reach
        // the same chip are one line.
        let mut runs: Vec<(u16, u16, u8)> = Vec::new();
        for w in &self.map {
            match runs.last_mut() {
                Some(run) if run.2 == w.chip && run.1 + 1 == w.start => run.1 = w.end,
                _ => runs.push((w.start, w.end, w.chip)),
            }
        }
        match runs.is_empty() {
            true => out.push_str("  windows       none: nothing is routed to a SID\n"),
            false => {
                for (start, end, chip) in runs {
                    out.push_str(&format!("  window        ${start:04x}-${end:04x} -> chip {chip}\n"));
                }
            }
        }
        out
    }

    pub fn tap(&self) -> SidTap {
        SidTap
    }

    /// Apply the writes traced during the last run, then follow the C64 to cycle `clk` when a sink listens.
    ///
    /// Without a sink the writes only set registers: nothing hears their timing, and the next DMA access clocks the
    /// gap. A program that keeps writing the SID (the placeholder KERNAL's jingle, default_kernal.tas:98-135) then costs
    /// no reSID clocking.
    pub fn advance(&mut self, clk: u64) {
        self.drain(self.clocking());
        if self.listening {
            self.command(Cmd::ClockTo(clk));
        }
        self.finish();
    }

    /// Apply, at cycle `clk`, the writes a firmware DMA write just made through `Machine::write_full`: that path's trace
    /// stamps `Machine.clk`, which is stale between runs (855 D4). Clocks the gap, as any DMA access does.
    pub fn dma_written(&mut self, clk: u64) {
        self.drain(Clocking::At(clk));
        self.finish();
    }

    /// A C64 core config latch changed (0x10180000 + `off`). A decode change rebuilds the table and the engines whose
    /// model changed.
    pub fn core_config(&mut self, off: u8, val: u8) {
        let mut decode = self.decode;
        decode.write(off, val);
        if decode == self.decode {
            return;
        }
        // Writes traced so far went to the old groups.
        self.drain(self.clocking());
        self.decode = decode;
        self.route();
        self.remodel();
        self.finish();
    }

    /// U64_AUDIO_MIXER byte `off` (0x10100500 + `off`); the gains apply from the next samples on.
    pub fn mixer_write(&mut self, off: u8, val: u8) {
        if let Some(byte) = self.mixer.get_mut(usize::from(off)) {
            *byte = val;
        }
        self.gains();
        self.finish();
    }

    /// Whether `addr` is in a range a SID can be mapped to.
    pub fn window(addr: u16) -> bool {
        matches!(addr, 0xD400..=0xD7FF | 0xDE00..=0xDFFF)
    }

    /// A firmware DMA read at cycle `clk` (I/O mapped) through UE2's own decode: the ARMSID's answer, else the first
    /// receiver's reSID (S17 §5 Q1). None when no SID decodes `addr`.
    pub fn read(&mut self, addr: u16, clk: u64) -> Option<u8> {
        let (mask, reg) = (self.receivers(addr), (addr & 0x1F) as u8);
        if mask == 0 {
            return None;
        }
        if let Some(v) = Self::armsid_answer(mask, reg) {
            return Some(v);
        }
        Some(match &mut self.host {
            Host::Inline(engines) => {
                engines.clock_to(clk);
                let val = engines.read(mask, reg);
                self.refresh_readback();
                val
            }
            Host::External(external) => {
                external.command(Cmd::ClockTo(clk));
                let val = external.shadow.read(mask, reg);
                self.refresh_readback();
                val
            }
        })
    }

    /// [`Sid::read`] for a debugger, from the local digital state without clocking.
    pub fn peek(&self, addr: u16) -> Option<u8> {
        let (mask, reg) = (self.receivers(addr), (addr & 0x1F) as u8);
        if mask == 0 {
            return None;
        }
        if let Some(v) = Self::armsid_answer(mask, reg) {
            return Some(v);
        }
        Some(match &self.host {
            Host::Inline(engines) => engines.read(mask, reg),
            Host::External(external) => external.shadow.read(mask, reg),
        })
    }

    /// The C64 reset line was asserted: the chips clear their registers; the ARMSID leaves configuration mode but keeps
    /// its mode.
    pub fn reset(&mut self) {
        self.drain(self.clocking());
        self.regs = [[0; 0x19]; RECEIVERS];
        self.command(Cmd::Reset);
        DOOR.with(|door| door.borrow_mut().armsid.config = false);
        self.finish();
    }

    /// The C64 moved to a row with another clock at cycle `clk` (S25 §3): what was owed up to there is clocked at the
    /// old rate, the rest at `clock_hz`.
    pub fn set_clock(&mut self, clock_hz: u64, clk: u64) {
        self.drain(self.clocking());
        if self.listening {
            self.command(Cmd::ClockTo(clk));
        }
        self.command(Cmd::ClockFreq(clock_hz));
        self.finish();
    }

    /// TRX64 restarted its cycle counter at `clk` (warm reset, c64_6510core.rs:677).
    pub fn reanchor(&mut self, clk: u64) {
        self.command(Cmd::Reanchor(clk));
        self.finish();
    }

    fn clocking(&self) -> Clocking {
        if self.listening {
            Clocking::Traced
        } else {
            Clocking::Untimed
        }
    }

    fn receivers(&self, addr: u16) -> u16 {
        self.decode.receivers(addr, self.socket1)
    }

    /// Groups and table from the decode (S17 §2.1). Every $D400-$D7FF block is listed, a block nobody decodes on an
    /// empty group, so TRX64's fallback to chip 0 never fires; a $DE00-$DFFF block only when something receives it.
    /// Chip 0, the one TRX64 ticks, is the first group holding UltiSID 1-A, else the first block's.
    fn route(&mut self) {
        let blocks = BLOCKS.map(|start| (start, self.receivers(start)));
        let chip0 = blocks.iter().find(|(_, rx)| rx & bit(ULTISID1_A) != 0).unwrap_or(&blocks[0]).1;
        let mut groups = vec![chip0];
        let mut map = Vec::with_capacity(BLOCKS.len());
        for (start, rx) in blocks {
            if start >= 0xDE00 && rx == 0 {
                continue;
            }
            let chip = groups.iter().position(|&g| g == rx).unwrap_or_else(|| {
                groups.push(rx);
                groups.len() - 1
            });
            map.push(SidMapping::window(start, chip as u8, AHEAD_OF_EXPANSION));
        }
        let armsid_chips = (groups.iter().enumerate())
            .filter(|(_, rx)| **rx & bit(SOCKET1_RX) != 0)
            .fold(0, |chips, (chip, _)| chips | 1u64 << chip);
        DOOR.with(|door| door.borrow_mut().armsid_chips = armsid_chips);
        self.groups = groups;
        if map != self.map {
            self.map = map;
            self.map_changed = true;
        }
        self.refresh_readback();
    }

    /// The ARMSID's configuration answer when socket 1 is among `rx`.
    fn armsid_answer(rx: u16, reg: u8) -> Option<u8> {
        (rx & bit(SOCKET1_RX) != 0).then(|| DOOR.with(|door| door.borrow().armsid.read(reg))).flatten()
    }

    /// The model a receiver's settings ask for: the ARMSID's mode for socket 1, WAVES of its UltiSID otherwise.
    fn wanted_model(&self, rx: usize) -> i32 {
        if rx == SOCKET1_RX {
            DOOR.with(|door| door.borrow().armsid.model)
        } else if self.decode.waves[(rx - 1) / 4] != 0 {
            MODEL_8580
        } else {
            MODEL_6581
        }
    }

    /// Build receiver `rx`'s engine at its first write, or rebuild it when its model changed.
    fn ensure(&mut self, rx: usize) {
        let model = self.wanted_model(rx);
        if self.built[rx] == Some(model) {
            return;
        }
        self.built[rx] = Some(model);
        self.command(Cmd::Ensure { rx, model });
    }

    /// Rebuild every engine whose model no longer matches its settings.
    fn remodel(&mut self) {
        for rx in 0..RECEIVERS {
            if self.built[rx].is_some() {
                self.ensure(rx);
            }
        }
    }

    /// Left and right gain of receiver `rx` in 1/[`SIDE_UNITY`] steps: its mixer channel's two bytes.
    fn gain(&self, rx: usize) -> (i32, i32) {
        let ch = match rx {
            SOCKET1_RX => CH_SOCKET1,
            _ if rx < ultisid_rx(1, 0) => CH_ULTISID1,
            _ => CH_ULTISID2,
        };
        (i32::from(self.mixer[2 * ch]), i32::from(self.mixer[2 * ch + 1]))
    }

    /// Hand the gains of the current mixer bytes to the engines.
    fn gains(&mut self) {
        let gains = std::array::from_fn(|rx| self.gain(rx));
        self.command(Cmd::Gains(gains));
    }

    /// Apply the queued traced writes in order, each to every receiver of its chip's group.
    fn drain(&mut self, clocking: Clocking) {
        let mut writes = DOOR.with(|door| std::mem::take(&mut door.borrow_mut().writes));
        if !writes.is_empty() {
            if let Clocking::At(clk) = clocking {
                self.command(Cmd::ClockTo(clk));
            }
            for &(at, chip, reg, val) in &writes {
                let at = matches!(clocking, Clocking::Traced).then_some(at);
                let mask = self.groups.get(usize::from(chip)).copied().unwrap_or(0);
                for rx in (0..RECEIVERS).filter(|&rx| mask & bit(rx) != 0) {
                    self.ensure(rx);
                    if let Some(r) = self.regs[rx].get_mut(usize::from(reg)) {
                        *r = val;
                    }
                }
                self.command(Cmd::Write { at, mask, reg, val });
            }
            writes.clear();
        }
        // The queue keeps its allocation.
        DOOR.with(|door| {
            let mut door = door.borrow_mut();
            if door.writes.is_empty() {
                door.writes = writes;
            }
        });
    }

    /// Run one command: inline straight away, else into the worker's batch.
    fn command(&mut self, cmd: Cmd) {
        match &mut self.host {
            Host::Inline(engines) => engines.command(cmd),
            Host::External(external) => external.command(cmd),
        }
    }

    /// End of a CPU batch: refresh the door's synchronous digital readback.
    fn finish(&mut self) {
        self.refresh_readback();
    }

    /// The door answers $1B/$1C of chips 1 and up from the first engine of each
    /// group's digital state as of the last CPU slice.
    fn refresh_readback(&self) {
        let cached = |value: u32| (value & 1 << 16 != 0).then(|| [(value >> 8) as u8, value as u8]);
        let first = |mask: u16| {
            (0..RECEIVERS).filter(|&rx| mask & bit(rx) != 0).find_map(|rx| match &self.host {
                Host::Inline(engines) => cached(engines.cache[rx]),
                Host::External(external) => cached(external.shadow.cache[rx]),
            })
        };
        DOOR.with(|door| {
            let mut door = door.borrow_mut();
            door.readback.clear();
            door.readback
                .extend(self.groups.iter().enumerate().map(|(chip, &mask)| first(mask).filter(|_| chip > 0)));
        });
    }

    /// The local engines or digital shadow, for timing tests.
    #[cfg(test)]
    fn inline(&self) -> &Engines {
        match &self.host {
            Host::Inline(engines) => engines,
            Host::External(external) => &external.shadow,
        }
    }

    #[cfg(test)]
    pub(crate) fn ultisid_regs(&self, n: usize, instance: usize) -> Option<[u8; 0x19]> {
        let rx = ultisid_rx(n, instance);
        self.built[rx].is_some().then(|| self.regs[rx])
    }
}


impl Default for Sid {
    fn default() -> Self {
        Self::new()
    }
}

/// Add mono `pcm` at `(left, right)` gain into the stereo frames `acc`, padded with its last sample or trimmed to
/// `acc`'s length: an engine built after the first can be a sample apart per chunk (855 §2).
fn mix_into(acc: &mut [i32], pcm: &[i16], (left, right): (i32, i32)) {
    let last = pcm.last().copied().unwrap_or(0);
    for (i, frame) in acc.chunks_exact_mut(2).enumerate() {
        let s = i32::from(pcm.get(i).copied().unwrap_or(last));
        frame[0] += s * left;
        frame[1] += s * right;
    }
}

#[cfg(test)]
fn mixed_down(acc: &[i32]) -> Vec<i16> {
    acc.iter().map(|&s| (s / SIDE_UNITY).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16).collect()
}

/// TRX64 [`Observer`] for a CPU run. Empty: TRX64's write trace carries the chip and cycle now (855 D4).
pub struct SidTap;

impl Observer for SidTap {
    const RECORDS_CPU: bool = false;
    const RECORDS_BUS: bool = false;
    #[inline(always)]
    fn on_instruction(&mut self, _: u16, _: u8, _: u8, _: u8, _: u8, _: u8, _: u8, _: u8, _: u8, _: u64) {}

    #[inline(always)]
    fn on_bus(&mut self, _: BusKind, _: u16, _: u8, _: u16, _: u64, _: u8) {}

    #[inline(always)]
    fn on_interrupt(&mut self, _: u16, _: u64) {}
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    use trx64_core::sid::resolve_sid;
    use ue2_core::c64host::C64Backend;

    use super::*;

    const U1A: u16 = bit(ULTISID1_A);
    const U1B: u16 = bit(ultisid_rx(0, 1));
    const U1C: u16 = bit(ultisid_rx(0, 2));
    const U1D: u16 = bit(ultisid_rx(0, 3));
    const U2A: u16 = bit(ultisid_rx(1, 0));
    const U2B: u16 = bit(ultisid_rx(1, 1));
    const S1: u16 = bit(SOCKET1_RX);

    /// A write as TRX64's trace reports it, routed by the table `sid` handed out.
    fn poke(sid: &Sid, addr: u16, val: u8, clk: u64) {
        let (chip, reg) = resolve_sid(&sid.map, addr).expect("every $D400-$D7FF block is listed");
        traced(chip, reg as u8, val, clk);
    }

    /// The SID player's map: unmapAllSids, then SetSidAddress for UltiSID 1 and 2 (u64_config.cc:1886-1920, 2031).
    fn player_map(sid: &mut Sid, ultisids: [(u8, u8); 2]) {
        for (dev, (base, mask)) in [(0x01, 0xFE), (0x01, 0xFE)].into_iter().chain(ultisids).enumerate() {
            sid.core_config(SID1_BASE + dev as u8, base);
            sid.core_config(SID1_MASK + dev as u8, mask);
        }
    }

    const UNMAPPED: (u8, u8) = (0x01, 0xFE);

    /// The firmware's post-detection map with socket 1 enabled (S_SetupDetectionAddresses, u64_config.cc:2303-2319).
    fn detection_map(sid: &mut Sid) {
        for (off, val) in [(0x08, 0x40), (0x09, 0x50), (0x0A, 0x60), (0x0B, 0x60), (0x0C, 0xF0), (0x0D, 0xF0)] {
            sid.core_config(off, val);
        }
        for (off, val) in [(0x0E, 0xFE), (0x0F, 0xFE), (0x11, 1), (0x12, 1), (0x14, 0)] {
            sid.core_config(off, val);
        }
    }

    /// Chip of each block as TRX64 would route its first address.
    fn chip_at(sid: &Sid, addr: u16) -> Option<u8> {
        resolve_sid(&sid.map, addr).map(|(chip, _)| chip)
    }

    fn group_at(sid: &Sid, addr: u16) -> Option<u16> {
        chip_at(sid, addr).map(|chip| sid.groups[usize::from(chip)])
    }

    #[test]
    fn boot_map_gives_d400_d7ff_to_both_ultisids() {
        let mut sid = Sid::new();
        let map = sid.take_map().expect("a new Sid has a table to hand over");
        assert_eq!(sid.take_map(), None, "handed over once");
        assert_eq!(map.len(), 32, "the $D400-$D7FF blocks, nothing at $DE00-$DFFF");
        for (i, w) in map.iter().enumerate() {
            assert_eq!((w.start, w.end, w.chip), (0xD400 + 32 * i as u16, 0xD41F + 32 * i as u16, 0));
        }
        assert_eq!(sid.groups, [U1A | U2A]);
        assert_eq!(chip_at(&sid, 0xDE00), None, "the expansion port keeps $DE00");
    }

    #[test]
    fn sid_player_maps_one_and_two_sids() {
        let mut sid = Sid::new();
        sid.take_map();
        player_map(&mut sid, [(0x40, 0xC0), UNMAPPED]);
        assert_eq!(sid.groups, [U1A], "one SID: UltiSID 1 mirrored through $D7FF (u64_config.cc:1893-1896)");
        assert!(sid.map.iter().all(|w| w.chip == 0) && sid.map.len() == 32);
        assert_eq!(sid.take_map(), None, "every block is still chip 0: only its group changed");

        player_map(&mut sid, [(0x40, 0xFE), (0x42, 0xFE)]);
        assert_eq!((group_at(&sid, 0xD400), chip_at(&sid, 0xD400)), (Some(U1A), Some(0)));
        assert_eq!((group_at(&sid, 0xD43F), chip_at(&sid, 0xD420)), (Some(U2A), Some(1)));
        assert_eq!((group_at(&sid, 0xD440), chip_at(&sid, 0xD7E0)), (Some(0), Some(2)), "nobody decodes: an empty group");
        assert_eq!(sid.groups, [U1A, U2A, 0]);
        assert_eq!(sid.map.len(), 32);
        assert!(sid.take_map().is_some());
    }

    #[test]
    fn splits_pick_separate_instances() {
        let mut sid = Sid::new();
        sid.core_config(EMUSID_SPLIT, 1);
        let alternating = [U1A | U2A, U1B | U2B, U1A | U2A, U1B | U2B];
        assert_eq!([0xD400, 0xD420, 0xD440, 0xD460].map(|a| group_at(&sid, a).unwrap()), alternating, "1/2 (A5)");
        assert_eq!(sid.groups, [U1A | U2A, U1B | U2B]);

        let fours: [(u8, [u16; 4]); 3] =
            [(5, [0x40, 0x42, 0x44, 0x46]), (6, [0x40, 0x42, 0x50, 0x52]), (7, [0x40, 0x48, 0x50, 0x58])];
        for (split, a) in fours {
            sid.core_config(EMUSID_SPLIT, split);
            let groups = a.map(|a| group_at(&sid, 0xD000 | a << 4).unwrap() & (U1A | U1B | U1C | U1D));
            assert_eq!(groups, [U1A, U1B, U1C, U1D], "1/4, split {split}");
        }
        assert_eq!(sid.groups.len(), 4, "A-D of both UltiSIDs, mirrored");
        assert_eq!(group_at(&sid, 0xD600), Some(U1A | U2A), "split 7: A7 and A8 clear again");
    }

    #[test]
    fn a_socket_needs_its_enable_and_a_fitted_chip() {
        let mut sid = Sid::new();
        detection_map(&mut sid);
        assert_eq!(group_at(&sid, 0xD400), Some(0), "socket 1 is empty");
        assert_eq!(group_at(&sid, 0xD600), Some(U1A | U2A), "UltiSIDs parked at $D600");
        assert_eq!(chip_at(&sid, 0xD600), Some(0));
        sid.set_socket1(true);
        assert_eq!(group_at(&sid, 0xD400), Some(S1));
        assert_eq!(group_at(&sid, 0xD4FF), Some(S1), "MASK 0xF0: $D400-$D4FF");
        assert_eq!(group_at(&sid, 0xD500), Some(0), "socket 2 is enabled at $D500, but never fitted");
        assert_eq!(group_at(&sid, 0xD620), Some(0), "MASK 0xFE: 32 bytes");
        sid.core_config(SID1_EN, 0);
        assert_eq!(group_at(&sid, 0xD400), Some(0), "disabled");
    }

    #[test]
    fn groups_are_numbered_from_ultisid_1a() {
        let mut sid = Sid::new();
        player_map(&mut sid, [(0x42, 0xFE), (0x40, 0xFE)]);
        assert_eq!(sid.groups, [U1A, U2A, 0], "chip 0 holds UltiSID 1-A although $D400 is UltiSID 2");
        assert_eq!([0xD400, 0xD420, 0xD440].map(|a| chip_at(&sid, a)), [Some(1), Some(0), Some(2)]);

        player_map(&mut sid, [UNMAPPED, UNMAPPED]);
        assert_eq!(sid.groups, [0], "nothing mapped: the first group, empty, is chip 0");
        assert!(sid.map.iter().all(|w| w.chip == 0) && sid.map.len() == 32);

        player_map(&mut sid, [(0x40, 0xFE), (0xE0, 0xFE)]);
        let de00 = *sid.map.last().unwrap();
        assert_eq!(sid.map.len(), 33, "one $DE00-$DFFF block, because UltiSID 2 receives it");
        assert_eq!((de00.start, de00.end, de00.ahead_of_expansion), (0xDE00, 0xDE1F, true));
        assert_eq!(sid.groups[usize::from(de00.chip)], U2A);
        assert_eq!(chip_at(&sid, 0xDE20), None);
    }

    #[test]
    fn a_write_fans_out_to_its_group() {
        let mut sid = Sid::new();
        sid.set_socket1(true);
        detection_map(&mut sid);
        for (off, val) in [(0x0A, 0x40), (0x0B, 0x40), (0x0E, 0xC0), (0x0F, 0xC0)] {
            sid.core_config(off, val);
        }
        assert_eq!(group_at(&sid, 0xD400), Some(S1 | U1A | U2A), "socket 1 and both UltiSIDs at $D400");
        poke(&sid, 0xD418, 0x0F, 0);
        poke(&sid, 0xD501, 66, 0);
        sid.advance(1000);
        for rx in [SOCKET1_RX, ULTISID1_A, ultisid_rx(1, 0)] {
            assert_eq!(sid.inline().engines[rx].as_ref().map(|e| e.regs[0x18]), Some(0x0F), "an engine per receiver: {rx}");
        }
        assert_eq!(sid.inline().engines[SOCKET1_RX].as_ref().map(|e| e.regs[0x01]), Some(0), "$D501 is not socket 1's");
        assert_eq!(sid.ultisid_regs(1, 0).map(|r| r[0x01]), Some(66), "the UltiSIDs mirror through $D7FF");
        assert!(sid.ultisid_regs(0, 1).is_none(), "no write, no engine");

        player_map(&mut sid, [(0x40, 0xFE), (0x42, 0xFE)]);
        poke(&sid, 0xD440, 0x55, 0);
        sid.advance(2000);
        assert_eq!(sid.inline().order.len(), 3, "a write nobody decodes builds nothing");
    }

    #[test]
    fn each_ultisid_takes_its_own_model() {
        let mut sid = Sid::new();
        poke(&sid, 0xD414, 0xF0, 0);
        sid.advance(0);
        let model = |sid: &Sid, rx: usize| sid.inline().engines[rx].as_ref().map(|e| (e.model, e.regs[0x14]));
        sid.core_config(EMUSID2_WAVES, 1);
        assert_eq!(model(&sid, ULTISID1_A), Some((MODEL_6581, 0xF0)));
        assert_eq!(model(&sid, ultisid_rx(1, 0)), Some((MODEL_8580, 0xF0)), "rebuilt with its registers");
        sid.core_config(EMUSID1_WAVES, 1);
        sid.core_config(EMUSID2_WAVES, 0);
        assert_eq!(model(&sid, ULTISID1_A), Some((MODEL_8580, 0xF0)));
        assert_eq!(model(&sid, ultisid_rx(1, 0)), Some((MODEL_6581, 0xF0)));
    }

    #[test]
    fn armsid_answers_the_firmware_probe_and_configuration() {
        let mut a = ArmSid::default();
        // detectRemakes (u64_config.cc:599-637).
        for (reg, val) in [(0x1D, b'S'), (0x1E, b'I'), (0x1F, b'D')] {
            a.write(reg, val);
        }
        assert_eq!((a.read(0x1B), a.read(0x1C)), (Some(b'N'), Some(b'O')));
        a.write(0x1F, b'I');
        a.write(0x1E, b'I');
        assert!(!matches!(a.read(0x1B), Some(b'L' | b'R')), "ARMSID, not ARM2SID");
        a.write(0x1D, 0);
        assert_eq!(a.read(0x1B), None, "configuration mode left");
        // readParams (sid_device_armsid.cc:108-179).
        let info = |a: &mut ArmSid, cmd: u8| {
            for (reg, val) in [(0x1D, b'S'), (0x1E, b'I'), (0x1F, b'D'), (0x1F, cmd), (0x1E, b'I')] {
                a.write(reg, val);
            }
            [a.read(0x1B).unwrap(), a.read(0x1C).unwrap()]
        };
        assert_eq!(info(&mut a, b'V'), ARMSID_VERSION);
        assert_eq!(info(&mut a, b'F')[0], b'6');
        assert_eq!(info(&mut a, b'U'), [0x23, 0x28]);
        // S_set_mode then S_set_filt (sid_device_armsid.cc:238-275).
        assert!([(0x1D, b'S'), (0x1E, b'E'), (0x1F, b'8')].iter().any(|&(r, v)| a.write(r, v)), "mode change");
        assert_eq!(a.model, MODEL_8580);
        for cmd in [0x8E, 0x91, 0xA2, 0xB3] {
            a.write(0x1F, cmd);
            a.write(0x1E, b'E');
        }
        assert_eq!(info(&mut a, b'H'), [0xE1, 0x23]);
        assert_eq!(info(&mut a, b'F')[0], b'8');
        a.write(0x1D, 0);
        assert!(!a.write(0x1F, b'6'), "no commands outside configuration mode");
        assert_eq!(a.model, MODEL_8580);
    }

    /// Firmware DMA reads through UE2's decode: the ARMSID, then socket 1's reSID; the ARMSID's mode picks its model.
    #[test]
    fn dma_reads_socket1_armsid_then_resid() {
        let mut sid = Sid::new();
        detection_map(&mut sid);
        let mut clk = 1000;
        let dma = |sid: &mut Sid, reg: u16, val: u8, clk: u64| {
            poke(sid, 0xD400 + reg, val, clk);
            sid.dma_written(clk);
        };
        for (reg, val) in [(0x1D, b'S'), (0x1E, b'I'), (0x1F, b'D')] {
            dma(&mut sid, reg, val, clk);
        }
        assert_eq!(sid.read(0xD41B, clk), None, "empty socket 1 does not answer");
        sid.set_socket1(true);
        for (reg, val) in [(0x1D, b'S'), (0x1E, b'I'), (0x1F, b'D')] {
            dma(&mut sid, reg, val, clk);
        }
        assert_eq!((sid.read(0xD41B, clk), sid.read(0xD41C, clk)), (Some(b'N'), Some(b'O')));
        assert_eq!(sid.peek(0xD41C), Some(b'O'));
        assert_eq!(sid.read(0xD51B, clk), None, "socket 2 is empty");
        assert_eq!(sid.read(0xD61B, clk), Some(0), "UltiSID 1 parked at $D600, never written");
        assert_eq!(sid.read(0xD71B, clk), None, "nothing at $D700");
        dma(&mut sid, 0x1D, 0, clk);
        // Voice 3 envelope through reSID: attack 0, gate on.
        dma(&mut sid, 0x13, 0x00, clk);
        dma(&mut sid, 0x14, 0xF0, clk);
        dma(&mut sid, 0x12, 0x21, clk);
        clk += 20_000;
        assert_eq!(sid.read(0xD41C, clk), Some(0xFF), "ENV3 from reSID after 20 ms of attack 0");
        assert_eq!(sid.inline().engines[SOCKET1_RX].as_ref().map(|e| e.model), Some(MODEL_6581));
        for (reg, val) in [(0x1D, b'S'), (0x1E, b'I'), (0x1F, b'D'), (0x1E, b'E'), (0x1F, b'8')] {
            dma(&mut sid, reg, val, clk);
        }
        let socket1 = sid.inline().engines[SOCKET1_RX].as_ref().map(|e| (e.model, e.regs[0x14]));
        assert_eq!(socket1, Some((MODEL_8580, 0xF0)), "the ARMSID's mode, registers kept");
        sid.set_socket1(false);
        assert!(sid.inline().engines[SOCKET1_RX].is_none() && !sid.inline().order.contains(&SOCKET1_RX), "the chip left the socket");
    }

    #[test]
    fn cpu_writes_clock_resid_only_for_a_sink() {
        let mut sid = Sid::new();
        for (clk, reg, val) in [(500, 0x18, 0x0F), (800, 0x14, 0xF0), (900, 0x12, 0x21)] {
            poke(&sid, 0xD400 + reg, val, clk);
        }
        sid.advance(1000);
        let regs = sid.ultisid_regs(0, 0).unwrap();
        assert_eq!((sid.inline().clk, regs[0x18], regs[0x12]), (0, 0x0F, 0x21), "registers set, reSID not clocked");
        assert_eq!(sid.read(0xD41C, 50_000), Some(0xFF), "a DMA read clocks the gap: ENV3 after attack 0, sustain 15");
        assert_eq!(sid.inline().clk, 50_000);
        sid.set_audio(48_000, Box::new(Collect::default()), 50_000);
        poke(&sid, 0xD418, 0x00, 60_000);
        sid.advance(70_000);
        let regs = sid.ultisid_regs(0, 0).unwrap();
        assert_eq!((sid.inline().clk, regs[0x18]), (70_000, 0x00), "with a sink, writes and advances are clocked");
    }

    #[derive(Default)]
    struct Collect(Rc<RefCell<Vec<i16>>>);

    /// The mean of each stereo frame: the tests below measure tone and level, which pan does not change.
    fn mono(pcm: &[i16]) -> impl Iterator<Item = i16> + '_ {
        pcm.chunks_exact(2).map(|f| ((i32::from(f[0]) + i32::from(f[1])) / 2) as i16)
    }

    impl AudioSink for Collect {
        fn samples(&mut self, pcm: &[i16]) {
            self.0.borrow_mut().extend(mono(pcm));
        }
    }

    /// 1000 Hz sawtooth (F = 17029, PAL), volume 15, sustain 15, on the SID at `base`.
    fn tone(sid: &Sid, base: u16, clk: u64) {
        for (reg, val) in [(0x18, 15), (0x05, 0), (0x06, 0xF0), (0x01, 66), (0x00, 133), (0x04, 0x21)] {
            poke(sid, base + reg, val, clk);
        }
    }

    /// Rising crossings of the mean per second.
    fn hz(pcm: &[i16], rate: f64) -> f64 {
        let mean = pcm.iter().map(|&s| f64::from(s)).sum::<f64>() / pcm.len() as f64;
        let crossings = pcm.windows(2).filter(|w| f64::from(w[0]) < mean && f64::from(w[1]) >= mean).count();
        crossings as f64 * rate / pcm.len() as f64 * (985_248.0 / 985_000.0)
    }

    /// A stereo stream collector used for inline/portable parity checks.
    #[derive(Clone, Default)]
    struct SendCollect(std::sync::Arc<std::sync::Mutex<Vec<i16>>>);

    impl AudioSink for SendCollect {
        fn samples(&mut self, pcm: &[i16]) {
            self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(mono(pcm));
        }
    }

    #[test]
    fn portable_events_preserve_wire_layout_and_large_cycle_values() {
        assert_eq!(std::mem::size_of::<SidEvent>(), 16);
        let event = SidEvent::new(SidEvent::WRITE, 0x18, U1A | U2A, 0x0f, 0x1234_5678_8765_4321);
        assert_eq!(SidEvent::from_le_bytes(event.to_le_bytes()), event);
        assert_eq!(SidEvent::from_words(event.words()), event);
        assert_eq!(event.cycle(), 0x1234_5678_8765_4321);
    }

    #[test]
    fn portable_worker_pcm_matches_inline_with_timed_writes_and_mixer_changes() {
        let play = |external: bool| {
            let mut sid = Sid::new();
            player_map(&mut sid, [(0x40, 0xFE), (0x42, 0xFE)]);
            let sink = SendCollect::default();
            let sink_pcm = sink.0.clone();
            let mut rendered = Vec::new();
            let mut renderer = SidRenderer::new(48_000);
            if external { sid.set_audio_external(48_000, Box::new(sink), 0); }
            else { sid.set_audio(48_000, Box::new(sink), 0); }
            tone(&sid, 0xD400, 0);
            for ms in 1..=60u64 {
                let clk = ms * 985;
                if ms == 20 { tone(&sid, 0xD420, clk); }
                if ms == 40 { sid.mixer_write(2, 0); sid.mixer_write(3, 0); }
                sid.advance(clk);
                if external { rendered.extend(renderer.render(&sid.take_events()).unwrap()); }
            }
            let collected = sink_pcm.lock().unwrap().clone();
            if external {
                assert!(collected.iter().all(|&x| x == 0), "CPU shadow outputs no SID PCM");
                assert_eq!(collected.len(), rendered.len() / 2, "sampler clock exactly matches SID output cadence");
                mono(&rendered).collect::<Vec<_>>()
            } else { collected }
        };
        assert_eq!(play(true), play(false), "portable worker is bit-identical to inline synthesis");
    }

    #[test]
    fn portable_readback_is_synchronous_before_worker_consumes_events() {
        let mut sid = Sid::new();
        player_map(&mut sid, [(0x40, 0xFE), (0x42, 0xFE)]);
        sid.set_audio_external(48_000, Box::new(Collect::default()), 0);
        for (reg, val) in [(0x0e, 0x80), (0x0f, 0x20), (0x13, 0x00), (0x14, 0xf0), (0x12, 0x21)] {
            poke(&sid, 0xD420 + reg, val, 0);
        }
        sid.advance(2000);
        let env = sid.read(0xD43C, 2000).unwrap();
        assert!(env > 0, "shadow sees voice3 attack while worker is still idle");
        let events = sid.take_events();
        let mut renderer = SidRenderer::new(48_000);
        renderer.render(&events).unwrap();
        assert_eq!(renderer.engines.read(U2A, 0x1c), env);
        assert_eq!(renderer.engines.read(U2A, 0x1b), sid.read(0xD43B, 2000).unwrap());
    }

    #[test]
    fn reusable_portable_fifo_preserves_order_and_both_buffer_allocations() {
        let mut sid = Sid::new();
        sid.set_audio_external(48_000, Box::new(Collect::default()), 0);
        let mut out = Vec::with_capacity(1024);
        sid.drain_events_into(&mut out);
        // Warm both sides, then alternate the same two allocations indefinitely.
        sid.drain_events_into(&mut out);
        let Host::External(external) = &sid.host else { panic!("external host required") };
        let buffers = [external.events.as_ptr(), out.as_ptr()];
        let capacities = [external.events.capacity(), out.capacity()];
        for batch in 0..32 {
            let expected: Vec<_> = (0..128).map(|i|
                SidEvent::new(SidEvent::WRITE, (i % 25) as u8, U1A, (batch + i) as u32, (batch * 128 + i) as u64)).collect();
            let Host::External(external) = &mut sid.host else { unreachable!() };
            external.events.extend_from_slice(&expected);
            sid.drain_events_into(&mut out);
            assert_eq!(out, expected, "FIFO order in batch {batch}");
            let Host::External(external) = &sid.host else { unreachable!() };
            assert!(external.events.is_empty());
            assert!(buffers.contains(&external.events.as_ptr()) && buffers.contains(&out.as_ptr()));
            assert!(capacities.contains(&external.events.capacity()) && capacities.contains(&out.capacity()));
        }
        sid.drain_events_into(&mut out);
        assert!(out.is_empty(), "empty drain cannot replay the preceding batch");
    }

    #[test]
    fn portable_sampler_and_sid_cadence_match_through_reset_model_and_clock_changes() {
        for fast in [false, true] {
            for rate in [44_100, 48_000, 96_000] {
                let mut sid = Sid::new();
                let output = SendCollect::default();
                let collected = output.0.clone();
                sid.set_audio_external(rate, Box::new(output), 0);
                let mut renderer = if fast { SidRenderer::new_fast(rate) } else { SidRenderer::new(rate) };
                let mut frames = 0;
                let mut clk = 0;
                for step in 0..50 {
                    if step == 1 { tone(&sid, 0xd400, clk); }
                    if step == 10 { sid.core_config(EMUSID1_WAVES, 1); }
                    if step == 20 { sid.set_clock(1_022_727, clk); }
                    if step == 30 { sid.reset(); sid.reanchor(0); clk = 0; tone(&sid, 0xd400, 0); }
                    clk += 997;
                    sid.advance(clk);
                    frames += renderer.render(&sid.take_events()).unwrap().len() / 2;
                    assert_eq!(collected.lock().unwrap().len(), frames, "rate {rate}, step {step}, fast {fast}");
                }
            }
        }
    }

    #[test]
    fn portable_fast_preserves_three_sid_readback_sample_counts_and_stereo_mixing() {
        for rate in [44_100, 48_000, 96_000] {
            let mut reference = SidRenderer::new(rate);
            let mut fast = SidRenderer::new_fast(rate);
            let mut setup = Vec::new();
            for (rx, model, control, gains) in [
                (ULTISID1_A, MODEL_6581, 0x21, (90, 0)),
                (ultisid_rx(0, 1), MODEL_8580, 0x81, (0, 90)),
                (ultisid_rx(1, 0), MODEL_6581, 0x41, (45, 45)),
            ] {
                let mask = bit(rx);
                setup.push(SidEvent::new(SidEvent::ENSURE, 0, mask, model as u32, 0));
                setup.push(SidEvent::new(SidEvent::GAINS, 0, mask, gains.0 | gains.1 << 16, 0));
                for (reg, value) in [(0x18, 0x1f), (0x17, 0x44), (0x16, 100),
                    (0x0e, 0xb1), (0x0f, 0x19), (0x10, 0), (0x11, 8),
                    (0x13, 0x04), (0x14, 0xf7), (0x12, control)] {
                    setup.push(SidEvent::new(SidEvent::WRITE, reg, mask, value, 0));
                }
            }
            reference.render(&setup).unwrap();
            fast.render(&setup).unwrap();
            let mut samples = Vec::new();
            let mut cycle = 0;
            for step in 0..50 {
                cycle += [997, 1, 19_656, 211][step % 4];
                let mut events = vec![SidEvent::new(SidEvent::CLOCK, 0, 0, 0, cycle)];
                if step == 10 { events.push(SidEvent::new(SidEvent::ENSURE, 0, U1A, 1, cycle)); }
                if step == 20 { events.push(SidEvent::new(SidEvent::CLOCK_HZ, 0, 0, 1_022_727, cycle)); }
                if step == 30 { events.push(SidEvent::new(SidEvent::WRITE, 0x12, U2A, 0x43, cycle)); }
                if step == 40 { events.push(SidEvent::new(SidEvent::RESET, 0, 0, 0, cycle)); }
                let exact = reference.render(&events).unwrap();
                let light = fast.render(&events).unwrap();
                assert_eq!(light.len(), exact.len(), "rate {rate}, step {step}");
                assert_eq!(light.len() % 2, 0, "whole stereo frames");
                for rx in [ULTISID1_A, ultisid_rx(0, 1), ultisid_rx(1, 0)] {
                    assert_eq!(fast.engines.cache[rx], reference.engines.cache[rx], "digital SID readback");
                    assert_eq!(fast.engines.engines[rx].as_ref().unwrap().resid.config().sampling_method,
                        SAMPLE_INTERPOLATE, "model/clock changes retain the lightweight sampler");
                }
                samples.extend(light);
            }
            assert!(samples.iter().any(|&sample| sample != 0), "audible output at {rate} Hz");
            assert!(samples.chunks_exact(2).any(|frame| frame[0] != frame[1]), "independent stereo routing");
            fast.engines.set_rate(48_000);
            for engine in fast.engines.engines.iter().flatten() {
                assert_eq!(engine.resid.config().sampling_method, SAMPLE_INTERPOLATE, "rate changes retain fast mode");
            }
        }
    }

    #[test]
    fn portable_fast_checkpoint_continues_pcm_and_sample_cadence() {
        let mut renderer = SidRenderer::new_fast(48_000);
        let setup = [SidEvent::new(SidEvent::ENSURE, 0, U1A, 0, 0),
            SidEvent::new(SidEvent::WRITE, 0x18, U1A, 15, 0),
            SidEvent::new(SidEvent::WRITE, 0x05, U1A, 0, 0),
            SidEvent::new(SidEvent::WRITE, 0x06, U1A, 0xf0, 0),
            SidEvent::new(SidEvent::WRITE, 0x01, U1A, 66, 0),
            SidEvent::new(SidEvent::WRITE, 0x00, U1A, 133, 0),
            SidEvent::new(SidEvent::WRITE, 0x04, U1A, 0x21, 0),
            SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 19_656)];
        renderer.render(&setup).unwrap();
        let snapshot = renderer.engines.engines[ULTISID1_A].as_ref().unwrap().resid.capture_state();
        let next = [SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 39_313)];
        let expected = renderer.render(&next).unwrap();
        renderer.engines.engines[ULTISID1_A].as_mut().unwrap().resid.restore_state(&snapshot);
        renderer.engines.clk = 19_656;
        assert_eq!(renderer.render(&next).unwrap(), expected);
    }

    #[test]
    fn portable_renderer_rejects_bad_batches_before_applying_any_event() {
        let mut renderer = SidRenderer::new(48_000);
        let mut out = vec![123];
        let events = [SidEvent::new(SidEvent::ENSURE, 0, U1A, 0, 0),
            SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 2000),
            SidEvent::new(SidEvent::WRITE, 0, U1A, 1, 1000)];
        assert!(renderer.render_into(&events, &mut out).is_err());
        assert!(renderer.engines.order.is_empty());
        assert_eq!(renderer.engines.clk, 0);
        assert_eq!(out, [123]);
        assert!(renderer.render(&[SidEvent::new(SidEvent::ENSURE, 0, 1 << 12, 0, 0)]).is_err());
    }

    fn partition_tone(rx: usize, model: i32, cycle: u64) -> Vec<SidEvent> {
        let mask = bit(rx);
        let mut events = vec![SidEvent::new(SidEvent::ENSURE, 0, mask, model as u32, cycle),
            SidEvent::new(SidEvent::GAINS, 0, mask, (255 - rx as u32 * 13) | ((60 + rx as u32 * 17) << 16), cycle)];
        for (reg, value) in [(0x18, 0x1f), (0x17, 0x44), (0x16, 100),
            (0x0e, 0xb1), (0x0f, 0x19 + rx as u32), (0x10, 0), (0x11, 8),
            (0x13, 0x04), (0x14, 0xf7), (0x12, 0x21)] {
            events.push(SidEvent::new(SidEvent::WRITE, reg, mask, value, cycle));
        }
        events
    }

    fn assert_partition_batch(reference: &mut SidRenderer, partitions: &mut [SidPartitionRenderer], events: &[SidEvent]) {
        let expected = reference.render(events).unwrap();
        let mut sum = vec![0i32; expected.len()];
        for partition in partitions {
            let raw = partition.render(events).unwrap();
            assert_eq!(raw.len(), expected.len(), "all partitions follow global first receiver per chunk");
            for (sum, raw) in sum.iter_mut().zip(raw) { *sum += raw; }
        }
        assert_eq!(mixed_down(&sum), expected, "normalization/clipping only after complete partition sum");
    }

    #[test]
    fn partition_renderer_matches_combined_one_two_three_receivers_at_all_rates() {
        for fast in [false, true] {
            for rate in [44_100, 48_000, 96_000] {
                for count in 1..=3 {
                    let receivers = &[5, 1, 2][..count]; // Global leader is not the smallest receiver index.
                    let mut reference = if fast { SidRenderer::new_fast(rate) } else { SidRenderer::new(rate) };
                    let mut partitions: Vec<_> = receivers.iter().map(|&rx| if fast {
                        SidPartitionRenderer::new_fast(rate, bit(rx)).unwrap()
                    } else { SidPartitionRenderer::new(rate, bit(rx)).unwrap() }).collect();
                    let events: Vec<_> = receivers.iter().enumerate().flat_map(|(i, &rx)|
                        partition_tone(rx, (i % 2) as i32, 0)).collect();
                    assert_partition_batch(&mut reference, &mut partitions, &events);
                    let mut cycle = 0;
                    for step in 0..40 {
                        cycle += [997, 1, 211, 123][step % 4];
                        let rx = receivers[step % count];
                        let events = [SidEvent::new(SidEvent::WRITE, 0x0e, bit(rx), (step * 17 % 256) as u32, cycle),
                            SidEvent::new(SidEvent::CLOCK, 0, 0, 0, cycle + 501)];
                        cycle += 501;
                        assert_partition_batch(&mut reference, &mut partitions, &events);
                    }
                }
            }
        }
    }

    #[test]
    fn partition_renderer_preserves_late_receiver_clock_resets_and_insertion_order() {
        for rate in [44_100, 48_000, 96_000] {
            let mut reference = SidRenderer::new_fast(rate);
            let mut partitions: Vec<_> = [5, 1, 2].iter().map(|&rx| SidPartitionRenderer::new_fast(rate, bit(rx)).unwrap()).collect();
            let mut cycle = 0;
            assert_partition_batch(&mut reference, &mut partitions, &[SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 97)]);
            cycle += 97;
            for step in 0..45 {
                let mut events = Vec::new();
                match step {
                    0 => events.extend(partition_tone(5, MODEL_6581, cycle)),
                    2 => events.extend(partition_tone(1, MODEL_8580, cycle)),
                    4 => events.extend(partition_tone(2, MODEL_6581, cycle)),
                    6 => events.push(SidEvent::new(SidEvent::ENSURE, 0, bit(5), MODEL_8580 as u32, cycle)),
                    8 => events.push(SidEvent::new(SidEvent::CLOCK_HZ, 0, 0, 1_022_727, cycle)),
                    10 => events.push(SidEvent::new(SidEvent::DROP, 0, bit(5), 0, cycle)),
                    12 => events.extend(partition_tone(5, MODEL_6581, cycle)),
                    15 => events.push(SidEvent::new(SidEvent::RESET, 0, 0, 0, cycle)),
                    17 => { events.push(SidEvent::new(SidEvent::REANCHOR, 0, 0, 0, 0)); cycle = 0; },
                    22 => {
                        reference.engines.set_rate(48_000);
                        for partition in &mut partitions { partition.set_rate(48_000).unwrap(); }
                    },
                    24 => for rx in [1, 2, 5] { events.push(SidEvent::new(SidEvent::DROP, 0, bit(rx), 0, cycle)); },
                    26 => events.extend(partition_tone(2, MODEL_8580, cycle)),
                    _ => {},
                }
                cycle += [997, 19_656, 1, 211][step % 4];
                events.push(SidEvent::new(SidEvent::WRITE, 0x12, bit(1), 0x21 + (step % 3) as u32, cycle));
                cycle += 1;
                events.push(SidEvent::new(SidEvent::CLOCK, 0, 0, 0, cycle));
                assert_partition_batch(&mut reference, &mut partitions, &events);
            }
        }
    }

    #[test]
    fn partition_renderer_aligns_late_receiver_at_every_large_clock_chunk() {
        let mut reference = SidRenderer::new_fast(48_000);
        let mut partitions = [SidPartitionRenderer::new_fast(48_000, bit(5)).unwrap(),
            SidPartitionRenderer::new_fast(48_000, bit(1)).unwrap()];
        assert_partition_batch(&mut reference, &mut partitions, &partition_tone(5, MODEL_6581, 0));
        assert_partition_batch(&mut reference, &mut partitions, &[SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 11)]);
        assert_partition_batch(&mut reference, &mut partitions, &partition_tone(1, MODEL_8580, 11));
        assert_partition_batch(&mut reference, &mut partitions, &[SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 11 + CHUNK + 1_011)]);
        assert_partition_batch(&mut reference, &mut partitions, &[SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 17 + 2 * CHUNK)]);
    }

    #[test]
    fn partition_renderer_rejects_bad_batch_without_changing_state_or_output() {
        assert!(SidPartitionRenderer::new_fast(48_000, 0).is_err());
        assert!(SidPartitionRenderer::new_fast(48_000, 1 << 12).is_err());
        assert!(SidPartitionRenderer::new_fast(0, bit(1)).is_err());
        let mut renderer = SidPartitionRenderer::new_fast(48_000, bit(1)).unwrap();
        let mut out = vec![123];
        let events = [SidEvent::new(SidEvent::ENSURE, 0, bit(1), 0, 0),
            SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 2000), SidEvent::new(SidEvent::WRITE, 0, bit(1), 1, 1000)];
        assert!(renderer.render_into(&events, &mut out).is_err());
        assert!(renderer.order.is_empty());
        assert_eq!(renderer.clk, 0);
        assert_eq!(out, [123]);
    }

    #[test]
    fn dynamic_partition_metadata_bootstrap_matches_late_ensure_and_global_transitions() {
        for rate in [44_100, 48_000, 96_000] {
            let mut reference = SidRenderer::new_fast(rate);
            let mut coordinator = SidPartitionRenderer::new_metadata(rate).unwrap();
            assert_partition_batch(&mut reference, std::slice::from_mut(&mut coordinator),
                &[SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 97)]);
            coordinator.select_receiver(bit(5)).unwrap();
            let mut partitions = vec![coordinator];
            assert_partition_batch(&mut reference, &mut partitions, &partition_tone(5, MODEL_8580, 97));
            let mut cycle = 97;
            for step in 0..35 {
                let mut events = Vec::new();
                match step {
                    2 | 4 => {
                        let rx = if step == 2 { 1 } else { 2 };
                        let mut snapshot = Vec::new();
                        partitions[0].export_metadata(&mut snapshot);
                        assert_eq!(snapshot.len(), SidPartitionRenderer::SNAPSHOT_BYTES);
                        let mut child = SidPartitionRenderer::new_fast(rate, bit(rx)).unwrap();
                        child.import_metadata(&snapshot).unwrap();
                        partitions.push(child);
                        events.extend(partition_tone(rx, MODEL_6581, cycle));
                    },
                    6 => events.push(SidEvent::new(SidEvent::ENSURE, 0, bit(5), MODEL_6581 as u32, cycle)),
                    8 => events.push(SidEvent::new(SidEvent::CLOCK_HZ, 0, 0, 1_022_727, cycle)),
                    10 => events.push(SidEvent::new(SidEvent::DROP, 0, bit(5), 0, cycle)),
                    12 => events.extend(partition_tone(5, MODEL_8580, cycle)),
                    14 => events.push(SidEvent::new(SidEvent::RESET, 0, 0, 0, cycle)),
                    16 => { events.push(SidEvent::new(SidEvent::REANCHOR, 0, 0, 0, 0)); cycle = 0; },
                    18 => {
                        reference.engines.set_rate(48_000);
                        for partition in &mut partitions { partition.set_rate(48_000).unwrap(); }
                    },
                    20 => for rx in [1, 2, 5] { events.push(SidEvent::new(SidEvent::DROP, 0, bit(rx), 0, cycle)); },
                    22 => events.extend(partition_tone(2, MODEL_8580, cycle)),
                    24 => {
                        // Create a fourth worker after rate/clock/reset/reanchor,
                        // preserving the new leader's fractional sample phase.
                        let mut snapshot = Vec::new();
                        partitions[0].export_metadata(&mut snapshot);
                        let mut child = SidPartitionRenderer::new_fast(rate, bit(3)).unwrap();
                        child.import_metadata(&snapshot).unwrap();
                        partitions.push(child);
                        events.extend(partition_tone(3, MODEL_6581, cycle));
                    },
                    _ => {},
                }
                cycle += [997, 211, 1, 19_656][step % 4];
                events.push(SidEvent::new(SidEvent::WRITE, 0x0e, bit(2), (step * 17 % 256) as u32, cycle));
                cycle += 501;
                events.push(SidEvent::new(SidEvent::CLOCK, 0, 0, 0, cycle));
                assert_partition_batch(&mut reference, &mut partitions, &events);
            }
        }
    }

    #[test]
    fn dynamic_partition_metadata_validates_snapshot_before_any_state_changes() {
        let mut source = SidPartitionRenderer::new_fast(48_000, bit(5)).unwrap();
        source.render(&partition_tone(5, MODEL_8580, 0)).unwrap();
        source.render(&[SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 997)]).unwrap();
        let mut snapshot = Vec::new();
        source.export_metadata(&mut snapshot);
        let mut destination = SidPartitionRenderer::new_fast(48_000, bit(1)).unwrap();
        destination.import_metadata(&snapshot).unwrap();
        let mut expected = Vec::new();
        destination.export_metadata(&mut expected);
        assert_eq!(snapshot, expected, "global metadata is independent of local receiver ownership");
        for (offset, value) in [(0, u32::MAX), (4, u32::MAX), (8, u32::MAX), (12, u32::MAX),
            (24, u32::MAX), (32, u32::MAX), (40, u32::MAX), (44, u32::MAX), (48, u32::MAX),
            (57, u32::MAX), (64, 0), (68, u32::MAX), (64 + 5 * 20, u32::MAX),
            (64 + 5 * 20 + 12, u32::MAX), (64 + 5 * 20 + 16, 1)] {
            let mut corrupt = snapshot.clone();
            corrupt[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(destination.import_metadata(&corrupt).is_err(), "offset {offset}");
            let mut actual = Vec::new();
            destination.export_metadata(&mut actual);
            assert_eq!(actual, expected, "invalid metadata cannot partially modify state");
        }
        assert!(destination.import_metadata(&snapshot[..snapshot.len() - 1]).is_err());
        let mut existing = SidPartitionRenderer::new_fast(48_000, bit(5)).unwrap();
        assert!(existing.import_metadata(&snapshot).is_err(), "register metadata cannot bootstrap an already built chip");
        source.render(&[SidEvent::new(SidEvent::DROP, 0, bit(5), 0, 997)]).unwrap();
        source.export_metadata(&mut snapshot);
        existing.render(&partition_tone(5, MODEL_8580, 0)).unwrap();
        assert!(existing.import_metadata(&snapshot).is_err(), "owned analog state cannot be silently overwritten");
        let mut control = SidPartitionRenderer::new_metadata(48_000).unwrap();
        control.import_metadata(&expected).unwrap();
        assert!(control.select_receiver(bit(5)).is_err());
        assert!(control.select_receiver(3).is_err());
        control.select_receiver(bit(1)).unwrap();
        assert!(control.select_receiver(bit(2)).is_err(), "coordinator keeps one receiver identity");
    }

    /// RMS around the mean: steadier than peak-to-peak, whose extremes fall on a different sample every window.
    fn level(pcm: &[i16]) -> f64 {
        let mean = pcm.iter().map(|&s| f64::from(s)).sum::<f64>() / pcm.len() as f64;
        (pcm.iter().map(|&s| (f64::from(s) - mean).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt()
    }

    #[test]
    fn audio_follows_emulated_time_and_carries_the_tone() {
        let mut sid = Sid::new();
        let out = Collect::default();
        let pcm = Rc::clone(&out.0);
        sid.set_audio(48_000, Box::new(out), 0);
        sid.advance(985);
        assert_eq!(pcm.borrow().len(), 47, "no engine yet: 985 cycles of silence all the same");
        assert!(pcm.borrow().iter().all(|&s| s == 0));
        // Default map: UltiSID 1 and 2 at $D400, two engines playing the same.
        tone(&sid, 0xD400, 985);
        for ms in 2..=1000u64 {
            sid.advance(ms * 985);
        }
        assert_eq!(sid.inline().order.len(), 2);
        let pcm = pcm.borrow();
        assert!((47_900..=48_000).contains(&pcm.len()), "985 000 cycles at 48 kHz: {}", pcm.len());
        let hz = hz(&pcm[24_000..], 48_000.0);
        assert!((990.0..1010.0).contains(&hz), "{hz} Hz");
    }

    /// S29: byte `2i` is the left gain, `2i + 1` the right.
    #[test]
    fn a_channel_panned_hard_left_is_silent_on_the_right() {
        #[derive(Default)]
        struct Raw(Rc<RefCell<Vec<i16>>>);
        impl AudioSink for Raw {
            fn samples(&mut self, pcm: &[i16]) {
                self.0.borrow_mut().extend_from_slice(pcm);
            }
        }
        let mut sid = Sid::new();
        player_map(&mut sid, [(0x40, 0xC0), UNMAPPED]);
        let out = Raw::default();
        let pcm = Rc::clone(&out.0);
        sid.set_audio(48_000, Box::new(out), 0);
        sid.mixer_write(0, 0x80);
        sid.mixer_write(1, 0x00);
        tone(&sid, 0xD400, 0);
        sid.advance(300 * 985);
        let pcm = pcm.borrow();
        let tail = &pcm[pcm.len() / 4 * 2..];
        assert!(tail.len() % 2 == 0 && tail.chunks(2).all(|f| f[1] == 0), "nothing on the right");
        let left: Vec<i16> = tail.iter().step_by(2).copied().collect();
        assert!(level(&left) > 1000.0, "the tone on the left: {}", level(&left));
    }

    #[test]
    fn mixer_gains_scale_and_mute() {
        let mut acc = vec![0; 8];
        mix_into(&mut acc, &[100, -200, 300], (SIDE_UNITY, 0));
        mix_into(&mut acc, &[10, 20, 30, 40, 50], (SIDE_UNITY / 2, SIDE_UNITY));
        assert_eq!(
            mixed_down(&acc),
            [105, 10, -190, 20, 315, 30, 320, 40],
            "left and right apart, padded with the last sample, trimmed to the first engine"
        );
        assert_eq!(mixed_down(&[i32::from(i16::MAX) * UNITY]), [i16::MAX], "saturates");

        let mut sid = Sid::new();
        player_map(&mut sid, [(0x40, 0xC0), UNMAPPED]);
        let out = Collect::default();
        let pcm = Rc::clone(&out.0);
        sid.set_audio(48_000, Box::new(out), 0);
        tone(&sid, 0xD400, 0);
        let mut clk = 0;
        let mut run = |sid: &mut Sid, ms: u64| {
            let start = pcm.borrow().len();
            for _ in 0..ms {
                clk += 985;
                sid.advance(clk);
            }
            pcm.borrow()[start + (pcm.borrow().len() - start) / 2..].to_vec()
        };
        let unity = level(&run(&mut sid, 500));
        assert!(unity > 1000.0, "the default mixer's 0 dB centre: {unity}");
        sid.mixer_write(0, 0x2D);
        sid.mixer_write(1, 0x2D);
        let half = level(&run(&mut sid, 500));
        assert!((half / unity - 0.5).abs() < 0.01, "UltiSID 1 at half gain: {half} of {unity}");
        sid.mixer_write(2, 0);
        sid.mixer_write(3, 0);
        let other = level(&run(&mut sid, 200));
        assert!((other / half - 1.0).abs() < 0.01, "UltiSID 2's channel is not UltiSID 1's: {other} vs {half}");
        for off in 0..8 {
            sid.mixer_write(off, 0);
        }
        assert!(run(&mut sid, 100).iter().all(|&s| s == 0), "u64_mute_sids (u64_config.cc:1318-1331)");
    }

    /// Door tests run TRX64 itself: its bus read and peek reach [`answered`].
    fn machine() -> crate::Trx64Backend {
        crate::Trx64Backend::new(Path::new("/nonexistent"))
    }

    #[test]
    fn chip_1_reads_env3_from_its_resid_through_the_door() {
        let mut c64 = machine();
        for (off, val) in [(0x0A, 0x40), (0x0E, 0xFE), (0x0B, 0x42), (0x0F, 0xFE)] {
            c64.core_config_write(off, val);
        }
        assert_eq!(c64.m.sid_map()[1].chip, 1, "UltiSID 2 at $D420 is chip 1");
        c64.sid.set_audio(48_000, Box::new(Collect::default()), c64.m.c64_core.clk);
        // Voice 3 of UltiSID 2: attack 0, sustain 15, gate on.
        c64.m.poke_io(0xD433, &[0x00, 0xF0]);
        c64.m.poke_io(0xD432, &[0x21]);
        let clk = c64.m.c64_core.clk;
        c64.sid.advance(clk + 20_000);
        assert_eq!(c64.m.read_full(0xD43C), 0xFF, "peek: ENV3 of UltiSID 2's reSID after 20 ms of attack 0");
        assert_eq!(c64.m.read_full_live(0xD43C), 0xFF, "bus read");
        assert_eq!(c64.m.read_full_live(0xD41C), 0, "chip 0 is TRX64's own, and nothing played there");
    }

    #[test]
    fn the_armsid_answers_through_the_door() {
        let mut c64 = machine();
        c64.set_sid_socket1(true);
        let detection = [(0x08, 0x40), (0x0C, 0xFE), (0x11, 1), (0x0A, 0x60), (0x0E, 0xFE), (0x0B, 0x60), (0x0F, 0xFE)];
        for (off, val) in detection {
            c64.core_config_write(off, val);
        }
        let chip = c64.m.sid_map()[0].chip;
        assert_ne!(chip, 0, "socket 1 alone at $D400; chip 0 holds the UltiSIDs at $D600");
        c64.m.poke_io(0xD41D, b"SID");
        assert_eq!((c64.m.read_full(0xD41B), c64.m.read_full(0xD41C)), (b'N', b'O'), "peek");
        let bus = (c64.m.read_full_live(0xD41B), c64.m.read_full_live(0xD41C));
        assert_eq!(bus, (b'N', b'O'), "bus read, before any drain");
        assert_eq!(c64.m.read_full_live(0xD61B), 0, "the UltiSIDs are no ARMSID");
        c64.m.poke_io(0xD41D, &[0]);
        assert_eq!(c64.m.read_full_live(0xD41B), 0, "configuration mode left");
    }
}
