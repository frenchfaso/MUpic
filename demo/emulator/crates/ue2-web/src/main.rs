//! All-Rust machine and SID entry points for independent browser workers.
//! Standalone WASM imports host clocks and logging; file storage is Rust.
mod crt;
mod remote;
use std::{cell::RefCell, collections::BTreeMap, ffi::CString, path::PathBuf, rc::Rc};
use anyhow::{ensure, Context, Result};
use c64_bridge::{AudioSink, SidEvent, SidRenderer, Trx64Backend};
#[cfg(feature = "sid-partition")]
use c64_bridge::SidPartitionRenderer;
use ue2_core::{Machine, MachineConfig, RunExit};
use ue2_core::{devices::{board::Board, c64::C64Port}, host::HostInput, render::Renderer};

const STOP: u32 = 0x1004_0001;
const AUDIO_LIMIT: usize = 2 * 96_000;
const MAX_ASSET_BYTES: u32 = 64 << 20;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[link(wasm_import_module = "ue2_host")]
extern "C" {
    #[link_name = "log"]
    fn browser_log(ptr: *const u8, len: usize);
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn host_log(bytes: &[u8]) {
    unsafe { browser_log(bytes.as_ptr(), bytes.len()); }
}

fn install_panic_hook() {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| std::panic::set_hook(Box::new(|info| host_log(info.to_string().as_bytes()))));
    }
}

#[derive(Clone)]
struct BrowserAudio(Rc<RefCell<Vec<i16>>>);
impl AudioSink for BrowserAudio {
    fn samples(&mut self, pcm: &[i16]) {
        let mut buffer = self.0.borrow_mut();
        if buffer.len() + pcm.len() > AUDIO_LIMIT { buffer.clear(); }
        buffer.extend_from_slice(pcm);
    }
}

struct Browser {
    machine: Machine,
    renderer: Renderer,
    pixels: Vec<u32>,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    frame_time: f64,
    audio: BrowserAudio,
    audio_out: Vec<i16>,
    mupic_pending: bool,
    remote_disk: bool,
    sid_events: Vec<SidEvent>,
    #[cfg(target_endian = "big")]
    sid_event_bytes: Vec<u8>,
}

impl Browser {
    fn new(rate: u32) -> Result<Self> {
        ensure!((8_000..=96_000).contains(&rate), "Invalid audio sample rate");
        let mut cfg = MachineConfig::new("/firmware.ue2".into(), "/roms".into());
        cfg.flash_image = Some(PathBuf::from("/flash.bin"));
        cfg.board = Board::C64U;
        cfg.settings.push("/mupic.cfg".into());
        cfg.usb.storage_slots = 1;
        cfg.capabilities |= ue2_core::devices::usb::CAPAB_USB_HOST2;
        let mut machine = Machine::new(cfg)?;
        machine.usb_attach_storage(1, remote::backend()?).map_err(anyhow::Error::msg)?;
        let audio = BrowserAudio(Rc::new(RefCell::new(Vec::new())));
        let mut backend = Trx64Backend::new(&machine.cfg.rom_dir);
        backend.set_sid_socket1(false);
        // The machine keeps digital SID readback and the RAM-coupled sampler.
        // Analogue SID synthesis runs in a separate WASM worker.
        backend.set_audio_external(rate, Box::new(audio.clone()));
        machine.attach_c64(Box::new(backend));
        if let Some(itu) = machine.bus.io.get_mut::<ue2_core::devices::itu::Itu>() {
            itu.capabilities |= c64_bridge::CAPAB_EEPROM
                | c64_bridge::CAPAB_COMMAND_INTF | c64_bridge::CAPAB_SAMPLER;
        }
        let font = ue2_host::fs::read("/roms/chars.bin").context("reading overlay font")?;
        Ok(Self { machine, renderer: Renderer::new(&font), pixels: Vec::new(),
            rgba: Vec::new(), width: 0, height: 0, frame_time: 0.0, audio, audio_out: Vec::new(),
            mupic_pending: true, remote_disk: true,
            sid_events: Vec::with_capacity(1024),
            #[cfg(target_endian = "big")]
            sid_event_bytes: Vec::new() })
    }

    fn load_prg(&mut self, path: &str, entry: u32) -> Result<()> {
        let bytes = ue2_host::fs::read(path).context("reading PRG")?;
        ensure!(bytes.len() >= 3, "PRG needs a load address and payload");
        let address = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        ensure!(address + bytes.len() - 2 <= 0x10000, "PRG exceeds C64 RAM");
        ensure!(entry <= 0xffff, "Entry exceeds C64 RAM");
        let entry = if entry == 0 { sys_entry(&bytes).unwrap_or(address as u16) } else { entry as u16 };
        self.machine.bus.poke_io8(STOP, 1);
        let port = self.machine.bus.io.get_mut::<C64Port>().context("missing C64 port")?;
        let backend = port.backend_mut().context("missing C64 backend")?;
        for (offset, value) in bytes[2..].iter().enumerate() {
            backend.dma_write((address + offset) as u16, *value, true);
        }
        let trx = backend.as_any_mut().and_then(|any| any.downcast_mut::<Trx64Backend>())
            .context("missing TRX64 backend")?;
        trx.trx64().c64_core.reg_pc = entry;
        self.machine.bus.poke_io8(STOP, 0);
        Ok(())
    }

    fn step(&mut self, instructions: u32) -> Result<u64> {
        // Wall-clock network latency must not consume guest USB timeouts, PCM
        // buffers or video deadlines. The worker resumes after the range settles.
        if self.remote_disk && remote::waiting() { return Ok(self.machine.now_ms()); }
        let deadline = self.machine.bus.now.saturating_add(4 * ue2_core::time::CLOCKS_PER_MS);
        let budget = u64::from(instructions.clamp(1, 100_000));
        let exit = if self.remote_disk {
            self.machine.run_until_when(budget, deadline, remote::waiting)
        } else { self.machine.run_until(budget, deadline) };
        if let RunExit::Halted(message) = exit {
            anyhow::bail!("Firmware halted: {message}");
        }
        if self.mupic_pending && !remote::waiting() && self.machine.now_ms() >= 6_000 {
            self.load_prg("/mupic.prg", 0x080e)?;
            self.mupic_pending = false;
        }
        // Avoid an unbounded firmware console buffer when no terminal is attached.
        self.machine.drain_console();
        Ok(self.machine.now_ms())
    }

