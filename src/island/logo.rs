// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Notification icons, including "agent thinking" animations.
//!
//! Corporate logos are not drawn due to trademark restrictions.
//! Claude Code is identified by its terminal indicator glyphs (Unicode), cycling at 8 Hz.
//! Other agents use the Polynya mark: three stripes with a gradient. "Thinking" animates
//! via 60° snap rotations. The mark is 60° symmetric, making the loop seamless.
//! Time is sourced from process start, not island clocks, to ensure glyph changes
//! occur even during one-off redraws.

use std::{sync::OnceLock, time::Instant};

use windows::{
    Win32::Graphics::Direct2D::{Common::D2D1_GRADIENT_STOP, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES},
    core::Result,
};
use windows_numerics::Vector2;

use super::{
    kit::{Font, Frame, Style, draw_text, ease_in_out, gradient, rgb, set_opacity},
    zone::Note,
};
use crate::icons::{self, Icon};

const GLYPHS: [&str; 6] = ["·", "✢", "✳", "✶", "✻", "✽"];
const GLYPH_REST: &str = "✻";
const GLYPH_RATE: f32 = 8.0;
const MARK_FRONT: &str = "M16.896 30.208V44.288H26.24V24.87L43.008 15.104L38.4 7.04L26.24 14.08V0H16.896V19.42L0 29.184L4.608 37.248L16.896 30.208Z";
const MARK_BACK: [&str; 2] = [
    "M43.0078 29.1836V39.9697L26.2402 30.208V24.8701L30.8838 22.1641L43.0078 29.1836Z",
    "M16.8965 14.0801V19.4199L0 29.1836L12.1602 22.1436L0 15.1035V4.30957L16.8965 14.0801Z",
];
const MARK_C: (f32, f32) = (21.568, 22.144);
const MARK_H: f32 = 45.0;
const MARK_SHADE: f32 = 0.1;
const MARK_STEP: f32 = 1.1;
const MARK_MOVE: f32 = 0.45;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anim {
    Glyph,
    Dots,
}

pub fn anim(note: &Note) -> Option<Anim> {
    note.animated.then_some(if note.icon == Icon::Claude { Anim::Glyph } else { Anim::Dots })
}

fn now() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

pub fn next_glyph() -> f32 {
    let t = now() * GLYPH_RATE;
    (t.floor() + 1.0 - t) / GLYPH_RATE
}

pub fn note_icon(f: &mut Frame, note: &Note, cx: f32, cy: f32, size: f32, alpha: f32) -> Result<()> {
    if let Some(bmp) = note.app.as_deref().and_then(|app| app_icon(f, app)) {
        let dst = windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
            left: cx - size / 2.0,
            top: cy - size / 2.0,
            right: cx + size / 2.0,
            bottom: cy + size / 2.0,
        };
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe { f.dc.DrawBitmap(&bmp, Some(&dst), alpha, windows::Win32::Graphics::Direct2D::D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None) };
        return Ok(());
    }
    match (anim(note), note.icon) {
        (Some(Anim::Glyph), _) => {
            let n = GLYPHS.len();
            let period = 2 * (n - 1);
            let step = (now() * GLYPH_RATE) as usize % period;
            glyph(f, GLYPHS[if step < n { step } else { period - step }], cx, cy, size, alpha)
        }
        (Some(Anim::Dots), _) => mark(f, now(), cx, cy, size, alpha),
        (None, Icon::Claude) => glyph(f, GLYPH_REST, cx, cy, size, alpha),
        (None, Icon::Agent) => mark(f, 0.0, cx, cy, size, alpha),
        (None, Icon::Color([r, g, b])) => {
            let mut c = super::kit::rgb(r, g, b);
            c.a = alpha;
            // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow
            // that outlives the call.
            unsafe {
                let brush = f.dc.CreateSolidColorBrush(&c, None)?;
                f.dc.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: windows_numerics::Vector2 { X: cx, Y: cy },
                        radiusX: size * 0.45,
                        radiusY: size * 0.45,
                    },
                    &brush,
                );
            }
            Ok(())
        }
        (None, icon) => {
            let br = f.br;
            let brush = match icon {
                Icon::Ok => &br.green,
                Icon::Fail => &br.red,
                _ => &br.fg,
            };
            set_opacity(brush, alpha);
            icons::draw(f.gfx, f.dc, icon, cx, cy, size, brush)?;
            set_opacity(brush, 1.0);
            Ok(())
        }
    }
}

