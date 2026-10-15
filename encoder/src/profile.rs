//! Explicit geometry and raster cadence shared by native and browser encoders.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum VideoProfile {
    LegacyPalSd = 0,
    PalSd = 1,
    #[default]
    PalHd = 2,
    NtscSd = 3,
    NtscHd = 4,
}
impl TryFrom<u32> for VideoProfile {
    type Error = String;
    fn try_from(id: u32) -> Result<Self, Self::Error> {
        match id {
            0 => Ok(Self::LegacyPalSd),
            1 => Ok(Self::PalSd),
            2 => Ok(Self::PalHd),
            3 => Ok(Self::NtscSd),
            4 => Ok(Self::NtscHd),
            _ => Err("Unknown MUpic video profile".into()),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FitRectangle {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
}
impl VideoProfile {
    pub fn is_ntsc(self) -> bool {
        matches!(self, Self::NtscSd | Self::NtscHd)
    }
    pub fn frame_ticks(self) -> usize {
        if self.is_ntsc() {
            4
        } else {
            3
        }
    }
    pub fn phi2_hz(self) -> u32 {
        if self.is_ntsc() {
            1_022_730
        } else {
            985_248
        }
    }
    pub fn raster_lines(self) -> u32 {
        if self.is_ntsc() {
            263
        } else {
            312
        }
    }
    pub fn cycles_per_line(self) -> u32 {
        if self.is_ntsc() {
            65
        } else {
            63
        }
    }
    pub fn refresh_cycles(self) -> u32 {
        self.raster_lines() * self.cycles_per_line()
    }
    pub fn content_fps(self) -> f64 {
        f64::from(self.phi2_hz()) / (f64::from(self.refresh_cycles()) * self.frame_ticks() as f64)
    }
    pub fn pixel_aspect(self) -> (u32, u32) {
        match self {
            Self::LegacyPalSd | Self::PalSd => (1, 1),
            Self::PalHd => (15, 16),
            Self::NtscSd | Self::NtscHd => (5, 6),
        }
    }
    pub fn guard_pixels(self) -> usize {
        if self == Self::LegacyPalSd {
            2
        } else {
            0
        }
    }
    pub fn active_width(self) -> usize {
        crate::VIDEO_WIDTH - self.guard_pixels() * 2
    }
    pub fn max_frames(self) -> usize {
        65535 / self.frame_ticks()
    }
    pub fn rate_budget(self) -> usize {
        // Same two-line service reserve as the PAL player; NTSC's shorter
        // blank interval is qualified against its assembled playback path.
        if self.is_ntsc() {
            (263 - 202 - 2) * 65 * 63
        } else {
            crate::RATE_BUDGET
        }
    }
    pub fn frame_count(self, seconds: f64) -> Result<usize, String> {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err("Duration must be positive and finite".into());
        }
        let frames = (seconds * self.content_fps()).ceil() as usize;
        if !(2..=self.max_frames()).contains(&frames) {
            return Err(format!(
                "The selected profile supports 2..={} frames",
                self.max_frames()
            ));
        }
        Ok(frames)
    }
    /// Fit display aspect into the physical UPIC canvas, compensating HDMI
    /// pixel aspect before integer raster rounding. The entire source is kept.
    pub fn fit(
        self,
        width: u32,
        height: u32,
        sar_num: u32,
        sar_den: u32,
    ) -> Result<FitRectangle, String> {
        if width == 0 || height == 0 || sar_num == 0 || sar_den == 0 {
            return Err("Source dimensions and pixel aspect must be positive".into());
        }
        let (par_num, par_den) = self.pixel_aspect();
        let aspect = f64::from(width) * f64::from(sar_num) * f64::from(par_den)
            / (f64::from(height) * f64::from(sar_den) * f64::from(par_num));
        let max_w = self.active_width();
        let (w, h) = if aspect >= max_w as f64 / crate::VIDEO_HEIGHT as f64 {
            (
                max_w,
                (max_w as f64 / aspect)
                    .round()
                    .clamp(1.0, crate::VIDEO_HEIGHT as f64) as usize,
            )
        } else {
            (
                (crate::VIDEO_HEIGHT as f64 * aspect)
                    .round()
                    .clamp(1.0, max_w as f64) as usize,
                crate::VIDEO_HEIGHT,
            )
        };
        Ok(FitRectangle {
            left: (crate::VIDEO_WIDTH - w) / 2,
            top: (crate::VIDEO_HEIGHT - h) / 2,
            width: w,
            height: h,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fitted_geometry_preserves_display_aspect_to_one_raster_pixel() {
        for id in 1..=4 {
            let profile = VideoProfile::try_from(id).unwrap();
            let (pn, pd) = profile.pixel_aspect();
            for (w, h, sn, sd) in [
                (1920, 1080, 1, 1),
                (640, 480, 1, 1),
                (1080, 1920, 1, 1),
                (720, 576, 16, 15),
                (2560, 1080, 1, 1),
                (1, 200, 1, 1),
            ] {
                let fit = profile.fit(w, h, sn, sd).unwrap();
                assert!(fit.width == 360 || fit.height == 200);
                assert!(fit.left + fit.width <= 360 && fit.top + fit.height <= 200);
                let source = f64::from(w) * f64::from(sn) / f64::from(h) / f64::from(sd);
                let actual = fit.width as f64 * f64::from(pn) / fit.height as f64 / f64::from(pd);
                let pixel_error =
                    f64::from(pn) / f64::from(pd) / fit.height as f64 + source / fit.height as f64;
                assert!((actual - source).abs() <= pixel_error);
            }
        }
        assert_eq!(
            VideoProfile::PalSd.fit(1920, 1080, 1, 1).unwrap(),
            FitRectangle {
                left: 2,
                top: 0,
                width: 356,
                height: 200
            }
        );
        assert_eq!(
            VideoProfile::PalHd.fit(1920, 1080, 1, 1).unwrap(),
            FitRectangle {
                left: 0,
                top: 5,
                width: 360,
                height: 190
            }
        );
        assert_eq!(
            VideoProfile::NtscHd.fit(1920, 1080, 1, 1).unwrap(),
            FitRectangle {
                left: 0,
                top: 15,
                width: 360,
                height: 169
            }
        );
    }
}
