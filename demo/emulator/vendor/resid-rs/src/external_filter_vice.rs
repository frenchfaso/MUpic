// VICE reSID output RC stage, translated from extfilt.{cc,h}.
// Copyright (C) 2010 Dag Lem; GPL-2.0-or-later, distributed under GPL-3.0+.
use super::ChipModel;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "snapshot-serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExternalFilter {
    enabled: bool,
    vlp: i32,
    vhp: i32,
}
impl ExternalFilter {
    pub fn new(_model: ChipModel) -> Self { Self { enabled: true, vlp: 0, vhp: 0 } }
    pub fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }
    pub fn clock(&mut self, input: i32) {
        let input = (input as i16 as i32).wrapping_shl(11);
        if !self.enabled { self.vlp = input; self.vhp = 0; return; }
        let low = 12_i32.wrapping_mul(input.wrapping_sub(self.vlp)) >> 7;
        let high = 13_i32.wrapping_mul(self.vlp.wrapping_sub(self.vhp)) >> 17;
        self.vlp = self.vlp.wrapping_add(low);
        self.vhp = self.vhp.wrapping_add(high);
    }
    pub fn clock_delta(&mut self, mut delta: u32, input: i32) {
        let input = (input as i16 as i32).wrapping_shl(11);
        if !self.enabled { self.vlp = input; self.vhp = 0; return; }
        while delta != 0 {
            let dt = delta.min(8) as i32;
            let low = ((12 * dt) >> 3).wrapping_mul(input.wrapping_sub(self.vlp)) >> 4;
            let high = ((13 * dt) >> 3).wrapping_mul(self.vlp.wrapping_sub(self.vhp)) >> 14;
            self.vlp = self.vlp.wrapping_add(low);
            self.vhp = self.vhp.wrapping_add(high);
            delta -= dt as u32;
        }
    }
    pub fn output(&self) -> i32 { self.vlp.wrapping_sub(self.vhp) >> 11 }
    pub fn reset(&mut self) { self.vlp = 0; self.vhp = 0; }
}