const APP_ICON_PX: f32 = 32.0;

fn app_icon(f: &mut Frame, app: &str) -> Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1> {
    use crate::launcher::{apps, icons as shell_icons};
    if let Some(b) = f.app_icons.get(app) {
        return b.clone();
    }
    let key = apps::shell_name(app);
    match shell_icons::take(&key) {
        Some(hbm) => {
            let bmp = hbm.and_then(|h| {
                let b = f.gfx.bitmap_from_hbitmap(h).ok();
                // SAFETY: `h` is an HBITMAP handed over by `icons::take`, deleted exactly once here.
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::DeleteObject(h.into());
                }
                b
            });
            f.app_icons.insert(app.to_string(), bmp.clone());
            bmp
        }
        None => {
            shell_icons::request(&key, (APP_ICON_PX * f.gfx.dpi / 96.0).round() as i32, crate::media_changed);
            None
        }
    }
}

fn glyph(f: &mut Frame, g: &str, cx: f32, cy: f32, size: f32, alpha: f32) -> Result<()> {
    let t = f.txt(g, if size > 15.0 { Font::SymbolBig } else { Font::Symbol }, Style::Plain)?;
    let fg = f.br.fg.clone();
    set_opacity(&fg, alpha);
    draw_text(f.dc, &t, cx - t.w / 2.0, cy - t.h / 2.0, &fg);
    set_opacity(&fg, 1.0);
    Ok(())
}

fn mark(f: &mut Frame, t: f32, cx: f32, cy: f32, size: f32, alpha: f32) -> Result<()> {
    let k = t / MARK_STEP;
    let a = (k.floor() + ease_in_out((k.fract() / MARK_MOVE).min(1.0))) * std::f32::consts::FRAC_PI_3;
    let (sin, cos) = a.sin_cos();
    let s = size / MARK_H;
    let p = |x: f32, y: f32| {
        let (dx, dy) = (x - MARK_C.0, y - MARK_C.1);
        Vector2 { X: cx + (dx * cos - dy * sin) * s, Y: cy + (dx * sin + dy * cos) * s }
    };
    let stop = |position: f32, [r, g, b]: [u8; 3]| D2D1_GRADIENT_STOP {
        position,
        color: {
            let mut c = rgb(r, g, b);
            c.a = alpha;
            c
        },
    };
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    let brush = |stops: &[D2D1_GRADIENT_STOP], from: (f32, f32), to: (f32, f32)| unsafe {
        let coll = gradient(f.dc, stops)?;
        f.dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: p(from.0, from.1), endPoint: p(to.0, to.1) }, None, &coll)
    };
    const ORANGE: [u8; 3] = [0xff, 0x5a, 0x1f];
    const RED: [u8; 3] = [0xe4, 0x1f, 0x18];
    const NAVY: [u8; 3] = [0x25, 0x33, 0x49];
    let back = brush(&[stop(0.0, ORANGE), stop(0.2, RED), stop(0.65, NAVY)], (42.977, -0.293), (-0.023, 44.707))?;
    let front = brush(&[stop(0.053, ORANGE), stop(0.255, RED), stop(0.75, NAVY)], (42.977, -5.716), (-9.708, 38.678))?;
    // Shadow on the mark's rays is black in any theme.
    // SAFETY: `f.dc` is a valid device context.
    let shade = unsafe { f.dc.CreateSolidColorBrush(&super::kit::black(1.0), None)? };
    set_opacity(&shade, alpha * MARK_SHADE);
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        for d in MARK_BACK {
            let geo = icons::fill_geometry(f.gfx, d, &p, s)?;
            f.dc.FillGeometry(&geo, &back, None);
            f.dc.FillGeometry(&geo, &shade, None);
        }
        f.dc.FillGeometry(&icons::fill_geometry(f.gfx, MARK_FRONT, &p, s)?, &front, None);
    }
    set_opacity(&shade, 1.0);
    Ok(())
}
