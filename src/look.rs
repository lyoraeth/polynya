// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! System look: how the taskbar actually looks, plus the Windows theme and accent color.
//!
//! DWM mixes in the native taskbar backdrop (blurred, tinted wallpaper) during composition; it never
//! reaches the taskbar window's own frame, only the translucent tint does. So the taskbar strip is
//! captured from the screen, with the island excluded from capture (`WDA_EXCLUDEFROMCAPTURE`) for
//! that moment so the taskbar shows in its place. Capture runs only on events that change the
//! taskbar's look; the main window collects them.

use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Dwm::DwmFlush,
            Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC,
                SRCCOPY, SelectObject,
            },
        },
        UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE},
    },
};

/// Color in unit fractions, sRGB.
pub type Rgb = [f32; 3];

/// Pixels to skip from the edge: the native taskbar has an outline line along it.
const EDGE_INSET: i32 = 2;
/// Brightness difference from the edge row that makes the second outer row part of the outline.
const BORDER_ROW_DIFF: f32 = 0.02;
/// A brightness jump between neighbors above this is an icon edge, not grain.
const GRAIN_MAX: f32 = 0.05;
/// Smoothing radius of the edge row, pixels.
const EDGE_SMOOTH: usize = 8;
/// Half-width of the median along the edge row, pixels: taskbar icons and their highlights (the
/// Start button is about 40 px) are narrower than the window and get dropped, while the taskbar
/// background changes smoothly across the width.
const EDGE_MEDIAN: usize = 48;

#[derive(Debug, Clone)]
pub struct Look {
    /// Whether the Windows theme is light; it decides text and icon color.
    pub light: bool,
    pub accent: Rgb,
    /// Taskbar color at the edge, per screen column: dropdowns grow out of it.
    pub edge: Vec<Rgb>,
    /// Average taskbar color per screen column: the background under plates.
    pub mean: Vec<Rgb>,
    /// Taskbar outline along the edge: the first pixel row on the screen side.
    pub border: Vec<Rgb>,
    /// Outline thickness in pixel rows, 1 or 2. Usually 1, but after a display with another
    /// scale was connected Explorer can keep a thicker one until it restarts.
    pub border_rows: u32,
    /// Taskbar grain: average brightness difference of neighboring pixels, in unit fractions. Acrylic
    /// noise on the native taskbar, zero on a solid-filled one.
    pub noise: f32,
}

impl Look {
    /// Captures the system look. `taskbar` is the taskbar rectangle in screen pixels, `bottom` means
    /// the taskbar is at the bottom edge (its edge is on top).
    pub fn capture(island: HWND, taskbar: RECT, bottom: bool) -> Option<Self> {
        let Strip { edge, mean, border, border_rows, noise } = strip(island, taskbar, bottom)?;
        Some(Self { light: crate::taskbar::light(), accent: accent().unwrap_or([0.0, 0.47, 0.84]), edge, mean, border, border_rows, noise })
    }

    /// Average taskbar color over columns `[l, r)` from the taskbar's left edge.
    pub fn mean_over(&self, l: usize, r: usize) -> Rgb {
        average(&self.mean[l.min(self.mean.len())..r.min(self.mean.len())])
    }
}

/// Sliding median of radius `r` pixels, per channel.
fn median(cols: &[Rgb], r: usize) -> Vec<Rgb> {
    let mut win = Vec::with_capacity(r * 2 + 1);
    (0..cols.len())
        .map(|i| {
            let part = &cols[i.saturating_sub(r)..(i + r + 1).min(cols.len())];
            let mut c = [0.0; 3];
            for (k, v) in c.iter_mut().enumerate() {
                win.clear();
                win.extend(part.iter().map(|p| p[k]));
                let mid = win.len() / 2;
                *v = *win.select_nth_unstable_by(mid, f32::total_cmp).1;
            }
            c
        })
        .collect()
}

/// Sliding mean of radius `r` pixels.
fn smooth(cols: &[Rgb], r: usize) -> Vec<Rgb> {
    (0..cols.len()).map(|i| average(&cols[i.saturating_sub(r)..(i + r + 1).min(cols.len())])).collect()
}

