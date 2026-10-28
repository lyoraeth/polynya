// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates an 11-step lightness scale for a given color, similar to Tailwind palettes (50–950).
//! Uses OKLCH for perceptually uniform steps. The hue is preserved, lightness follows the scale,
//! and the original color snaps to the nearest step unchanged. Out-of-gamut saturation is clamped.

/// Target lightness values for steps 50 through 950.
const STEPS: [f32; 11] = [0.975, 0.94, 0.885, 0.81, 0.71, 0.63, 0.555, 0.48, 0.41, 0.355, 0.27];

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f32) -> u8 {
    let c = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Converts linear sRGB to OKLab.
fn oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// Converts OKLab to linear sRGB.
fn linear_from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
}

/// Finds the maximum chroma that fits within the sRGB gamut for a given lightness and hue, capped at `c`.
fn fit(l: f32, c: f32, h: f32) -> [u8; 3] {
    let at = |c: f32| linear_from_oklab([l, c * h.cos(), c * h.sin()]);
    let inside = |v: [f32; 3]| v.iter().all(|x| (-1e-4..=1.0 + 1e-4).contains(x));
    let (mut lo, mut hi) = (0.0, c);
    if !inside(at(hi)) {
        // Binary search for the maximum valid chroma within sRGB gamut.
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if inside(at(mid)) { lo = mid } else { hi = mid }
        }
        hi = lo;
    }
    at(hi).map(from_linear)
}

/// Generates an 11-step color scale and returns the index of the step matching the original color.
pub fn shades(rgb: [u8; 3]) -> (Vec<[u8; 3]>, usize) {
    let [l, a, b] = oklab(rgb.map(to_linear));
    let (c, h) = ((a * a + b * b).sqrt(), b.atan2(a));
    let mark = (0..STEPS.len()).min_by(|&i, &j| (STEPS[i] - l).abs().total_cmp(&(STEPS[j] - l).abs())).unwrap();
    // Softly reduce chroma away from the original lightness to prevent neon artifacts at the scale extremes.
    let out = STEPS.iter().enumerate().map(|(i, &li)| if i == mark { rgb } else { fit(li, c * (1.0 - 0.5 * (li - l).abs()), h) }).collect();
    (out, mark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_sits_on_its_step() {
        let (s, mark) = shades([0x3b, 0x82, 0xf6]);
        assert_eq!(s.len(), 11);
        assert_eq!(s[mark], [0x3b, 0x82, 0xf6]);
        let lum = |c: [u8; 3]| c.iter().map(|&x| x as u32).sum::<u32>();
        assert!(lum(s[0]) > lum(s[5]) && lum(s[5]) > lum(s[10]));
    }

    #[test]
    fn grays_stay_gray() {
        let (s, _) = shades([0x1e, 0x1e, 0x1e]);
        for [r, g, b] in s {
            assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1);
        }
    }

    #[test]
    fn roundtrip() {
        let c = [0x12, 0x9a, 0x4f];
        let lab = oklab(c.map(to_linear));
        assert_eq!(linear_from_oklab(lab).map(from_linear), c);
    }
}
