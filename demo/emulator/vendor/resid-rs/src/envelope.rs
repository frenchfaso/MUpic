// This file is part of resid-rs.
// Copyright (c) 2017-2019 Sebastian Jastrzebski <sebby2k@gmail.com>. All rights reserved.
// Portions (c) 2004 Dag Lem <resid@nimrod.no>
// Licensed under the GPLv3. See LICENSE file in the project root for full license text.

#![cfg_attr(feature = "cargo-clippy", allow(clippy::cast_lossless))]

use super::ChipModel;
use bit_field::BitField;

const RATE_COUNTER_MASK: u16 = 0x7fff;
const RATE_COUNTER_MSB_MASK: u16 = 0x8000;

// Rate counter periods are calculated from the Envelope Rates table in
// the Programmer's Reference Guide. The rate counter period is the number of
// cycles between each increment of the envelope counter.
// The rates have been verified by sampling ENV3.
//
// The rate counter is a 16 bit register which is incremented each cycle.
// When the counter reaches a specific comparison value, the envelope counter
// is incremented (attack) or decremented (decay/release) and the
// counter is zeroed.
//
// NB! Sampling ENV3 shows that the calculated values are not exact.
// It may seem like most calculated values have been rounded (.5 is rounded
// down) and 1 has beed added to the result. A possible explanation for this
// is that the SID designers have used the calculated values directly
// as rate counter comparison values, not considering a one cycle delay to
// zero the counter. This would yield an actual period of comparison value + 1.
//
// The time of the first envelope count can not be exactly controlled, except
// possibly by resetting the chip. Because of this we cannot do cycle exact
// sampling and must devise another method to calculate the rate counter
// periods.
//
// The exact rate counter periods can be determined e.g. by counting the number
// of cycles from envelope level 1 to envelope level 129, and dividing the
// number of cycles by 128. CIA1 timer A and B in linked mode can perform
// the cycle count. This is the method used to find the rates below.
//
// To avoid the ADSR delay bug, sampling of ENV3 should be done using
// sustain = release = 0. This ensures that the attack state will not lower
// the current rate counter period.
//
// The ENV3 sampling code below yields a maximum timing error of 14 cycles.
//     lda #$01
// l1: cmp $d41c
//     bne l1
//     ...
//     lda #$ff
// l2: cmp $d41c
//     bne l2
//
// This yields a maximum error for the calculated rate period of 14/128 cycles.
// The described method is thus sufficient for exact calculation of the rate
// periods.
//
static RATE_COUNTER_PERIOD: [u16; 16] = [
    9,     // 2ms*1.0MHz/256 = 7.81
    32,    // 8ms*1.0MHz/256 = 31.25
    63,    // 16ms*1.0MHz/256 = 62.50
    95,    // 24ms*1.0MHz/256 = 93.75
    149,   // 38ms*1.0MHz/256 = 148.44
    220,   // 56ms*1.0MHz/256 = 218.75
    267,   // 68ms*1.0MHz/256 = 265.63
    313,   // 80ms*1.0MHz/256 = 312.50
    392,   // 100ms*1.0MHz/256 = 390.63
    977,   // 250ms*1.0MHz/256 = 976.56
    1954,  // 500ms*1.0MHz/256 = 1953.13
    3126,  // 800ms*1.0MHz/256 = 3125.00
    3907,  // 1 s*1.0MHz/256 =  3906.25
    11720, // 3 s*1.0MHz/256 = 11718.75
    19532, // 5 s*1.0MHz/256 = 19531.25
    31251, // 8 s*1.0MHz/256 = 31250.00
];

/// From the sustain levels it follows that both the low and high 4 bits of the
/// envelope counter are compared to the 4-bit sustain value.
/// This has been verified by sampling ENV3.
// VICE separates the comparator from its one-cycle counter reset pipeline.
// The 2004 implementation included that cycle in each period constant.
fn period(rate: u8) -> u16 {
    RATE_COUNTER_PERIOD[rate as usize] - if cfg!(feature = "vice-analog") { 1 } else { 0 }
}

