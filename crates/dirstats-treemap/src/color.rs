// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// OKLab / OKLCH conversions follow the reference formulas published by
// Björn Ottosson (https://bottosson.github.io/posts/oklab/, public domain /
// MIT). Gamut mapping here is a simple chroma reduction.

//! Perceptual colour: everything is shaded in OKLCH and converted to sRGB
//! only when a pixel is written.

/// sRGB, 8 bits per channel. Output only.
pub type Rgb = [u8; 3];

/// OKLCH: lightness 0..=1, chroma ≥ 0 (about 0.4 is the sRGB limit), hue in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

impl Oklch {
    #[must_use]
    pub const fn new(l: f64, c: f64, h: f64) -> Self {
        Self { l, c, h }
    }

    /// Neutral grey at this lightness.
    #[must_use]
    pub const fn grey(l: f64) -> Self {
        Self { l, c: 0.0, h: 0.0 }
    }

    #[must_use]
    pub fn with_lightness(self, l: f64) -> Self {
        Self { l: l.clamp(0.0, 1.0), ..self }
    }

    #[must_use]
    pub fn lighten(self, delta: f64) -> Self {
        self.with_lightness(self.l + delta)
    }

    #[must_use]
    pub fn scale_lightness(self, factor: f64) -> Self {
        self.with_lightness(self.l * factor)
    }

    #[must_use]
    pub fn scale_chroma(self, factor: f64) -> Self {
        Self { c: (self.c * factor).max(0.0), ..self }
    }

    #[must_use]
    pub fn from_srgb(rgb: Rgb) -> Self {
        let [r, g, b] = rgb.map(|c| srgb_to_linear(f64::from(c) / 255.0));
        let l_ = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
        let m_ = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
        let s_ = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
        let (l_, m_, s_) = (l_.cbrt(), m_.cbrt(), s_.cbrt());
        let l = 0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_;
        let a = 1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_;
        let b = 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_;
        let c = (a * a + b * b).sqrt();
        let h = b.atan2(a).to_degrees().rem_euclid(360.0);
        Self { l, c, h }
    }

    /// Convert to sRGB, reducing chroma until the colour fits the gamut.
    #[must_use]
    pub fn to_srgb(self) -> Rgb {
        let l = self.l.clamp(0.0, 1.0);
        if let Some(rgb) = (Self { l, ..self }).try_srgb() {
            return rgb;
        }
        // Binary search the largest in-gamut chroma at this lightness and hue.
        let (mut lo, mut hi) = (0.0, self.c);
        for _ in 0..12 {
            let mid = (lo + hi) / 2.0;
            if (Self { l, c: mid, h: self.h }).try_srgb().is_some() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Self { l, c: lo, h: self.h }.try_srgb().unwrap_or_else(|| Self::grey(l).try_srgb().unwrap_or([0; 3]))
    }

    /// sRGB if every channel is within 0..=1, else `None`.
    fn try_srgb(self) -> Option<Rgb> {
        let (a, b) = self.h.to_radians().sin_cos();
        let (a, b) = (self.c * b, self.c * a);
        let l_ = self.l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
        let m_ = self.l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
        let s_ = self.l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
        let (l_, m_, s_) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
        let r = 4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_;
        let g = -1.268_438_004_6 * l_ + 2.609_757_401_9 * m_ - 0.341_319_396_5 * s_;
        let b = -0.004_196_086_3 * l_ - 0.703_418_614_8 * m_ + 1.707_614_701_0 * s_;
        const EPS: f64 = 1e-4;
        let channels = [r, g, b];
        if channels.iter().any(|&c| !(-EPS..=1.0 + EPS).contains(&c)) {
            return None;
        }
        Some(channels.map(|c| (linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0).round() as u8))
    }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_srgb() {
        for rgb in [[0, 0, 0], [255, 255, 255], [255, 0, 0], [0, 128, 255], [37, 190, 12]] {
            let back = Oklch::from_srgb(rgb).to_srgb();
            for (a, b) in rgb.iter().zip(back) {
                assert!((i32::from(*a) - i32::from(b)).abs() <= 1, "{rgb:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn white_and_black_have_extreme_lightness() {
        assert!(Oklch::from_srgb([255, 255, 255]).l > 0.99);
        assert!(Oklch::from_srgb([0, 0, 0]).l < 0.01);
    }

    #[test]
    fn out_of_gamut_is_mapped_not_clipped() {
        let vivid = Oklch::new(0.9, 0.4, 30.0).to_srgb();
        assert!(vivid.iter().all(|&c| c > 0), "chroma reduction keeps all channels lit: {vivid:?}");
    }
}
