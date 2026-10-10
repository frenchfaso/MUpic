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
    fn input_u32(&self, pointer: usize, length: usize) -> Result<Vec<u32>, String> {
        if !length.is_multiple_of(4) {
            return Err("RGB keys and plans must contain whole u32 values".into());
        }
        Ok(self
            .input(pointer, length)?
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect())
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
pub extern "C" fn mup_prepare_keys(pointer: usize, length: usize) -> i32 {
    operation(|r| {
        let rgb = r.input(pointer, length)?.to_vec();
        r.result = r
            .quantizer()?
            .prepare_keys(&rgb)?
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn mup_plan_keys(keys: usize, keys_length: usize) -> i32 {
    operation(|r| {
        let keys = r.input_u32(keys, keys_length)?;
        r.result = r
            .quantizer()?
            .author_mut()
            .plan_keys(&keys)?
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn mup_import_plans(
    keys: usize,
    keys_length: usize,
    plans: usize,
    plans_length: usize,
) -> i32 {
    operation(|r| {
        let keys = r.input_u32(keys, keys_length)?;
        let plans = r.input_u32(plans, plans_length)?;
        r.quantizer()?.import_plans(&keys, &plans)
    })
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