static SUSTAIN_LEVEL: [u8; 16] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    feature = "snapshot-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum State {
    Attack,
    DecaySustain,
    Release,
}

/// Envelope counters and phase pipelines, excluding model-dependent static DAC data.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    feature = "snapshot-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct EnvelopeRuntimeState {
    pub state: State,
    pub envelope_counter: u8,
    pub exponential_counter: u8,
    pub exponential_counter_period: u8,
    pub hold_zero: bool,
    pub rate_counter: u16,
    pub rate_counter_period: u16,
    pub env3: u8,
    pub next_state: State,
    pub envelope_pipeline: u8,
    pub exponential_pipeline: u8,
    pub state_pipeline: u8,
    pub reset_rate_counter: bool,
}

/// A 15 bit counter is used to implement the envelope rates, in effect
/// dividing the clock to the envelope counter by the currently selected rate
/// period.
/// In addition, another counter is used to implement the exponential envelope
/// decay, in effect further dividing the clock to the envelope counter.
/// The period of this counter is set to 1, 2, 4, 8, 16, 30 at the envelope
/// counter values 255, 93, 54, 26, 14, 6, respectively.
#[derive(Clone, Copy)]
pub struct EnvelopeGenerator {
    chip_model: ChipModel,
    env3: u8,
    next_state: State,
    envelope_pipeline: u8,
    exponential_pipeline: u8,
    state_pipeline: u8,
    reset_rate_counter: bool,
    // Configuration
    attack: u8,
    decay: u8,
    sustain: u8,
    release: u8,
    // Control
    gate: bool,
    // Runtime State
    pub state: State,
    pub envelope_counter: u8,
    pub exponential_counter: u8,
    pub exponential_counter_period: u8,
    pub hold_zero: bool,
    pub rate_counter: u16,
    pub rate_counter_period: u16,
}

impl Default for EnvelopeGenerator {
    fn default() -> Self {
        let mut envelope = EnvelopeGenerator {
            chip_model: ChipModel::Mos6581,
            env3: 0,
            next_state: State::Release,
            envelope_pipeline: 0,
            exponential_pipeline: 0,
            state_pipeline: 0,
            reset_rate_counter: false,
            attack: 0,
            decay: 0,
            sustain: 0,
            release: 0,
            gate: false,
            state: State::Release,
            envelope_counter: if cfg!(feature = "vice-analog") {
                0xaa
            } else {
                0
            },
            exponential_counter: 0,
            exponential_counter_period: 0,
            hold_zero: false,
            rate_counter: 0,
            rate_counter_period: 0,
        };
        envelope.reset();
        envelope
    }
}

impl EnvelopeGenerator {
    pub fn read_runtime_state(&self) -> EnvelopeRuntimeState {
        EnvelopeRuntimeState {
            state: self.state,
            envelope_counter: self.envelope_counter,
            exponential_counter: self.exponential_counter,
            exponential_counter_period: self.exponential_counter_period,
            hold_zero: self.hold_zero,
            rate_counter: self.rate_counter,
            rate_counter_period: self.rate_counter_period,
            env3: self.env3,
            next_state: self.next_state,
            envelope_pipeline: self.envelope_pipeline,
            exponential_pipeline: self.exponential_pipeline,
            state_pipeline: self.state_pipeline,
            reset_rate_counter: self.reset_rate_counter,
        }
    }

    pub fn write_runtime_state(&mut self, state: &EnvelopeRuntimeState) {
        self.state = state.state;
        self.envelope_counter = state.envelope_counter;
        self.exponential_counter = state.exponential_counter;
        self.exponential_counter_period = state.exponential_counter_period;
        self.hold_zero = state.hold_zero;
        self.rate_counter = state.rate_counter;
        self.rate_counter_period = state.rate_counter_period;
        self.env3 = state.env3;
        self.next_state = state.next_state;
        self.envelope_pipeline = state.envelope_pipeline;
        self.exponential_pipeline = state.exponential_pipeline;
        self.state_pipeline = state.state_pipeline;
        self.reset_rate_counter = state.reset_rate_counter;
    }