    fn render(&mut self) -> *const u8 {
        self.frame_time = self.machine.bus.now as f64 / ue2_core::time::CLOCKS_PER_MS as f64;
        let snapshot = self.machine.display();
        // Feed Guest the same analogue raster as the native CRT presenter,
        // including firmware overlays. An HDMI-sized menu would otherwise
        // change the scanline density and reset PAL feedback on every toggle.
        let raster_size = Renderer::raster_size(&snapshot);
        self.renderer.render_scaled(&snapshot, &mut self.pixels, raster_size);
        let (width, height) = raster_size;
        self.width = width as u32;
        self.height = height as u32;
        self.rgba.resize(width * height * 4, 0);
        for (pixel, dst) in self.pixels.iter().zip(self.rgba.chunks_exact_mut(4)) {
            dst.copy_from_slice(&[(*pixel >> 16) as u8, (*pixel >> 8) as u8, *pixel as u8, 255]);
        }
        self.rgba.as_ptr()
    }

    fn drain_sid(&mut self) -> *const u8 {
        self.sid_events.clear();
        if let Some(trx) = self.machine.bus.io.get_mut::<C64Port>()
            .and_then(C64Port::backend_mut)
            .and_then(|b| b.as_any_mut())
            .and_then(|any| any.downcast_mut::<Trx64Backend>()) {
            trx.drain_sid_events_into(&mut self.sid_events);
        }
        #[cfg(target_endian = "little")]
        { self.sid_events.as_ptr().cast() }
        #[cfg(target_endian = "big")]
        {
            self.sid_event_bytes.clear();
            for event in &self.sid_events {
                self.sid_event_bytes.extend_from_slice(&event.to_le_bytes());
            }
            self.sid_event_bytes.as_ptr()
        }
    }
}