fn average(cols: &[Rgb]) -> Rgb {
    if cols.is_empty() {
        return [0.0; 3];
    }
    let n = cols.len() as f32;
    let s = cols.iter().fold([0.0; 3], |a, c| [a[0] + c[0], a[1] + c[1], a[2] + c[2]]);
    [s[0] / n, s[1] / n, s[2] / n]
}

/// Windows accent color.
fn accent() -> Option<Rgb> {
    let c = UISettings::new().ok()?.GetColorValue(UIColorType::Accent).ok()?;
    Some([c.R as f32 / 255.0, c.G as f32 / 255.0, c.B as f32 / 255.0])
}

/// What [`strip`] reads off the taskbar; the fields mean the same as in [`Look`].
struct Strip {
    edge: Vec<Rgb>,
    mean: Vec<Rgb>,
    border: Vec<Rgb>,
    border_rows: u32,
    noise: f32,
}

/// Taskbar strip from the screen without the island: the edge row, column averages and the outline.
fn strip(island: HWND, r: RECT, bottom: bool) -> Option<Strip> {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= EDGE_INSET * 2 {
        return None;
    }
    // SAFETY: the screen DC, memory DC and DIB are created and released here in pairs; `bits` points
    // to the DIB's `w * h * 4` bytes, read before the DIB is deleted; `island` is our own window.
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // Negative height: rows go top to bottom.
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let pixels: Option<Vec<u8>> = match CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) => {
                let old = SelectObject(mem, bmp.into());
                // The island leaves capture; DWM applies this with the next composition frame.
                let excluded = SetWindowDisplayAffinity(island, WDA_EXCLUDEFROMCAPTURE).is_ok();
                let _ = DwmFlush();
                let ok = BitBlt(mem, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY).is_ok();
                if excluded {
                    let _ = SetWindowDisplayAffinity(island, WDA_NONE);
                }
                let out = (ok && excluded).then(|| std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec());
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                out
            }
            Err(_) => None,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        let px = pixels?;
        let at = |x: i32, y: i32| {
            let i = ((y * w + x) * 4) as usize;
            [px[i + 2] as f32 / 255.0, px[i + 1] as f32 / 255.0, px[i] as f32 / 255.0]
        };
        let edge_y = if bottom { EDGE_INSET } else { h - 1 - EDGE_INSET };
        let lum = |c: Rgb| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        // The edge row is stretched downward, so its grain would become vertical streaks: smooth it
        // horizontally, the dropdown adds its own noise. Icons can sit under the dropdown, and their pixels
        // in the edge row would stretch down as stripes.
        let edge = smooth(&median(&(0..w).map(|x| at(x, edge_y)).collect::<Vec<_>>(), EDGE_MEDIAN), EDGE_SMOOTH);
        // Exclude the outline at the edge and at the far side from the average.
        let mean = (0..w).map(|x| average(&(EDGE_INSET..h - EDGE_INSET).map(|y| at(x, y)).collect::<Vec<_>>())).collect();
        let border = (0..w).map(|x| at(x, if bottom { 0 } else { h - 1 })).collect();
        // The outline is as many outer rows as differ from the edge row, averaged over the width.
        let row_lum = |y: i32| (0..w).map(|x| lum(at(x, y))).sum::<f32>() / w as f32;
        let edge_lum = row_lum(edge_y);
        let second = if bottom { 1 } else { h - 2 };
        let border_rows = if (row_lum(second) - edge_lum).abs() > BORDER_ROW_DIFF { 2 } else { 1 };
        // Large jumps are taskbar icon edges, not grain. A median does not work: the noise is sparse,
        // and more than half of the neighbors are equal.
        let (mut sum, mut n) = (0.0f32, 0u32);
        for y in EDGE_INSET..h - EDGE_INSET {
            for x in 0..w - 1 {
                let d = (lum(at(x, y)) - lum(at(x + 1, y))).abs();
                if d < GRAIN_MAX {
                    sum += d;
                    n += 1;
                }
            }
        }
        let noise = if n == 0 { 0.0 } else { sum / n as f32 };
        Some(Strip { edge, mean, border, border_rows, noise })
    }
}

/// `#rrggbb` for the log.
pub fn hex(c: Rgb) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}