    pub fn set_chip_model(&mut self, model: ChipModel) {
        self.chip_model = model;
    }
    pub fn get_attack_decay(&self) -> u8 {
        self.attack << 4 | self.decay
    }

    pub fn get_control(&self) -> u8 {
        let mut value = 0u8;
        value.set_bit(0, self.gate);
        value
    }

    pub fn get_sustain_release(&self) -> u8 {
        self.sustain << 4 | self.release
    }

    pub fn set_attack_decay(&mut self, value: u8) {
        self.attack = (value >> 4) & 0x0f;
        self.decay = value & 0x0f;
        match self.state {
            State::Attack => self.rate_counter_period = period(self.attack),
            State::DecaySustain => {
                self.rate_counter_period = period(self.decay)
            }
            _ => {}
        }
    }

    pub fn set_control(&mut self, value: u8) {
        let gate = value.get_bit(0);
        if cfg!(feature = "vice-analog") {
            if self.gate != gate {
                self.next_state = if gate { State::Attack } else { State::Release };
                if gate {
                    self.state = State::DecaySustain;
                    self.rate_counter_period = period(self.decay);
                    self.state_pipeline = 2;
                    if self.reset_rate_counter || self.exponential_pipeline == 2 {
                        self.envelope_pipeline = if self.exponential_counter_period == 1
                            || self.exponential_pipeline == 2
                        {
                            2
                        } else {
                            4
                        };
                    } else if self.exponential_pipeline == 1 {
                        self.state_pipeline = 3;
                    }
                } else {
                    self.state_pipeline = if self.envelope_pipeline > 0 { 3 } else { 2 };
                }
                self.gate = gate;
            }
            return;
        }
        if !self.gate && gate {
            // Gate bit on: Start attack, decay, sustain.
            self.state = State::Attack;
            self.rate_counter_period = period(self.attack);
            // Switching to attack state unlocks the zero freeze.
            self.hold_zero = false;
        } else if self.gate && !gate {
            // Gate bit off: Start release.
            self.state = State::Release;
            self.rate_counter_period = period(self.release);
        }
        self.gate = gate;
    }

    pub fn set_sustain_release(&mut self, value: u8) {
        self.sustain = (value >> 4) & 0x0f;
        self.release = value & 0x0f;
        if self.state == State::Release {
            self.rate_counter_period = period(self.release);
        }
    }

