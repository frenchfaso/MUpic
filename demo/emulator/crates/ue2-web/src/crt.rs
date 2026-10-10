//! Native Commodore 1702 Guest PAL shader contract. The exact native preset is
//! cross-compiled ahead of time to WebGPU WGSL; no shader compiler ships in WASM.
//! The compiled assets are hashed by this Rust-owned contract.
pub const SHADER: &str = include_str!("../../../web/crt/contract.json");

/// Retained eight-float transport ABI for the browser worker. Native CRT
/// parameters and per-pass uniforms now come from the reflected manifest.
pub static PARAMS: [f32; 8] = [384.0, 272.0, 768.0, 544.0, 0.16, 0.09, 0.035, 1.0];

#[no_mangle]
pub extern "C" fn ue2_crt_shader() -> *const u8 { SHADER.as_ptr() }
#[no_mangle]
pub extern "C" fn ue2_crt_shader_len() -> u32 { SHADER.len() as u32 }
#[no_mangle]
pub extern "C" fn ue2_crt_params() -> *const f32 { PARAMS.as_ptr() }
#[no_mangle]
pub extern "C" fn ue2_crt_params_len() -> u32 { PARAMS.len() as u32 }
