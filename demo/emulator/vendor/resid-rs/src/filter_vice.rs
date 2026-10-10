// Rust translation of the VICE 3.10 reSID 1.0-pre2 NEW_8580_FILTER runtime.
// Copyright (C) 2010 Dag Lem; GPL-2.0-or-later, distributed here under GPL-3.0+.
// Lookup tables retain exact original values. See ../PROVENANCE.md.

use super::ChipModel;
#[cfg(not(feature = "vice-packed-tables"))]
use alloc::{boxed::Box, sync::OnceLock, vec::Vec};
use core::convert::TryInto;
#[path = "data/vice/constants.rs"]
mod constants;

/// Immutable exact table lookup, using signed delta bitplanes per 64 values.
/// The prefix sum is computed with population counts; no heap or decode cache.
struct Table(
    &'static [u8],
    #[cfg(not(feature = "vice-packed-tables"))] OnceLock<Box<[u16]>>,
);
impl Table {
    #[inline]
    fn get(&self, index: usize) -> i32 {
        #[cfg(feature = "vice-packed-tables")]
        {
            self.get_packed(index)
        }
        #[cfg(not(feature = "vice-packed-tables"))]
        {
            self.1.get_or_init(|| self.decode())[index] as i32
        }
    }
    #[cfg(not(feature = "vice-packed-tables"))]
    fn decode(&self) -> Box<[u16]> {
        let count = u32::from_le_bytes(self.0[4..8].try_into().unwrap()) as usize;
        let blocks = u32::from_le_bytes(self.0[8..12].try_into().unwrap()) as usize;
        let mut decoded = Vec::with_capacity(count);
        for block in 0..blocks {
            let directory = 12 + block * 4;
            let start = 12
                + blocks * 4
                + u32::from_le_bytes(self.0[directory..directory + 4].try_into().unwrap()) as usize;
            let width = self.0[start] as usize;
            let entries = (count - block * 64).min(64);
            if width == 255 {
                for bit in 0..entries {
                    let offset = start + 3 + bit * 2;
                    decoded.push(u16::from_le_bytes(
                        self.0[offset..offset + 2].try_into().unwrap(),
                    ));
                }
                continue;
            }
            let mut value =
                u16::from_le_bytes(self.0[start + 1..start + 3].try_into().unwrap()) as i32;
            decoded.push(value as u16);
            let mut planes = [0_u64; 15];
            for plane in 0..width {
                let offset = start + 3 + plane * 8;
                planes[plane] = u64::from_le_bytes(self.0[offset..offset + 8].try_into().unwrap());
            }
            for bit in 0..entries - 1 {
                for plane in 0..width {
                    let weight = if plane + 1 == width {
                        -(1_i32 << plane)
                    } else {
                        1_i32 << plane
                    };
                    value += ((planes[plane] >> bit) & 1) as i32 * weight;
                }
                decoded.push(value as u16);
            }
        }
        decoded.into_boxed_slice()
    }
    #[inline]
    fn get_packed(&self, index: usize) -> i32 {
        let count = u32::from_le_bytes(self.0[4..8].try_into().unwrap()) as usize;
        assert!(index < count, "analog table index out of range");
        let blocks = u32::from_le_bytes(self.0[8..12].try_into().unwrap()) as usize;
        let directory = 12 + (index / 64) * 4;
        let start = 12
            + blocks * 4
            + u32::from_le_bytes(self.0[directory..directory + 4].try_into().unwrap()) as usize;
        let width = self.0[start] as usize;
        let bit = index % 64;
        if width == 255 {
            let offset = start + 3 + bit * 2;
            return u16::from_le_bytes(self.0[offset..offset + 2].try_into().unwrap()) as i32;
        }
        let mut value = u16::from_le_bytes(self.0[start + 1..start + 3].try_into().unwrap()) as i32;
        let prefix = (1_u64 << bit).wrapping_sub(1);
        for plane in 0..width {
            let offset = start + 3 + plane * 8;
            let mask = u64::from_le_bytes(self.0[offset..offset + 8].try_into().unwrap());
            let weight = if plane + 1 == width {
                -(1_i32 << plane)
            } else {
                1_i32 << plane
            };
            value += (mask & prefix).count_ones() as i32 * weight;
        }
        value
    }
}
static MODEL0: Table = Table(
    include_bytes!("data/vice/model0.rdp"),
    #[cfg(not(feature = "vice-packed-tables"))]
    OnceLock::new(),
);
static MODEL1: Table = Table(
    include_bytes!("data/vice/model1.rdp"),
    #[cfg(not(feature = "vice-packed-tables"))]
    OnceLock::new(),
);
static VCR: Table = Table(
    include_bytes!("data/vice/vcr.rdp"),
    #[cfg(not(feature = "vice-packed-tables"))]
    OnceLock::new(),
);
static NOISE: &[u8] = include_bytes!("data/vice/noise.u32le");
const OPAMP: usize = 0;
const SUMMER: usize = 65536;
const GAIN: usize = SUMMER + 20 * 65536;
const RESONANCE: usize = GAIN + 16 * 65536;
const MIXER: usize = RESONANCE + 16 * 65536;
const F0: usize = MIXER + 28 * 65536 + 1;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    feature = "snapshot-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct Filter {
    model: usize,
    enabled: bool,
    fc: u16,
    filt: u8,
    res: u8,
    mode: u8,
    vol: u8,
    voice_mask: u8,
    sum: u8,
    mix: u8,
    pub vhp: i32,
    pub vbp: i32,
    pub vlp: i32,
    bp_x: i32,
    bp_vc: i32,
    lp_x: i32,
    lp_vc: i32,
    voices: [i32; 4],
    bias: f64,
    vw_square: u32,
    n_dac: i32,
    n_vgt: i32,
    noise_index: usize,
}
impl Filter {
    pub fn new(chip_model: ChipModel) -> Self {
        let mut filter = Self {
            model: if matches!(chip_model, ChipModel::Mos6581) {
                0
            } else {
                1
            },
            enabled: true,
            fc: 0,
            filt: 0,
            res: 0,
            mode: 0,
            vol: 0,
            voice_mask: 0xf7,
            sum: 0,
            mix: 0,
            vhp: 0,
            vbp: 0,
            vlp: 0,
            bp_x: 0,
            bp_vc: 0,
            lp_x: 0,
            lp_vc: 0,
            voices: [0; 4],
            bias: 0.0,
            vw_square: 0,
            n_dac: 0,
            n_vgt: 0,
            noise_index: 0,
        };
        filter.adjust_filter_bias(0.0);
        filter.input(0);
        filter.reset();
        filter
    }
    fn table(&self) -> &'static Table {
        if self.model == 0 {
            &MODEL0
        } else {
            &MODEL1
        }
    }
    fn parameters(&self) -> (i32, i32, i32, i32, f64) {
        if self.model == 0 {
            constants::MODEL0
        } else {
            constants::MODEL1
        }
    }
    pub fn get_fc_hi(&self) -> u8 {
        (self.fc >> 3) as u8
    }
    pub fn get_fc_lo(&self) -> u8 {
        (self.fc & 7) as u8
    }
    pub fn get_mode_vol(&self) -> u8 {
        self.mode | self.vol
    }
    pub fn get_res_filt(&self) -> u8 {
        (self.res << 4) | self.filt
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.routing();
    }
    pub fn set_voice_mask(&mut self, mask: u8) {
        self.voice_mask = 0xf0 | (mask & 15);
        self.routing();
    }
    pub fn set_fc_hi(&mut self, value: u8) {
        self.fc = ((value as u16) << 3) | (self.fc & 7);
        self.cutoff();
    }
    pub fn set_fc_lo(&mut self, value: u8) {
        self.fc = (self.fc & 0x7f8) | (value as u16 & 7);
        self.cutoff();
    }
    pub fn set_res_filt(&mut self, value: u8) {
        self.res = value >> 4;
        self.filt = value & 15;
        self.routing();
    }
    pub fn set_mode_vol(&mut self, value: u8) {
        self.mode = value & 0xf0;
        self.vol = value & 15;
        self.routing();
    }
    pub fn adjust_filter_bias(&mut self, bias: f64) {
        if !bias.is_finite() {
            return;
        }
        self.bias = bias;
        self.n_vgt = (constants::MODEL1.4 * (4.7975 * (bias * 6.0 / 100.0 + 1.6) - 0.80 - 1.30)
            + 0.5) as i32;
        self.cutoff();
    }
    fn cutoff(&mut self) {
        // Cutoff DAC lookups alone do not require decoding the other model.
        let voltage =
            (self.bias * constants::MODEL0.4) as i32 + MODEL0.get_packed(F0 + self.fc as usize);
        let diff = constants::MODEL0.0.wrapping_sub(voltage) as u32;
        self.vw_square = diff.wrapping_mul(diff) >> 1;
        self.n_dac =
            constants::N_PARAM.wrapping_mul(MODEL1.get_packed(F0 + self.fc as usize)) >> 11;
    }
    fn routing(&mut self) {
        self.sum = (if self.enabled { self.filt } else { 0 }) & self.voice_mask;
        self.mix = (if self.enabled {
            (self.mode & 0x70) | (!(self.filt | ((self.mode & 0x80) >> 5)) & 15)
        } else {
            15
        }) & self.voice_mask;
    }
    pub fn input(&mut self, sample: i16) {
        let scale = self.parameters().1;
        self.voices[3] =
            ((sample as i32).wrapping_mul(scale).wrapping_mul(3) >> 14) + self.table().get(MIXER);
    }
    fn dither(&mut self) -> i32 {
        self.noise_index = (self.noise_index + 1) & 1023;
        let index = self.noise_index * 4;
        u32::from_le_bytes(NOISE[index..index + 4].try_into().unwrap()) as i32
    }
    fn integrate(&self, dt: i32, vi: i32, mut vx: i32, mut vc: i32) -> (i32, i32, i32) {
        let table = self.table();
        let current = if self.model == 0 {
            let k = constants::MODEL0.0;
            let gst = k.wrapping_sub(vx) as u32;
            let gdt = k.wrapping_sub(vi) as u32;
            let square = gdt.wrapping_mul(gdt);
            let snake = constants::N_SNAKE
                .wrapping_mul((gst.wrapping_mul(gst).wrapping_sub(square) as i32) >> 15);
            let vg = VCR.get((self.vw_square.wrapping_add(square >> 1) >> 16) as usize);
            let vgs = vg - vx + 32768;
            let vgd = vg - vi + 32768;
            let vcr = VCR
                .get(65536 + vgs as usize)
                .wrapping_sub(VCR.get(65536 + vgd as usize))
                .wrapping_shl(15);
            snake.wrapping_add(vcr)
        } else {
            let gst = self.n_vgt.wrapping_sub(vx) as u32;
            let gdt = if vi < self.n_vgt {
                self.n_vgt.wrapping_sub(vi) as u32
            } else {
                0
            };
            self.n_dac.wrapping_mul(
                (gst.wrapping_mul(gst).wrapping_sub(gdt.wrapping_mul(gdt)) as i32) >> 15,
            ) >> 4
        };
        vc = vc.wrapping_sub(current.wrapping_mul(dt));
        vx = table.get(OPAMP + ((vc >> 15) + 32768) as usize);
        (vx + (vc >> 14), vx, vc)
    }
    pub fn clock(&mut self, voice1: i32, voice2: i32, voice3: i32, _ext_in: i32) {
        self.clock_internal(1, [voice1, voice2, voice3], false);
    }
    pub fn clock_delta(&mut self, delta: u32, voice1: i32, voice2: i32, voice3: i32, _ext_in: i32) {
        self.clock_internal(delta, [voice1, voice2, voice3], true);
    }
    fn clock_internal(&mut self, mut delta: u32, voices: [i32; 3], fast: bool) {
        let (_, scale, dc, _, _) = self.parameters();
        for (i, voice) in voices.iter().enumerate() {
            self.voices[i] = (voice.wrapping_mul(scale).wrapping_add(self.dither()) >> 18) + dc;
        }
        if fast && !self.enabled {
            return;
        }
        let mut input = 0;
        let mut count = 0;
        for i in 0..4 {
            if self.sum & (1 << i) != 0 {
                input += self.voices[i];
                count += 1;
            }
        }
        let offset = SUMMER + ((count * (count + 3) / 2) << 16);
        while delta != 0 {
            let dt = if fast { delta.min(3) } else { 1 } as i32;
            let (out, x, vc) = self.integrate(dt, self.vbp, self.lp_x, self.lp_vc);
            self.vlp = out;
            self.lp_x = x;
            self.lp_vc = vc;
            let (out, x, vc) = self.integrate(dt, self.vhp, self.bp_x, self.bp_vc);
            self.vbp = out;
            self.bp_x = x;
            self.bp_vc = vc;
            self.vhp = self.table().get(
                offset
                    + self
                        .table()
                        .get(RESONANCE + self.res as usize * 65536 + self.vbp as usize)
                        as usize
                    + self.vlp as usize
                    + input as usize,
            );
            delta -= dt as u32;
        }
    }
    pub fn output(&self) -> i32 {
        let mut input = 0;
        let mut count = 0;
        for i in 0..4 {
            if self.mix & (1 << i) != 0 {
                input += self.voices[i];
                count += 1;
            }
        }
        let mut filtered = 0;
        let mut filter_count = 0;
        for (i, value) in [self.vlp, self.vbp, self.vhp].iter().enumerate() {
            if self.mix & (0x10 << i) != 0 {
                filtered += value;
                filter_count += 1;
            }
        }
        if filter_count != 0 {
            let gain = self.parameters().3;
            input += (filtered * gain + 32767 * (4096 - gain)) >> 12;
        }
        count += filter_count;
        let offset = if count == 0 {
            0
        } else {
            1 + ((count * (count - 1) / 2) << 16)
        };
        let mixed = self.table().get(MIXER + offset + input as usize);
        self.table()
            .get(GAIN + self.vol as usize * 65536 + mixed as usize)
            - 32768
    }
    pub fn reset(&mut self) {
        self.fc = 0;
        self.filt = 0;
        self.res = 0;
        self.mode = 0;
        self.vol = 0;
        self.vhp = 0;
        self.vbp = 0;
        self.vlp = 0;
        self.bp_x = 0;
        self.bp_vc = 0;
        self.lp_x = 0;
        self.lp_vc = 0;
        self.cutoff();
        self.routing();
    }
}