    #[inline]
    pub fn clock(&mut self) {
        if cfg!(feature = "vice-analog") {
            self.clock_vice();
            return;
        }
        // Check for ADSR delay bug.
        // If the rate counter comparison value is set below the current value of the
        // rate counter, the counter will continue counting up until it wraps around
        // to zero at 2^15 = 0x8000, and then count rate_period - 1 before the
        // envelope can finally be stepped.
        // This has been verified by sampling ENV3.
        self.rate_counter += 1;
        if self.rate_counter & RATE_COUNTER_MSB_MASK != 0 {
            self.rate_counter += 1;
            self.rate_counter &= RATE_COUNTER_MASK;
        }
        if self.rate_counter == self.rate_counter_period {
            self.rate_counter = 0;
            // The first envelope step in the attack state also resets the exponential
            // counter. This has been verified by sampling ENV3.
            self.exponential_counter += 1; // TODO check w/ ref impl
            if self.state == State::Attack
                || self.exponential_counter == self.exponential_counter_period
            {
                self.exponential_counter = 0;
                // Check whether the envelope counter is frozen at zero.
                if self.hold_zero {
                    return;
                }
                match self.state {
                    State::Attack => {
                        // The envelope counter can flip from 0xff to 0x00 by changing state to
                        // release, then to attack. The envelope counter is then frozen at
                        // zero; to unlock this situation the state must be changed to release,
                        // then to attack. This has been verified by sampling ENV3.
                        self.envelope_counter += 1;
                        if self.envelope_counter == 0xff {
                            self.state = State::DecaySustain;
                            self.rate_counter_period = period(self.decay);
                        }
                    }
                    State::DecaySustain => {
                        if self.envelope_counter != SUSTAIN_LEVEL[self.sustain as usize] {
                            self.envelope_counter -= 1;
                        }
                    }
                    State::Release => {
                        // The envelope counter can flip from 0x00 to 0xff by changing state to
                        // attack, then to release. The envelope counter will then continue
                        // counting down in the release state.
                        // This has been verified by sampling ENV3.
                        // NB! The operation below requires two's complement integer.
                        self.envelope_counter -= 1;
                    }
                }
                // Check for change of exponential counter period.
                match self.envelope_counter {
                    0xff => self.exponential_counter_period = 1,
                    0x5d => self.exponential_counter_period = 2,
                    0x36 => self.exponential_counter_period = 4,
                    0x1a => self.exponential_counter_period = 8,
                    0x0e => self.exponential_counter_period = 16,
                    0x06 => self.exponential_counter_period = 30,
                    0x00 => {
                        self.exponential_counter_period = 1;
                        // When the envelope counter is changed to zero, it is frozen at zero.
                        // This has been verified by sampling ENV3.
                        self.hold_zero = true;
                    }
                    _ => {}
                }
            }
        }
    }

    #[inline]
    pub fn clock_delta(&mut self, mut delta: u32) {
        // NB! This requires two's complement integer.
        let mut rate_step = self.rate_counter_period as i32 - self.rate_counter as i32;
        if rate_step <= 0 {
            rate_step += 0x7fff;
        }
        while delta != 0 {
            if delta < rate_step as u32 {
                self.rate_counter += delta as u16;
                if self.rate_counter & RATE_COUNTER_MSB_MASK != 0 {
                    self.rate_counter += 1;
                    self.rate_counter &= RATE_COUNTER_MASK;
                }
                return;
            }
            self.rate_counter = 0;
            delta -= rate_step as u32;
            // The first envelope step in the attack state also resets the exponential
            // counter. This has been verified by sampling ENV3.
            self.exponential_counter += 1; // TODO check w/ ref impl
            if self.state == State::Attack
                || self.exponential_counter == self.exponential_counter_period
            {
                self.exponential_counter = 0;
                // Check whether the envelope counter is frozen at zero.
                if self.hold_zero {
                    rate_step = self.rate_counter_period as i32;
                    continue;
                }
                match self.state {
                    State::Attack => {
                        // The envelope counter can flip from 0xff to 0x00 by changing state to
                        // release, then to attack. The envelope counter is then frozen at
                        // zero; to unlock this situation the state must be changed to release,
                        // then to attack. This has been verified by sampling ENV3.
                        self.envelope_counter += 1;
                        if self.envelope_counter == 0xff {
                            self.state = State::DecaySustain;
                            self.rate_counter_period = period(self.decay);
                        }
                    }
                    State::DecaySustain => {
                        if self.envelope_counter != SUSTAIN_LEVEL[self.sustain as usize] {
                            self.envelope_counter -= 1;
                        }
                    }
                    State::Release => {
                        // The envelope counter can flip from 0x00 to 0xff by changing state to
                        // attack, then to release. The envelope counter will then continue
                        // counting down in the release state.
                        // This has been verified by sampling ENV3.
                        // NB! The operation below requires two's complement integer.
                        self.envelope_counter -= 1;
                    }
                }
                // Check for change of exponential counter period.
                match self.envelope_counter {
                    0xff => self.exponential_counter_period = 1,
                    0x5d => self.exponential_counter_period = 2,
                    0x36 => self.exponential_counter_period = 4,
                    0x1a => self.exponential_counter_period = 8,
                    0x0e => self.exponential_counter_period = 16,
                    0x06 => self.exponential_counter_period = 30,
                    0x00 => {
                        self.exponential_counter_period = 1;
                        // When the envelope counter is changed to zero, it is frozen at zero.
                        // This has been verified by sampling ENV3.
                        self.hold_zero = true;
                    }
                    _ => {}
                }
            }
            rate_step = self.rate_counter_period as i32;
        }
    }

