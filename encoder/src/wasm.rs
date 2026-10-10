//! Raw WebAssembly ABI. Each instance owns independent quantizer/encoder state.
//! Pointer inputs must be allocations returned by mup_alloc; copy output before
//! the next mutation and refresh typed views after WebAssembly memory growth.
#![cfg(target_arch = "wasm32")]
use crate::*;
use std::{cell::RefCell, collections::HashMap};

#[derive(Default)]
struct Runtime {
    allocations: HashMap<usize, Vec<u8>>,
    quantizer: Option<Quantizer>,
    encoder: Option<Encoder>,
    result: Vec<u8>,
    error: Vec<u8>,
    gpu_search: Vec<u32>,
}
impl Runtime {
    fn input(&self, pointer: usize, length: usize) -> Result<&[u8], String> {
        if length == 0 {
            return Ok(&[]);
        }
        self.allocations
            .get(&pointer)
            .and_then(|v| v.get(..length))
            .ok_or_else(|| "Input must be a live mup_alloc allocation with valid length".into())
    }
    fn quantizer(&mut self) -> Result<&mut Quantizer, String> {
        if self.quantizer.is_none() {
            self.quantizer = Some(Quantizer::new(1)?);
        }
        Ok(self.quantizer.as_mut().unwrap())
    }
}
thread_local! { static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default()); }
fn operation(f: impl FnOnce(&mut Runtime) -> Result<(), String>) -> i32 {
    RUNTIME.with(|r| {
        let mut runtime = r.borrow_mut();
        match f(&mut runtime) {
            Ok(()) => {
                runtime.error.clear();
                0
            }
            Err(e) => {
                runtime.error = e.into_bytes();
                1
            }
        }
    })
}
#[no_mangle]
pub extern "C" fn mup_alloc(length: usize) -> usize {
    if length == 0 {
        return 0;
    }
    RUNTIME.with(|r| {
        let mut v = vec![0; length];
        let pointer = v.as_mut_ptr() as usize;
        r.borrow_mut().allocations.insert(pointer, v);
        pointer
    })
}
#[no_mangle]
pub extern "C" fn mup_free(pointer: usize, _length: usize) {
    RUNTIME.with(|r| {
        r.borrow_mut().allocations.remove(&pointer);
    });
}
#[no_mangle]
pub extern "C" fn mup_begin(
    frames: usize,
    pcm: usize,
    pcm_length: usize,
    title: usize,
    title_length: usize,
) -> i32 {
    operation(|r| {
        let pcm = r.input(pcm, pcm_length)?.to_vec();
        let title = std::str::from_utf8(r.input(title, title_length)?)
            .map_err(|_| "Title must be ASCII")?
            .to_owned();
        r.encoder = Some(Encoder::new(frames, pcm, title)?);
        r.result.clear();
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn mup_quantize(pointer: usize, length: usize) -> i32 {
    operation(|r| {
        let input = r.input(pointer, length)?.to_vec();
        r.result = r.quantizer()?.quantize(&input)?;
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn mup_quantize_seeded(
    pointer: usize,
    length: usize,
    seeds: usize,
    seeds_length: usize,
) -> i32 {
    operation(|r| {
        let rgb = r.input(pointer, length)?.to_vec();
        if rgb.len() != RGB_BYTES || seeds_length != VIDEO_WIDTH * VIDEO_HEIGHT * 4 {
            return Err("GPU seed array must contain one u32 per pixel".into());
        }
        let seeds: Vec<u32> = r
            .input(seeds, seeds_length)?
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let q = r.quantizer()?;
        q.prepare_cache(rgb.len())?;
        q.author_mut().preplan_rgb_seeded(&rgb, &seeds)?;
        r.result = q.pack_plans(&rgb);
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn mup_gpu_search_ptr() -> usize {
    let status = operation(|r| {
        if r.gpu_search.is_empty() {
            r.gpu_search = r.quantizer()?.author_mut().gpu_search_data();
        }
        Ok(())
    });
    if status != 0 {
        return 0;
    }
    RUNTIME.with(|r| r.borrow().gpu_search.as_ptr() as usize)
}
#[no_mangle]
pub extern "C" fn mup_gpu_search_len() -> usize {
    RUNTIME.with(|r| r.borrow().gpu_search.len() * 4)
}
#[no_mangle]
pub extern "C" fn mup_encode_packed(pointer: usize, length: usize) -> i32 {
    operation(|r| {
        let packed = r.input(pointer, length)?.to_vec();
        r.encoder
            .as_mut()
            .ok_or("Call mup_begin before encoding")?
            .push(&packed)
    })
}
#[no_mangle]
pub extern "C" fn mup_finish() -> i32 {
    operation(|r| {
        r.result = r
            .encoder
            .as_mut()
            .ok_or("Call mup_begin before finishing")?
            .finish()?;
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn mup_result_ptr() -> usize {
    RUNTIME.with(|r| r.borrow().result.as_ptr() as usize)
}
#[no_mangle]
pub extern "C" fn mup_result_len() -> usize {
    RUNTIME.with(|r| r.borrow().result.len())
}
#[no_mangle]
pub extern "C" fn mup_error_ptr() -> usize {
    RUNTIME.with(|r| r.borrow().error.as_ptr() as usize)
}
#[no_mangle]
pub extern "C" fn mup_error_len() -> usize {
    RUNTIME.with(|r| r.borrow().error.len())
}
#[no_mangle]
pub extern "C" fn mup_output_ptr() -> usize {
    RUNTIME.with(|r| {
        r.borrow()
            .encoder
            .as_ref()
            .map_or(0, |e| e.output().as_ptr() as usize)
    })
}
#[no_mangle]
pub extern "C" fn mup_output_len() -> usize {
    RUNTIME.with(|r| r.borrow().encoder.as_ref().map_or(0, |e| e.output().len()))
}
#[no_mangle]
pub extern "C" fn mup_output_clear() {
    RUNTIME.with(|r| {
        if let Some(e) = &mut r.borrow_mut().encoder {
            e.clear_output();
        }
    });
}
#[no_mangle]
pub extern "C" fn mup_preview_ptr() -> usize {
    RUNTIME.with(|r| {
        r.borrow()
            .encoder
            .as_ref()
            .map_or(0, |e| e.preview().as_ptr() as usize)
    })
}
#[no_mangle]
pub extern "C" fn mup_preview_len() -> usize {
    RUNTIME.with(|r| r.borrow().encoder.as_ref().map_or(0, |e| e.preview().len()))
}
#[no_mangle]
pub extern "C" fn mup_peak_slots() -> usize {
    RUNTIME.with(|r| r.borrow().encoder.as_ref().map_or(0, |e| e.peak_slots))
}
#[no_mangle]
pub extern "C" fn mup_adapted_frames() -> usize {
    RUNTIME.with(|r| r.borrow().encoder.as_ref().map_or(0, |e| e.adapted_frames))
}
#[no_mangle]
pub extern "C" fn mup_received_frames() -> usize {
    RUNTIME.with(|r| r.borrow().encoder.as_ref().map_or(0, |e| e.received()))
}
