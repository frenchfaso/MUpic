// This file is part of resid-rs.
// Copyright (c) 2017-2019 Sebastian Jastrzebski <sebby2k@gmail.com>. All rights reserved.
// Portions (c) 2004 Dag Lem <resid@nimrod.no>
// Licensed under the GPLv3. See LICENSE file in the project root for full license text.

#![no_std]

#[cfg(all(feature = "alloc", not(feature = "std")))]
extern crate alloc;
#[cfg(all(feature = "alloc", feature = "std"))]
extern crate std as alloc;

mod data;
pub mod envelope;
#[cfg(not(feature = "vice-analog"))]
pub mod external_filter;
#[cfg(feature = "vice-analog")]
#[path = "external_filter_vice.rs"]
pub mod external_filter;
#[cfg(not(feature = "vice-analog"))]
pub mod filter;
#[cfg(feature = "vice-analog")]
#[path = "filter_vice.rs"]
pub mod filter;
pub mod sampler;
mod sid;
pub mod spline;
pub mod synth;
pub mod voice;
pub mod wave;

#[cfg(not(feature = "std"))]
mod math;

#[derive(Clone, Copy)]
pub enum ChipModel {
    Mos6581,
    Mos8580,
}

pub use self::sampler::SamplingMethod;
pub use self::sid::Sid;
pub use self::sid::State as SidState;