    #[inline]
    pub fn output(&self) -> u8 {
        #[cfg(feature = "vice-analog")]
        {
            return match self.chip_model {
                ChipModel::Mos6581 => VICE_ENV_DAC6581[self.envelope_counter as usize] as u8,
                ChipModel::Mos8580 => VICE_ENV_DAC8580[self.envelope_counter as usize] as u8,
            };
        }
        #[cfg(not(feature = "vice-analog"))]
        self.envelope_counter
    }

    pub fn read_env(&self) -> u8 {
        if cfg!(feature = "vice-analog") {
            return self.env3;
        }
        self.output()
    }

    pub fn reset(&mut self) {
        self.attack = 0;
        self.decay = 0;
        self.sustain = 0;
        self.release = 0;
        self.gate = false;
        self.state = State::Release;
        if !cfg!(feature = "vice-analog") {
            self.envelope_counter = 0;
        }
        self.exponential_counter = 0;
        self.exponential_counter_period = 1;
        self.hold_zero = !cfg!(feature = "vice-analog");
        self.rate_counter = 0;
        self.rate_counter_period = period(self.release);
        self.envelope_pipeline = 0;
        self.exponential_pipeline = 0;
        self.state_pipeline = 0;
        self.reset_rate_counter = false;
    }

    fn set_exponential_counter_vice(&mut self) {
        match self.envelope_counter {
            0xff => self.exponential_counter_period = 1,
            0x5d => self.exponential_counter_period = 2,
            0x36 => self.exponential_counter_period = 4,
            0x1a => self.exponential_counter_period = 8,
            0x0e => self.exponential_counter_period = 16,
            0x06 => self.exponential_counter_period = 30,
            0x00 => {
                self.exponential_counter_period = 1;
                self.hold_zero = true;
            }
            _ => {}
        }
    }

    /// Single-cycle VICE 3.10 envelope state and rate/comparator pipelines.
    fn clock_vice(&mut self) {
        self.env3 = self.envelope_counter;
        if self.state_pipeline != 0 {
            self.state_pipeline -= 1;
            match self.next_state {
                State::Attack if self.state_pipeline == 0 => {
                    self.state = State::Attack;
                    self.rate_counter_period = period(self.attack);
                    self.hold_zero = false;
                }
                State::Release
                    if (self.state == State::Attack && self.state_pipeline == 0)
                        || (self.state == State::DecaySustain && self.state_pipeline == 1) =>
                {
                    self.state = State::Release;
                    self.rate_counter_period = period(self.release);
                }
                _ => {}
            }
        }
        if self.envelope_pipeline != 0 {
            self.envelope_pipeline -= 1;
            if self.envelope_pipeline == 0 && !self.hold_zero {
                match self.state {
                    State::Attack => {
                        self.envelope_counter = self.envelope_counter.wrapping_add(1);
                        if self.envelope_counter == 0xff {
                            self.state = State::DecaySustain;
                            self.rate_counter_period = period(self.decay);
                        }
                    }
                    State::DecaySustain | State::Release => {
                        self.envelope_counter = self.envelope_counter.wrapping_sub(1)
                    }
                }
                self.set_exponential_counter_vice();
            }
        }
        let exponential_active = self.exponential_pipeline != 0;
        if exponential_active {
            self.exponential_pipeline -= 1;
        }
        if exponential_active && self.exponential_pipeline == 0 {
            self.exponential_counter = 0;
            if (self.state == State::DecaySustain
                && self.envelope_counter != SUSTAIN_LEVEL[self.sustain as usize])
                || self.state == State::Release
            {
                self.envelope_pipeline = 1;
            }
        } else if self.reset_rate_counter {
            self.rate_counter = 0;
            self.reset_rate_counter = false;
            if self.state == State::Attack {
                self.exponential_counter = 0;
                self.envelope_pipeline = 2;
            } else if !self.hold_zero {
                self.exponential_counter = self.exponential_counter.wrapping_add(1);
                if self.exponential_counter == self.exponential_counter_period {
                    self.exponential_pipeline = if self.exponential_counter_period != 1 {
                        2
                    } else {
                        1
                    };
                }
            }
        }
        if self.rate_counter != self.rate_counter_period {
            self.rate_counter = self.rate_counter.wrapping_add(1);
            if self.rate_counter & 0x8000 != 0 {
                self.rate_counter = self.rate_counter.wrapping_add(1) & 0x7fff;
            }
        } else {
            self.reset_rate_counter = true;
        }
    }
}

