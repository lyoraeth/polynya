// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! System-derived plate colors: a semi-transparent Windows accent tint over the actual
//! taskbar color. Opacity and lightness are tuned to maintain APCA contrast (WCAG 3)
//! for text on any taskbar and accent combination.

use crate::look::Rgb;

/// APCA thresholds (Lc): primary text and dimmed text (labels, large time).
const LC_TEXT: f32 = 60.0;
const LC_DIM: f32 = 45.0;
/// Plate opacity corridor: denser obscures the panel, more transparent obscures the plate.
const ALPHA_MAX: f32 = 0.45;
const ALPHA_MIN: f32 = 0.20;
/// Accent chroma limit: the plate is a tint, not a solid color fill.
const CHROMA_MAX: f32 = 0.06;
/// OKLab lightness range for light panels: where to start and how far to go for contrast.
/// The plate is slightly darker than the panel (a depression, not a highlight); if contrast
/// is short, the search moves toward lighter.
const L_LIGHT: (f32, f32) = (0.80, 1.0);
/// OKLab lightness range for dark panels: lighter than the panel; if contrast is short,
/// the search moves toward darker.
const L_DARK: (f32, f32) = (0.40, 0.22);

/// Plate configuration: tint color and opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    pub color: Rgb,
    pub alpha: f32,
}

/// Selects a plate color and opacity for text `fg` and dimmed `dim` on background `bg`.
/// Returns the densest (most visible) option closest to the target lightness.
pub fn plate(accent: Rgb, bg: Rgb, fg: Rgb, dim: Rgb, light: bool) -> Plate {
    let (_, c, h) = oklch(accent);
    let (from, to) = if light { L_LIGHT } else { L_DARK };
    // The plate is not required to be higher contrast than the bare panel: if the panel itself
    // misses the threshold, require no worse than the panel.
    let lc_text = LC_TEXT.min(apca(fg, bg) - 5.0);
    let lc_dim = LC_DIM.min(apca(dim, bg) - 5.0);
    const STEPS: usize = 8;
    for i in 0..=STEPS {
        let l = from + (to - from) * i as f32 / STEPS as f32;
        let color = from_oklch(l, c.min(CHROMA_MAX), h);
        let mut alpha = ALPHA_MAX;
        while alpha >= ALPHA_MIN - 1e-4 {
            let under = mix(bg, color, alpha);
            if apca(fg, under) >= lc_text && apca(dim, under) >= lc_dim {
                return Plate { color, alpha };
            }
            alpha -= 0.05;
        }
    }
    // Fallback: contrast cannot be achieved even at the edge (e.g., black panel in light theme).
    // Return the most contrasting plate possible.
    Plate { color: from_oklch(to, c.min(CHROMA_MAX), h), alpha: if light { ALPHA_MAX } else { ALPHA_MIN } }
}

/// Vivid panel tone for enabled toggles: same hue as the panel, but lighter and more
/// saturated than plates, ensuring "on" state is immediately readable.
pub fn vivid(bg: Rgb, light: bool) -> Rgb {
    let (_, c, h) = oklch(bg);
    from_oklch(if light { 0.55 } else { 0.64 }, c.clamp(0.09, 0.14), h)
}

/// OKLab lightness, 0.0 to 1.0.
pub fn lightness(c: Rgb) -> f32 {
    oklch(c).0
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// APCA 0.0.98G contrast (Lc module, 0 to ~106): `text` on `bg`.
pub fn apca(text: Rgb, bg: Rgb) -> f32 {
    let y = |c: Rgb| {
        let y = 0.2126729 * c[0].max(0.0).powf(2.4) + 0.7151522 * c[1].max(0.0).powf(2.4) + 0.0721750 * c[2].max(0.0).powf(2.4);
        // Soft threshold near black.
        if y < 0.022 { y + (0.022 - y).powf(1.414) } else { y }
    };
    let (yt, yb) = (y(text), y(bg));
    if (yb - yt).abs() < 0.0005 {
        return 0.0;
    }
    let lc = if yb > yt {
        // Dark text on light background.
        let s = (yb.powf(0.56) - yt.powf(0.57)) * 1.14;
        if s < 0.1 { 0.0 } else { s - 0.027 }
    } else {
        // Light text on dark background.
        let s = (yb.powf(0.65) - yt.powf(0.62)) * 1.14;
        if s > -0.1 { 0.0 } else { s + 0.027 }
    };
    (lc * 100.0).abs()
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB to OKLCH: lightness, chroma, hue (radians).
fn oklch(c: Rgb) -> (f32, f32, f32) {
    let [r, g, b] = c.map(to_linear);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let (ll, a, bb) = (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    );
    (ll, a.hypot(bb), bb.atan2(a))
}

fn from_oklch(l: f32, c: f32, h: f32) -> Rgb {
    let (a, b) = (c * h.cos(), c * h.sin());
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        to_srgb(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        to_srgb(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
        to_srgb(-0.0041960863 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apca_reference() {
        assert!((apca([0.533; 3], [1.0; 3]) - 63.06).abs() < 0.5, "{}", apca([0.533; 3], [1.0; 3]));
        assert!((apca([1.0; 3], [0.0; 3]) - 107.88).abs() < 0.5);
    }

    #[test]
    fn oklch_roundtrip() {
        let c = [0.2, 0.5, 0.8];
        let (l, ch, h) = oklch(c);
        let back = from_oklch(l, ch, h);
        assert!(c.iter().zip(back).all(|(a, b)| (a - b).abs() < 1e-3));
    }

    #[test]
    fn plate_holds_contrast() {
        let (fg_l, dim_l) = ([0.106, 0.106, 0.122], [0.33, 0.33, 0.35]);
        let (fg_d, dim_d) = ([0.925; 3], [0.55; 3]);
        for (bg, light, fg, dim) in [([0.9, 0.878, 0.82], true, fg_l, dim_l), ([0.13, 0.13, 0.14], false, fg_d, dim_d)] {
            let p = plate([0.0, 0.47, 0.84], bg, fg, dim, light);
            let under = mix(bg, p.color, p.alpha);
            assert!(apca(fg, under) >= LC_TEXT, "{p:?}");
            assert!((ALPHA_MIN..=ALPHA_MAX).contains(&p.alpha));
        }
    }
}