fn sys_entry(prg: &[u8]) -> Option<u16> {
    if prg.get(..2)? != [1, 8] { return None; }
    let basic = &prg[2..prg.len().min(82)];
    let start = basic.iter().position(|byte| *byte == 0x9e)? + 1;
    let digits: Vec<u8> = basic[start..].iter().copied().skip_while(|byte| *byte == b' ')
        .take_while(u8::is_ascii_digit).collect();
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

thread_local! {
    static BROWSER: RefCell<Option<Browser>> = const { RefCell::new(None) };
    static ERROR: RefCell<CString> = RefCell::new(CString::new("").unwrap());
    static SID: RefCell<Option<BrowserSid>> = const { RefCell::new(None) };
    // Asset ownership remains in Rust. This also validates callers before any
    // raw pointer is read or freed across the browser ABI.
    static ASSETS: RefCell<BTreeMap<usize, Box<[u8]>>> = const { RefCell::new(BTreeMap::new()) };
}
struct BrowserSid {
    renderer: SidRenderer,
    #[cfg(feature = "sid-partition")]
    partition: Option<SidPartitionRenderer>,
    events: Vec<SidEvent>,
    pcm: Vec<i16>,
    sampler: Vec<i16>,
    #[cfg(feature = "sid-partition")]
    raw: Vec<i32>,
    #[cfg(feature = "sid-partition")]
    mix_input: Vec<i32>,
    #[cfg(feature = "sid-partition")]
    mix_acc: Vec<i32>,
    #[cfg(feature = "sid-partition")]
    mix_count: u32,
    #[cfg(feature = "sid-partition")]
    partition_snapshot: Vec<u8>,
}
fn fail(error: impl std::fmt::Display) -> i32 {
    ERROR.with(|value| *value.borrow_mut() = CString::new(error.to_string().replace('\0', " ")).unwrap());
    -1
}
fn with_browser<T>(operation: impl FnOnce(&mut Browser) -> T) -> Option<T> {
    BROWSER.with(|value| value.borrow_mut().as_mut().map(operation))
}

#[no_mangle]
pub extern "C" fn ue2_init(rate: u32) -> i32 {
    install_panic_hook();
    match Browser::new(rate) {
        Ok(browser) => { BROWSER.with(|value| *value.borrow_mut() = Some(browser)); 0 }
        Err(error) => fail(format!("{error:#}")),
    }
}
/// Configure the read-only remote image before initializing MUpic.
#[no_mangle]
pub extern "C" fn ue2_remote_disk(bytes: u32) -> i32 {
    match remote::configure(bytes) { Ok(()) => 0, Err(error) => fail(error) }
}
#[no_mangle]
pub extern "C" fn ue2_remote_waiting() -> u32 { u32::from(remote::waiting()) }
/// Call only after every outstanding browser range request has settled.
#[no_mangle]
pub extern "C" fn ue2_remote_resume() -> i32 { remote::resume(); 0 }
#[no_mangle]
pub extern "C" fn ue2_step(instructions: u32) -> f64 {
    match with_browser(|browser| browser.step(instructions)) {
        Some(Ok(ms)) => ms as f64,
        Some(Err(error)) => f64::from(fail(format!("{error:#}"))),
        None => f64::from(fail("Emulator is not initialized")),
    }
}
#[no_mangle]
pub extern "C" fn ue2_render() -> *const u8 { with_browser(Browser::render).unwrap_or(std::ptr::null()) }
#[no_mangle]
pub extern "C" fn ue2_width() -> u32 { with_browser(|b| b.width).unwrap_or(0) }
#[no_mangle]
pub extern "C" fn ue2_height() -> u32 { with_browser(|b| b.height).unwrap_or(0) }
/// VIC publish counter, independent of browser timers and GPU draw calls.
#[no_mangle]
pub extern "C" fn ue2_frame_id() -> f64 { with_browser(|b| b.machine.frame_counter() as f64).unwrap_or(0.0) }
/// Exact emulated time of the last stable completed-frame capture.
#[no_mangle]
pub extern "C" fn ue2_frame_time() -> f64 { with_browser(|b| b.frame_time).unwrap_or(0.0) }
#[no_mangle]
pub extern "C" fn ue2_audio() -> *const i16 {
    with_browser(|b| {
        let mut queued = b.audio.0.borrow_mut();
        std::mem::swap(&mut b.audio_out, &mut *queued);
        queued.clear();
        b.audio_out.as_ptr()
    })
        .unwrap_or(std::ptr::null())
}
#[no_mangle]
pub extern "C" fn ue2_audio_samples() -> u32 { with_browser(|b| b.audio_out.len() as u32).unwrap_or(0) }
#[no_mangle]
pub extern "C" fn ue2_key(index: u32, down: u32) {
    if index < 64 { with_browser(|b| b.machine.input(HostInput::Key { row: (index >> 3) as u8, col: (index & 7) as u8, down: down != 0 })); }
}
#[no_mangle]
pub extern "C" fn ue2_joystick(port: u32, lines: u32) {
    if (1..=2).contains(&port) && lines <= u8::MAX as u32 {
        with_browser(|b| b.machine.input(HostInput::JoystickPort { port: port as u8, lines: lines as u8 }));
    }
}
#[no_mangle]
pub extern "C" fn ue2_menu(down: u32) { with_browser(|b| b.machine.input(HostInput::MenuButton(down != 0))); }
#[no_mangle]
pub extern "C" fn ue2_restore(down: u32) { with_browser(|b| b.machine.input(HostInput::Restore(down != 0))); }
#[no_mangle]
pub extern "C" fn ue2_error() -> *const std::ffi::c_char { ERROR.with(|value| value.borrow().as_ptr()) }
/// Allocate zeroed browser staging bytes. Every nonzero allocation must be
/// released using its original length; registration copies the bytes.
#[no_mangle]
pub extern "C" fn ue2_alloc(len: u32) -> *mut u8 {
    if len == 0 || len > MAX_ASSET_BYTES {
        fail("Asset allocation must contain 1 byte to 64 MiB");
        return std::ptr::null_mut();
    }
    let mut bytes = Vec::new();
    if let Err(error) = bytes.try_reserve_exact(len as usize) {
        fail(format!("Asset allocation failed: {error}"));
        return std::ptr::null_mut();
    }
    bytes.resize(len as usize, 0);
    let mut bytes = bytes.into_boxed_slice();
    let ptr = bytes.as_mut_ptr();
    ASSETS.with(|assets| assets.borrow_mut().insert(ptr as usize, bytes));
    ptr
}

#[no_mangle]
pub extern "C" fn ue2_dealloc(ptr: *mut u8, len: u32) {
    if ptr.is_null() { return; }
    ASSETS.with(|assets| {
        let mut assets = assets.borrow_mut();
        if assets.get(&(ptr as usize)).is_some_and(|bytes| bytes.len() == len as usize) {
            assets.remove(&(ptr as usize));
        } else { fail("Unknown asset allocation or incorrect allocation length"); }
    });
}

/// Register UTF-8 path and data windows created by `ue2_alloc` in the Rust
/// filesystem. Returns 0 on success, -1 with `ue2_error()` on failure.
#[no_mangle]
pub extern "C" fn ue2_file_register(path_ptr: *const u8, path_len: u32, data_ptr: *const u8, data_len: u32) -> i32 {
    if path_len == 0 || path_len > 4096 || data_len > MAX_ASSET_BYTES {
        return fail("Invalid asset path or data length");
    }
    ASSETS.with(|assets| {
        let assets = assets.borrow();
        let Some(path) = assets.get(&(path_ptr as usize)).and_then(|bytes| bytes.get(..path_len as usize))
            else { return fail("Asset path is outside its staging allocation"); };
        let Ok(path) = std::str::from_utf8(path) else { return fail("Asset path is not UTF-8"); };
        if path.contains('\0') { return fail("Asset path contains NUL"); }
        let data = if data_len == 0 { &[][..] } else {
            let Some(data) = assets.get(&(data_ptr as usize)).and_then(|bytes| bytes.get(..data_len as usize))
                else { return fail("Asset data is outside its staging allocation"); };
            data
        };
        match ue2_host::fs::register(path, data) { Ok(()) => 0, Err(error) => fail(error) }
    })
}

#[no_mangle]
pub extern "C" fn ue2_sid_events() -> *const u8 { with_browser(Browser::drain_sid).unwrap_or(std::ptr::null()) }
#[no_mangle]
pub extern "C" fn ue2_sid_events_bytes() -> u32 { with_browser(|b| (b.sid_events.len() * std::mem::size_of::<SidEvent>()) as u32).unwrap_or(0) }
fn initialize_sid(rate: u32) -> i32 {
    install_panic_hook();
    if !(8_000..=96_000).contains(&rate) { return fail("Invalid SID rate"); }
    let renderer = SidRenderer::new_fast(rate);
    SID.with(|slot| *slot.borrow_mut() = Some(BrowserSid { renderer,
        events: Vec::new(), pcm: Vec::new(), sampler: Vec::new(),
        #[cfg(feature = "sid-partition")] partition: None,
        #[cfg(feature = "sid-partition")] raw: Vec::new(),
        #[cfg(feature = "sid-partition")] mix_input: Vec::new(),
        #[cfg(feature = "sid-partition")] mix_acc: Vec::new(),
        #[cfg(feature = "sid-partition")] mix_count: 0,
        #[cfg(feature = "sid-partition")] partition_snapshot: Vec::new() }));
    0
}
#[no_mangle]
pub extern "C" fn ue2_sid_init(rate: u32) -> i32 { initialize_sid(rate) }
/// Experimental one-SID worker. All timestamp boundaries must be broadcast;
/// only this receiver is synthesized. Output is raw i32 stereo mixer units.
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_init(rate: u32, receiver_mask: u32) -> i32 {
    if receiver_mask.count_ones() != 1 || receiver_mask > u32::from(u16::MAX) {
        return fail("SID partition must select exactly one receiver");
    }
    let partition = match SidPartitionRenderer::new_fast(rate, receiver_mask as u16) {
        Ok(partition) => partition, Err(error) => return fail(error),
    };
    if initialize_sid(rate) != 0 { return -1; }
    SID.with(|slot| slot.borrow_mut().as_mut().unwrap().partition = Some(partition));
    0
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_control_init(rate: u32) -> i32 {
    let partition = match SidPartitionRenderer::new_metadata(rate) {
        Ok(partition) => partition, Err(error) => return fail(error),
    };
    if initialize_sid(rate) != 0 { return -1; }
    SID.with(|slot| slot.borrow_mut().as_mut().unwrap().partition = Some(partition));
    0
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_select(receiver_mask: u32) -> i32 {
    if receiver_mask > u32::from(u16::MAX) { return fail("Invalid SID receiver mask"); }
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(partition) = slot.as_mut().and_then(|s| s.partition.as_mut()) else { return fail("SID partition is not initialized"); };
        match partition.select_receiver(receiver_mask as u16) { Ok(()) => 0, Err(error) => fail(error) }
    })
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_snapshot() -> *const u8 {
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { return std::ptr::null(); };
        let Some(partition) = &s.partition else { return std::ptr::null(); };
        partition.export_metadata(&mut s.partition_snapshot);
        s.partition_snapshot.as_ptr()
    })
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_snapshot_bytes() -> u32 { SidPartitionRenderer::SNAPSHOT_BYTES as u32 }
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_snapshot_alloc(bytes: u32) -> *mut u8 {
    if bytes as usize != SidPartitionRenderer::SNAPSHOT_BYTES { fail("Invalid SID snapshot size"); return std::ptr::null_mut(); }
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { return std::ptr::null_mut(); };
        s.partition_snapshot.resize(bytes as usize, 0);
        s.partition_snapshot.as_mut_ptr()
    })
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_snapshot_import(bytes: u32) -> i32 {
    if bytes as usize != SidPartitionRenderer::SNAPSHOT_BYTES { return fail("Invalid SID snapshot size"); }
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { return fail("SID partition is not initialized"); };
        let Some(partition) = &mut s.partition else { return fail("SID partition is not initialized"); };
        match partition.import_metadata(&s.partition_snapshot) { Ok(()) => 0, Err(error) => fail(error) }
    })
}
/// A mixer instance has no active reSID engines or analog tables.
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_mixer_init(rate: u32) -> i32 { initialize_sid(rate) }
#[no_mangle]
pub extern "C" fn ue2_sid_alloc(bytes: u32) -> *mut u32 {
    if bytes > 1_048_576 || bytes % 16 != 0 { fail("Invalid SID event buffer size"); return std::ptr::null_mut(); }
    SID.with(|slot| slot.borrow_mut().as_mut().map(|s| {
        s.events.resize(bytes as usize / 16, SidEvent::from_words([0; 4]));
        s.events.as_mut_ptr().cast()
    }).unwrap_or(std::ptr::null_mut()))
}
#[no_mangle]
pub extern "C" fn ue2_sid_process(bytes: u32) -> i32 {
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { return fail("SID is not initialized"); };
        if bytes % 16 != 0 || bytes as usize / 16 != s.events.len() { return fail("SID buffer size differs"); }
        #[cfg(feature = "sid-partition")]
        let result = if let Some(partition) = &mut s.partition {
            partition.render_into(&s.events, &mut s.raw)
        } else {
            s.renderer.render_into(&s.events, &mut s.pcm)
        };
        #[cfg(not(feature = "sid-partition"))]
        let result = s.renderer.render_into(&s.events, &mut s.pcm);
        match result { Ok(()) => 0, Err(e) => fail(e) }
    })
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_audio() -> *const i32 {
    SID.with(|slot| slot.borrow().as_ref().filter(|s| s.partition.is_some())
        .map(|s| s.raw.as_ptr()).unwrap_or(std::ptr::null()))
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_partition_audio_samples() -> u32 {
    SID.with(|slot| slot.borrow().as_ref().filter(|s| s.partition.is_some())
        .map_or(0, |s| s.raw.len() as u32))
}
/// Clear the common accumulator once for the next interleaved stereo batch.
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_mixer_begin(samples: u32) -> i32 {
    if samples > 384_000 || samples % 2 != 0 { return fail("Invalid SID mixer batch size"); }
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { return fail("SID mixer is not initialized"); };
        s.mix_acc.clear();
        s.mix_acc.resize(samples as usize, 0);
        s.pcm.resize(samples as usize, 0);
        s.mix_input.clear();
        s.mix_count = 0;
        0
    })
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_mixer_input_alloc(samples: u32) -> *mut i32 {
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { fail("SID mixer is not initialized"); return std::ptr::null_mut(); };
        if samples as usize != s.mix_acc.len() { fail("SID partition/mixer clocks differ"); return std::ptr::null_mut(); }
        s.mix_input.resize(samples as usize, 0);
        s.mix_input.as_mut_ptr()
    })
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_mixer_add(samples: u32) -> i32 {
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { return fail("SID mixer is not initialized"); };
        if samples as usize != s.mix_acc.len() || s.mix_input.len() != s.mix_acc.len() {
            return fail("SID partition/mixer clocks differ");
        }
        if s.mix_count == 9 || s.mix_input.iter().any(|&input| !(-8_355_840..=8_355_840).contains(&input)) {
            return fail("SID mixer input exceeds one receiver's range or receiver count");
        }
        // At most nine hardware receivers, each i16 * 255. Their complete sum
        // fits i32; keeping this precision prevents cancellation being clipped.
        for (sum, &input) in s.mix_acc.iter_mut().zip(&s.mix_input) { *sum += input; }
        s.mix_count += 1;
        0
    })
}
#[no_mangle]
#[cfg(feature = "sid-partition")]
pub extern "C" fn ue2_sid_mixer_finish(samples: u32) -> i32 {
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { return fail("SID mixer is not initialized"); };
        if samples as usize != s.mix_acc.len() || s.sampler.len() != s.mix_acc.len() {
            return fail("SID/sampler clocks differ");
        }
        for ((pcm, &sum), &sampler) in s.pcm.iter_mut().zip(&s.mix_acc).zip(&s.sampler) {
            let sid = (sum / 90).clamp(i16::MIN as i32, i16::MAX as i32);
            *pcm = (sid + i32::from(sampler)).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
        0
    })
}
#[no_mangle]
pub extern "C" fn ue2_sid_sampler_alloc(samples: u32) -> *mut i16 {
    if samples > 384_000 { fail("Sampler batch exceeds two seconds"); return std::ptr::null_mut(); }
    SID.with(|slot| slot.borrow_mut().as_mut().map(|s| {
        s.sampler.resize(samples as usize, 0); s.sampler.as_mut_ptr()
    }).unwrap_or(std::ptr::null_mut()))
}
#[no_mangle]
pub extern "C" fn ue2_sid_mix(samples: u32) -> i32 {
    SID.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(s) = slot.as_mut() else { return fail("SID is not initialized"); };
        if samples as usize != s.sampler.len() || s.sampler.len() != s.pcm.len() {
            return fail(format!("SID/sampler clocks differ: {} vs {} samples", s.pcm.len(), s.sampler.len()));
        }
        for (sid, sampler) in s.pcm.iter_mut().zip(&s.sampler) {
            *sid = (i32::from(*sid) + i32::from(*sampler)).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
        0
    })
}
#[no_mangle]
pub extern "C" fn ue2_sid_audio() -> *const i16 { SID.with(|s| s.borrow().as_ref().map(|s| s.pcm.as_ptr()).unwrap_or(std::ptr::null())) }
#[no_mangle]
pub extern "C" fn ue2_sid_audio_samples() -> u32 { SID.with(|s| s.borrow().as_ref().map(|s| s.pcm.len() as u32).unwrap_or(0)) }

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_browser() -> Browser {
        let cfg = MachineConfig::new(PathBuf::new(), PathBuf::new());
        let mut bus = ue2_core::bus::SystemBus::new();
        bus.ram[..4].copy_from_slice(&0x0000_006fu32.to_le_bytes()); // Firmware stub: j .
        bus.io.add(0x1004_0000, 0x1_0000, Box::new(C64Port::new()));
        let mut machine = Machine::from_parts(cfg, bus, 0, ue2_core::symbols::Symbols::empty());
        machine.attach_c64(Box::new(Trx64Backend::new(std::path::Path::new("/nonexistent"))));
        Browser {
            machine, renderer: Renderer::new(&[0; 2048]), pixels: Vec::new(), rgba: Vec::new(),
            width: 0, height: 0, frame_time: 0.0, audio: BrowserAudio(Rc::new(RefCell::new(Vec::new()))),
            audio_out: Vec::new(), mupic_pending: false, remote_disk: false, sid_events: Vec::new(),
            #[cfg(target_endian = "big")]
            sid_event_bytes: Vec::new(),
        }
    }

    #[test]
    fn remote_network_hold_preserves_guest_time_and_cpu_until_resume() {
        let mut browser = isolated_browser();
        browser.remote_disk = true;
        BROWSER.with(|value| *value.borrow_mut() = Some(browser));
        assert_eq!(ue2_remote_disk(536_870_912), 0);
        assert_eq!(ue2_remote_disk(513), -1);
        let before = with_browser(|b| (b.machine.bus.now, b.machine.cpu.pc, b.machine.cpu.insns)).unwrap();
        remote::hold_for_test();
        assert_eq!(ue2_remote_waiting(), 1);
        for _ in 0..20 {
            assert_eq!(ue2_step(100_000), 0.0);
            assert_eq!(with_browser(|b| (b.machine.bus.now, b.machine.cpu.pc, b.machine.cpu.insns)).unwrap(), before);
        }
        ue2_remote_resume();
        assert_eq!(ue2_remote_waiting(), 0);
        assert!(ue2_step(100_000) > 0.0);
        assert!(with_browser(|b| b.machine.cpu.insns).unwrap() > before.2);
        BROWSER.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    fn browser_video_abi_observes_consecutive_completed_vic_frames() {
        BROWSER.with(|value| *value.borrow_mut() = Some(isolated_browser()));
        let mut observed = Vec::new();
        let mut previous_time = 0.0;
        for _ in 0..125 {
            let now = ue2_step(100_000);
            assert!(now >= previous_time && now - previous_time <= 4.0);
            previous_time = now;
            let frame = ue2_frame_id();
            if observed.last().copied() != Some(frame) {
                observed.push(frame);
                assert!(!ue2_render().is_null());
                assert!(ue2_frame_time() >= now && ue2_frame_time() < now + 1.0);
            }
        }
        assert!(observed.len() >= 24, "PAL VIC must publish at least 24 frames in half a second");
        assert_eq!(observed, (0..observed.len()).map(|i| i as f64).collect::<Vec<_>>());
        let complete = with_browser(|browser| browser.rgba.clone()).unwrap();
        with_browser(|browser| {
            let backend = browser.machine.bus.io.get_mut::<C64Port>().unwrap().backend_mut().unwrap();
            let trx = backend.as_any_mut().unwrap().downcast_mut::<Trx64Backend>().unwrap().trx64();
            trx.vic.dbuf.fill(3); // In-progress next raster must not leak into capture.
        }).unwrap();
        ue2_render();
        assert_eq!(with_browser(|browser| browser.rgba.clone()).unwrap(), complete);
        BROWSER.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    fn browser_crt_overlay_retains_native_raster_and_composition() {
        use ue2_core::devices::overlay::{self, Overlay};

        let mut browser = isolated_browser();
        let mut layer = Overlay::new();
        layer.regs[overlay::REG_CHAR_WIDTH] = 8;
        layer.regs[overlay::REG_CHAR_HEIGHT] = 8;
        layer.regs[overlay::REG_CHARS_PER_LINE] = 80;
        layer.regs[overlay::REG_ACTIVE_LINES] = 25;
        layer.color.fill(0x11);
        layer.palette[4..7].copy_from_slice(&[128, 64, 192]);
        layer.hdmi[3] = (720 / 8) as u8;
        layer.hdmi[11] = (576 / 8) as u8;
        browser.machine.bus.io.add(overlay::BASE, overlay::SIZE, Box::new(layer));
        BROWSER.with(|value| *value.borrow_mut() = Some(browser));

        let mut captures = Vec::new();
        for visibility in [0, 0x80, 0] {
            let (size, expected) = with_browser(|browser| {
                browser.machine.bus.io.get_mut::<Overlay>().unwrap()
                    .regs[overlay::REG_TRANSPARENCY] = visibility;
                let snapshot = browser.machine.display();
                let size = Renderer::raster_size(&snapshot);
                assert_eq!(Renderer::canvas(&snapshot), (720, 576));
                assert_ne!(Renderer::canvas(&snapshot), size,
                    "fixture must distinguish HDMI/overlay canvas from analogue raster");
                let mut pixels = Vec::new();
                Renderer::new(&[0; 2048]).render_scaled(&snapshot, &mut pixels, size);
                let rgba = pixels.iter().flat_map(|pixel|
                    [(*pixel >> 16) as u8, (*pixel >> 8) as u8, *pixel as u8, 255]
                ).collect::<Vec<_>>();
                (size, rgba)
            }).unwrap();
            assert!(!ue2_render().is_null());
            assert_eq!((ue2_width() as usize, ue2_height() as usize), size,
                "overlay visibility must not change CRT source raster");
            let actual = with_browser(|browser| browser.rgba.clone()).unwrap();
            assert_eq!(actual, expected, "same composited bitmap as native CRT input");
            captures.push(actual);
        }
        assert_ne!(captures[0], captures[1], "visible overlay must actually be rendered");
        assert_eq!(captures[0], captures[2], "hiding overlay must restore original picture");
        BROWSER.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    fn sd_crt_rgba_preserves_the_full_raster_for_local_and_remote_storage() {
        use ue2_core::devices::overlay::Overlay;
        let mut browser = isolated_browser();
        let mut layer = Overlay::new();
        // PAL SD: 384x288 picture scaled 15/8 horizontally and 2 vertically
        // fills the 720x576 output without HDMI pillarboxes.
        layer.hdmi[3] = 90;
        layer.hdmi[11] = 72;
        layer.hdmi[19] = 0x04;
        layer.hdmi[20] = 0x00;
        layer.cropper = [8, 0, 192, 144];
        browser.machine.bus.io.add(ue2_core::devices::overlay::BASE,
            ue2_core::devices::overlay::SIZE, Box::new(layer));
        let snapshot = browser.machine.display();
        assert_eq!(Renderer::canvas(&snapshot), (720, 576));
        let size = Renderer::raster_size(&snapshot);
        assert_eq!(size, (384, 288));
        let mut pixels = Vec::new();
        Renderer::new(&[0; 2048]).render_scaled(&snapshot, &mut pixels, size);
        let expected: Vec<u8> = pixels.iter().flat_map(|pixel|
            [(*pixel >> 16) as u8, (*pixel >> 8) as u8, *pixel as u8, 255]
        ).collect();
        for remote_disk in [false, true] {
            browser.remote_disk = remote_disk;
            browser.render();
            assert_eq!((browser.width, browser.height), (384, 288));
            assert_eq!(browser.rgba, expected, "every native raster pixel reaches the CRT, regardless of storage backend");
            assert_eq!(browser.pixels.len(), 384 * 288);
            assert_eq!(browser.rgba.len(), 384 * 288 * 4);
        }
    }

    #[test]
    fn joystick_browser_abi_reaches_both_trx64_ports_and_releases() {
        // A real C64 backend with no firmware/ROM fixture dependency. Exercise
        // the exported ABI through Machine::input and C64Port's host lines.
        let browser = isolated_browser();
        BROWSER.with(|value| *value.borrow_mut() = Some(browser));
        ue2_joystick(2, 0xea); // Forward, strafe left, use/fire.
        ue2_joystick(1, 0xf7); // Independent look right.
        ue2_joystick(0, 0);
        ue2_joystick(3, 0);
        ue2_joystick(2, 256);
        with_browser(|browser| {
            let backend = browser.machine.bus.io.get_mut::<C64Port>().unwrap().backend_mut().unwrap();
            let trx = backend.as_any_mut().unwrap().downcast_mut::<Trx64Backend>().unwrap().trx64();
            assert!(trx.joystick2.up && trx.joystick2.left && trx.joystick2.fire);
            assert!(!trx.joystick2.down && !trx.joystick2.right);
            assert!(trx.joystick1.right);
            assert!(!trx.joystick1.up && !trx.joystick1.down && !trx.joystick1.left && !trx.joystick1.fire);
        }).unwrap();
        ue2_joystick(1, 0xff);
        ue2_joystick(2, 0xff);
        with_browser(|browser| {
            let backend = browser.machine.bus.io.get_mut::<C64Port>().unwrap().backend_mut().unwrap();
            let trx = backend.as_any_mut().unwrap().downcast_mut::<Trx64Backend>().unwrap().trx64();
            for joystick in [&trx.joystick1, &trx.joystick2] {
                assert!(!joystick.up && !joystick.down && !joystick.left && !joystick.right && !joystick.fire);
            }
        }).unwrap();
        with_browser(|browser| browser.machine.input(HostInput::Paddles {
            values: Some([[171, 0xa5], [22, 30]]),
        }));
        with_browser(|browser| {
            let backend = browser.machine.bus.io.get_mut::<C64Port>().unwrap().backend_mut().unwrap();
            backend.core_config_write(0x34, 77); // Firmware values stay behind the host adapter.
            let trx = backend.as_any_mut().unwrap().downcast_mut::<Trx64Backend>().unwrap().trx64();
            assert_eq!(trx.pot(1), Some((171, 0xa5)));
            assert_eq!(trx.pot(2), Some((22, 30)));
            trx.write_full(0x0000, 0x2f);
            trx.write_full(0x0001, 0x37);
            trx.write_full(0xdc02, 0xc0);
            trx.write_full(0xdc00, 0x40); // Select control port 1.
            trx.c64_core.clk += 512;
            assert_eq!((trx.pot_peek(0), trx.pot_peek(1)), (171, 0xa5));
            assert_eq!((trx.read_full(0xd419), trx.read_full(0xd41a)), (171, 0xa5));
            trx.write_full(0xdc00, 0x80); // Select control port 2.
            trx.c64_core.clk += 512;
            assert_eq!((trx.read_full(0xd419), trx.read_full(0xd41a)), (22, 30));
        }).unwrap();
        with_browser(|browser| browser.machine.input(HostInput::Paddles { values: None }));
        with_browser(|browser| {
            let backend = browser.machine.bus.io.get_mut::<C64Port>().unwrap().backend_mut().unwrap();
            let trx = backend.as_any_mut().unwrap().downcast_mut::<Trx64Backend>().unwrap().trx64();
            assert_eq!(trx.pot(1), Some((0, 0)), "firmware's unmodified port 1 restored");
            assert_eq!(trx.pot(2), Some((77, 0)), "host release restores firmware POT values");
        }).unwrap();
        BROWSER.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    fn sys_entry_accepts_basic_loader_and_rejects_non_decimal_or_truncated_data() {
        assert_eq!(sys_entry(&[1,8,11,8,10,0,0x9e,b'2',b'0',b'6',b'2',0]), Some(2062));
        assert_eq!(sys_entry(&[1,8,11,8,10,0,0x9e,b' ',b'4',b'0',b'9',b'6',0]), Some(4096));
        assert_eq!(sys_entry(&[1,8,0x9e,b'X',0]), None);
        assert_eq!(sys_entry(&[1]), None);
        assert_eq!(sys_entry(&[0,16,0x9e,b'1',0]), None);
    }

    #[test]
    fn pcm_abi_swaps_owned_buffers_without_replay_or_mutating_the_last_drain() {
        let mut browser = isolated_browser();
        browser.audio_out = Vec::with_capacity(16);
        *browser.audio.0.borrow_mut() = Vec::with_capacity(16);
        let buffers = [browser.audio_out.as_ptr(), browser.audio.0.borrow().as_ptr()];
        BROWSER.with(|value| *value.borrow_mut() = Some(browser));
        for batch in 0..16 {
            let expected = [batch, -batch, i16::MAX, i16::MIN];
            with_browser(|b| b.audio.samples(&expected)).unwrap();
            let ptr = ue2_audio();
            assert!(buffers.contains(&ptr));
            assert_eq!(ue2_audio_samples(), 4);
            assert_eq!(unsafe { std::slice::from_raw_parts(ptr, 4) }, expected);
            // A new sink callback owns the other buffer and leaves this ABI
            // pointer valid until the next ue2_audio call.
            with_browser(|b| b.audio.samples(&[17, -17])).unwrap();
            assert_eq!(unsafe { std::slice::from_raw_parts(ptr, 4) }, expected);
            let ptr = ue2_audio();
            assert_eq!(ue2_audio_samples(), 2);
            assert_eq!(unsafe { std::slice::from_raw_parts(ptr, 2) }, [17, -17]);
            ue2_audio();
            assert_eq!(ue2_audio_samples(), 0, "empty drain cannot repeat PCM");
        }
        BROWSER.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    fn sid_word_abi_writes_typed_fifo_and_rejects_inconsistent_batch_sizes() {
        assert_eq!(ue2_sid_init(48_000), 0);
        assert!(ue2_sid_alloc(15).is_null());
        let ptr = ue2_sid_alloc(16);
        assert!(!ptr.is_null());
        let event = SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 19_656);
        // The real JS adapter writes u32 words into this same ABI allocation.
        unsafe { std::ptr::copy_nonoverlapping(event.words().as_ptr(), ptr, 4); }
        assert_eq!(ue2_sid_process(32), -1);
        assert_eq!(ue2_sid_process(16), 0);
        assert!(ue2_sid_audio_samples() > 0);
        let again = ue2_sid_alloc(16);
        assert_eq!(ptr, again, "same-size batches retain the allocation");
        let event = SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 39_312);
        unsafe { std::ptr::copy_nonoverlapping(event.words().as_ptr(), again, 4); }
        assert_eq!(ue2_sid_process(16), 0);
        SID.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    #[cfg(feature = "sid-partition")]
    fn sid_partition_abi_matches_combined_and_mixes_sampler_after_sid_clipping() {
        for rate in [44_100, 48_000, 96_000] {
            let mut events = Vec::new();
            for rx in [5, 1, 2] {
                let mask = 1 << rx;
                events.push(SidEvent::new(SidEvent::ENSURE, 0, mask, (rx % 2) as u32, 0));
                events.push(SidEvent::new(SidEvent::GAINS, 0, mask, 255 | (255 << 16), 0));
                for (reg, value) in [(0x18, 15), (0x0e, 0xb1), (0x0f, 0x19), (0x13, 0), (0x14, 0xf0), (0x12, 0x21)] {
                    events.push(SidEvent::new(SidEvent::WRITE, reg, mask, value, 0));
                }
            }
            events.push(SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 19_656));
            let bytes = (events.len() * 16) as u32;
            let put_events = || {
                let ptr = ue2_sid_alloc(bytes);
                assert!(!ptr.is_null());
                unsafe { std::ptr::copy_nonoverlapping(events.as_ptr(), ptr.cast::<SidEvent>(), events.len()); }
                assert_eq!(ue2_sid_process(bytes), 0);
            };
            assert_eq!(ue2_sid_init(rate), 0);
            put_events();
            let samples = ue2_sid_audio_samples();
            let expected = unsafe { std::slice::from_raw_parts(ue2_sid_audio(), samples as usize).to_vec() };
            let mut raw = Vec::new();
            for rx in [5, 1, 2] {
                assert_eq!(ue2_sid_partition_init(rate, 1 << rx), 0);
                put_events();
                assert_eq!(ue2_sid_partition_audio_samples(), samples);
                raw.push(unsafe { std::slice::from_raw_parts(ue2_sid_partition_audio(), samples as usize).to_vec() });
            }
            // The final partition instance is also the mixer: no fourth SID
            // worker/instance and no reset of its synthesis state are needed.
            assert_eq!(ue2_sid_mixer_begin(samples), 0);
            for contribution in raw {
                let ptr = ue2_sid_mixer_input_alloc(samples);
                unsafe { std::ptr::copy_nonoverlapping(contribution.as_ptr(), ptr, contribution.len()); }
                assert_eq!(ue2_sid_mixer_add(samples), 0);
            }
            let sampler = ue2_sid_sampler_alloc(samples);
            for sample in 0..samples as usize { unsafe { *sampler.add(sample) = ((sample % 17) as i16 - 8) * 1000; } }
            assert_eq!(ue2_sid_mixer_finish(samples), 0);
            let actual = unsafe { std::slice::from_raw_parts(ue2_sid_audio(), samples as usize) };
            for (sample, (&actual, &sid)) in actual.iter().zip(&expected).enumerate() {
                let sampler = ((sample % 17) as i32 - 8) * 1000;
                assert_eq!(actual, (i32::from(sid) + sampler).clamp(i16::MIN as i32, i16::MAX as i32) as i16);
            }
            // Mixer calls cannot damage the coordinator SID's next interval.
            let expected_next = SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 39_312);
            let ptr = ue2_sid_alloc(16);
            unsafe { std::ptr::copy_nonoverlapping(expected_next.words().as_ptr(), ptr, 4); }
            assert_eq!(ue2_sid_process(16), 0);
            assert!(ue2_sid_partition_audio_samples() > 0);
        }
        assert_eq!(ue2_sid_partition_init(48_000, 3), -1, "ABI assigns exactly one SID per worker");
        SID.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    #[cfg(feature = "sid-partition")]
    fn sid_partition_mixer_preserves_cancellation_and_rejects_inconsistent_input() {
        assert_eq!(ue2_sid_mixer_init(48_000), 0);
        assert_eq!(ue2_sid_mixer_begin(3), -1);
        assert_eq!(ue2_sid_mixer_begin(4), 0);
        assert!(ue2_sid_mixer_input_alloc(2).is_null());
        assert_eq!(ue2_sid_mixer_add(4), -1);
        for contribution in [[5_000_000, -5_000_000, 5_000_000, -5_000_000], [-4_950_000, 4_950_000, 0, 0]] {
            let ptr = ue2_sid_mixer_input_alloc(4);
            unsafe { std::ptr::copy_nonoverlapping(contribution.as_ptr(), ptr, 4); }
            assert_eq!(ue2_sid_mixer_add(4), 0);
        }
        assert_eq!(ue2_sid_mixer_finish(4), -1, "Ultimate PCM length must match");
        let sampler = ue2_sid_sampler_alloc(4);
        unsafe { std::ptr::copy_nonoverlapping([1000i16, -1000, -20_000, 20_000].as_ptr(), sampler, 4); }
        assert_eq!(ue2_sid_mixer_finish(4), 0);
        assert_eq!(unsafe { std::slice::from_raw_parts(ue2_sid_audio(), 4) }, [1555, -1555, 12_767, -12_768]);
        let ptr = ue2_sid_mixer_input_alloc(4);
        unsafe { *ptr = i32::MAX; }
        assert_eq!(ue2_sid_mixer_add(4), -1, "invalid input rejected before accumulator mutation");
        assert_eq!(ue2_sid_mixer_finish(4), 0);
        assert_eq!(unsafe { std::slice::from_raw_parts(ue2_sid_audio(), 4) }, [1555, -1555, 12_767, -12_768]);
        SID.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    #[cfg(feature = "sid-partition")]
    fn sid_partition_metadata_abi_imports_exact_global_clock_before_late_ensure() {
        assert_eq!(ue2_sid_partition_control_init(48_000), 0);
        assert_eq!(ue2_sid_partition_select(1 << 5), 0);
        let events = [SidEvent::new(SidEvent::ENSURE, 0, 1 << 5, 1, 0),
            SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 997)];
        let ptr = ue2_sid_alloc(32);
        unsafe { std::ptr::copy_nonoverlapping(events.as_ptr(), ptr.cast::<SidEvent>(), 2); }
        assert_eq!(ue2_sid_process(32), 0);
        let bytes = ue2_sid_partition_snapshot_bytes();
        assert_eq!(bytes, 244);
        let snapshot = unsafe { std::slice::from_raw_parts(ue2_sid_partition_snapshot(), bytes as usize).to_vec() };
        assert_eq!(ue2_sid_partition_init(48_000, 1 << 1), 0);
        assert!(ue2_sid_partition_snapshot_alloc(bytes - 1).is_null());
        let ptr = ue2_sid_partition_snapshot_alloc(bytes);
        unsafe { std::ptr::copy_nonoverlapping(snapshot.as_ptr(), ptr, bytes as usize); }
        assert_eq!(ue2_sid_partition_snapshot_import(bytes), 0);
        assert_eq!(unsafe { std::slice::from_raw_parts(ue2_sid_partition_snapshot(), bytes as usize) }, snapshot);
        let events = [SidEvent::new(SidEvent::ENSURE, 0, 1 << 1, 1, 997),
            SidEvent::new(SidEvent::CLOCK, 0, 0, 0, 19_656)];
        let ptr = ue2_sid_alloc(32);
        unsafe { std::ptr::copy_nonoverlapping(events.as_ptr(), ptr.cast::<SidEvent>(), 2); }
        assert_eq!(ue2_sid_process(32), 0);
        assert!(ue2_sid_partition_audio_samples() > 0);
        // An export containing this owned SID cannot be reimported as if it
        // were sufficient to recreate oscillator/envelope/filter phase.
        ue2_sid_partition_snapshot();
        assert_eq!(ue2_sid_partition_snapshot_import(bytes), -1);
        SID.with(|value| *value.borrow_mut() = None);
    }

    #[test]
    fn asset_abi_copies_file_and_validates_staging_ownership() {
        let path = std::env::temp_dir().join(format!("ue2-asset-abi-{}.bin", std::process::id()));
        let path_bytes = path.to_str().unwrap().as_bytes();
        let path_ptr = ue2_alloc(path_bytes.len() as u32);
        let data_ptr = ue2_alloc(4);
        assert!(!path_ptr.is_null() && !data_ptr.is_null());
        unsafe {
            std::ptr::copy_nonoverlapping(path_bytes.as_ptr(), path_ptr, path_bytes.len());
            std::ptr::copy_nonoverlapping([0, 0x7f, 0x80, 0xff].as_ptr(), data_ptr, 4);
        }
        // Incorrect release length preserves the allocation for recovery.
        ue2_dealloc(data_ptr, 3);
        assert_eq!(ue2_file_register(path_ptr, path_bytes.len() as u32, data_ptr, 4), 0);
        ue2_dealloc(data_ptr, 4);
        ue2_dealloc(path_ptr, path_bytes.len() as u32);
        assert_eq!(ue2_host::fs::read(&path).unwrap(), [0, 0x7f, 0x80, 0xff]);
        assert_eq!(ue2_file_register(path_ptr, path_bytes.len() as u32, data_ptr, 4), -1);
        assert_eq!(ue2_file_register(std::ptr::null(), 1, std::ptr::null(), 1), -1);
        ue2_host::fs::remove_file(path).unwrap();
    }

    #[test]
    fn asset_abi_rejects_invalid_utf8_and_oversized_windows() {
        assert!(ue2_alloc(0).is_null());
        assert!(ue2_alloc(MAX_ASSET_BYTES + 1).is_null());
        let path_ptr = ue2_alloc(1);
        unsafe { *path_ptr = 0xff; }
        assert_eq!(ue2_file_register(path_ptr, 1, std::ptr::null(), 0), -1);
        unsafe { *path_ptr = b'a'; }
        assert_eq!(ue2_file_register(path_ptr, 2, std::ptr::null(), 0), -1);
        ue2_dealloc(path_ptr, 1);
    }
}