// VICE 3.10 reSID 1.0-pre2 DAC tables, derived from dac.cc (Dag Lem, 2010,
// GPL-2.0-or-later). Baked f64 R-2R ladder values preserve native/WASM identity.
#[cfg(feature = "vice-analog")]
#[rustfmt::skip]
const VICE_ENV_DAC6581: [u16; 256] = [
    2, 4, 5, 6, 7, 8, 9, 11, 11, 12, 13, 15, 15, 17, 18, 20,
    19, 21, 21, 23, 23, 25, 26, 28, 28, 29, 30, 32, 32, 34, 35, 37,
    35, 36, 37, 39, 39, 41, 42, 44, 43, 45, 46, 48, 48, 50, 51, 52,
    51, 53, 54, 56, 56, 58, 59, 60, 60, 62, 63, 65, 65, 67, 67, 69,
    65, 67, 68, 70, 70, 72, 72, 74, 74, 76, 77, 78, 79, 80, 81, 83,
    82, 84, 85, 86, 87, 88, 89, 91, 91, 93, 93, 95, 95, 97, 98, 100,
    98, 100, 100, 102, 102, 104, 105, 107, 107, 108, 109, 111, 111, 113, 114, 116,
    115, 116, 117, 119, 119, 121, 122, 124, 123, 125, 126, 128, 128, 130, 131, 132,
    124, 126, 127, 129, 129, 131, 132, 134, 133, 135, 136, 138, 138, 140, 140, 142,
    141, 143, 144, 146, 146, 148, 149, 150, 150, 152, 153, 155, 155, 157, 157, 159,
    157, 159, 160, 162, 162, 164, 164, 166, 166, 168, 168, 170, 170, 172, 173, 175,
    174, 176, 177, 178, 179, 180, 181, 183, 183, 185, 185, 187, 187, 189, 190, 192,
    188, 189, 190, 192, 192, 194, 195, 197, 196, 198, 199, 201, 201, 203, 204, 206,
    205, 206, 207, 209, 209, 211, 212, 214, 213, 215, 216, 218, 218, 220, 221, 222,
    220, 222, 223, 225, 225, 227, 228, 229, 229, 231, 232, 234, 234, 236, 236, 238,
    237, 239, 240, 242, 242, 244, 244, 246, 246, 248, 249, 250, 251, 252, 253, 255,
];
#[cfg(feature = "vice-analog")]
#[rustfmt::skip]
const VICE_ENV_DAC8580: [u16; 256] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32,
    33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48,
    49, 50, 51, 52, 53, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
    64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
    80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95,
    96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
    112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127,
    128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143,
    144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159,
    160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175,
    176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 187, 188, 189, 190,
    191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206,
    207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222,
    223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238,
    239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254,
];
