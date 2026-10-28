// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! DirectComposition marquee: text is rendered once into a separate layer, and the
//! composition engine handles the scrolling. The island avoids per-frame rendering
//! for long track titles.
//!
//! Layer structure: a clipped lane with global opacity (for hover transitions),
//! containing a surface with text rendered twice with a gap. The composition
//! animation shifts this surface. Edge fading uses gradients matching the pill color.
//! The left edge fades in only when scrolling starts, ensuring readability at rest.

use windows::{
    Win32::Graphics::{
        Direct2D::{
            Common::{D2D_RECT_F, D2D1_COLOR_F, D2D1_GRADIENT_STOP},
            D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, ID2D1SolidColorBrush,
        },
        DirectComposition::{DCOMPOSITION_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, IDCompositionEffectGroup, IDCompositionSurface, IDCompositionVisual},
    },
    core::Result,
};
use windows_numerics::Vector2;

use super::kit::{MARQUEE_FADE, MARQUEE_GAP, MARQUEE_SPEED, Text, draw_text, gradient};
use crate::gfx::Gfx;

/// Vertical padding above and below the text in the layer, in DIPs.
const PAD_Y: f32 = 2.0;
/// Duration for the left edge fade-in when scrolling starts, in seconds.
const LEFT_FADE_IN: f32 = 0.25;

/// Request to show a scrolling text that doesn't fit in the lane `[l, r]` centered at `cy`.
pub struct Request {
    pub key: String,
    pub text: Text,
    pub l: f32,
    pub r: f32,
    pub cy: f32,
    pub alpha: f32,
    /// Delay before scrolling starts, in seconds.
    pub delay: f32,
    /// Background color under the text, used for edge fading.
    pub bg: [f32; 3],
}

pub struct Scroller {
    lane: IDCompositionVisual,
    text: IDCompositionVisual,
    fade_l: IDCompositionVisual,
    fade_r: IDCompositionVisual,
    opacity: IDCompositionEffectGroup,
    left_opacity: IDCompositionEffectGroup,
    /// Surfaces are retained as long as the layer displays them.
    surfaces: Vec<IDCompositionSurface>,
    /// Cached text and lane width to avoid redundant redraws.
    painted: Option<(String, u32, [u32; 3])>,
    /// Last applied offset and opacity to avoid redundant composition calls.
    placed: Option<(f32, f32, f32)>,
}

impl Scroller {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        // SAFETY: DirectComposition calls on live objects; every argument outlives the call.
        unsafe {
            let d = &gfx.dcomp;
            let (lane, text, fade_l, fade_r) = (d.CreateVisual()?, d.CreateVisual()?, d.CreateVisual()?, d.CreateVisual()?);
            // Integer pixel shifting prevents text blurring.
            text.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR)?;
            lane.AddVisual(&text, false, None::<&IDCompositionVisual>)?;
            lane.AddVisual(&fade_l, true, &text)?;
            lane.AddVisual(&fade_r, true, &fade_l)?;
            let opacity = d.CreateEffectGroup()?;
            opacity.SetOpacity2(0.0)?;
            lane.SetEffect(&opacity)?;
            let left_opacity = d.CreateEffectGroup()?;
            fade_l.SetEffect(&left_opacity)?;
            gfx.root.AddVisual(&lane, true, None::<&IDCompositionVisual>)?;
            Ok(Self { lane, text, fade_l, fade_r, opacity, left_opacity, surfaces: Vec::new(), painted: None, placed: None })
        }
    }

    /// Shows the scrolling text. Redraws the layer and restarts the animation only if
    /// the text or lane width has changed. Changes are committed to the composition
    /// engine on the next frame.
    pub fn show(&mut self, gfx: &Gfx, req: &Request, fg: &ID2D1SolidColorBrush) -> Result<()> {
        let k = gfx.dpi / 96.0;
        let lane_w = req.r - req.l;
        let h = req.text.h + PAD_Y * 2.0;
        // Background color is part of the key: wallpaper changes require edge recoloring.
        let key = (req.key.clone(), lane_w.to_bits(), req.bg.map(f32::to_bits));
        if self.painted.as_ref() != Some(&key) {
            self.repaint(gfx, req, fg, lane_w, h)?;
            self.painted = Some(key);
        }
        // Lane must be aligned to integer pixels to prevent text blurring.
        let place = ((req.l * k).round(), ((req.cy - h / 2.0) * k).round(), req.alpha);
        if self.placed != Some(place) {
            // SAFETY: DirectComposition calls on live objects; every argument outlives the call.
            unsafe {
                self.lane.SetOffsetX2(place.0)?;
                self.lane.SetOffsetY2(place.1)?;
                self.opacity.SetOpacity2(place.2)?;
            }
            self.placed = Some(place);
        }
        Ok(())
    }

    pub fn hide(&mut self) -> Result<()> {
        if self.placed.is_some_and(|p| p.2 > 0.0) {
            // SAFETY: DirectComposition calls on live objects; every argument outlives the call.
            unsafe { self.opacity.SetOpacity2(0.0)? };
            self.placed = None;
        }
        Ok(())
    }

    fn repaint(&mut self, gfx: &Gfx, req: &Request, fg: &ID2D1SolidColorBrush, lane_w: f32, h: f32) -> Result<()> {
        let k = gfx.dpi / 96.0;
        let cycle = req.text.w + MARQUEE_GAP;
        let (text_surface, text_px) = gfx.layer_surface(cycle + req.text.w, h)?;
        gfx.paint(&text_surface, text_px, |dc| {
            draw_text(dc, &req.text, 0.0, PAD_Y, fg);
            draw_text(dc, &req.text, cycle, PAD_Y, fg);
        })?;
        // Edge fades: gradient matching the pill color. Left fades out, right fades in.
        let fade = |from: f32, to: f32| -> Result<(IDCompositionSurface, (u32, u32))> {
            let (s, px) = gfx.layer_surface(MARQUEE_FADE, h)?;
            // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow
            // that outlives the call.
            gfx.paint(&s, px, |dc| unsafe {
                let c = |a: f32| D2D1_COLOR_F { r: req.bg[0], g: req.bg[1], b: req.bg[2], a };
                let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: c(from) }, D2D1_GRADIENT_STOP { position: 1.0, color: c(to) }];
                if let Ok(coll) = gradient(dc, &stops) {
                    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: 0.0, Y: 0.0 }, endPoint: Vector2 { X: MARQUEE_FADE, Y: 0.0 } };
                    if let Ok(brush) = dc.CreateLinearGradientBrush(&props, None, &coll) {
                        dc.FillRectangle(&D2D_RECT_F { left: 0.0, top: 0.0, right: MARQUEE_FADE, bottom: h }, &brush);
                    }
                }
            })?;
            Ok((s, px))
        };
        let (left, _) = fade(1.0, 0.0)?;
        let (right, _) = fade(0.0, 1.0)?;
        // SAFETY: DirectComposition calls on live objects; every argument outlives the call.
        unsafe {
            self.text.SetContent(&text_surface)?;
            self.fade_l.SetContent(&left)?;
            self.fade_r.SetContent(&right)?;
            self.fade_r.SetOffsetX2(((lane_w - MARQUEE_FADE) * k).round())?;
            self.lane.SetClip2(&D2D_RECT_F { left: 0.0, top: 0.0, right: (lane_w * k).round(), bottom: (h * k).round() })?;

            let d = &gfx.dcomp;
            // Animation: pause, then uniform shift for the repeat period, looping.
            let speed = MARQUEE_SPEED * k;
            let period = (cycle * k / speed) as f64;
            let delay = req.delay as f64;
            let shift = d.CreateAnimation()?;
            shift.AddCubic(0.0, 0.0, 0.0, 0.0, 0.0)?;
            shift.AddCubic(delay, 0.0, -speed, 0.0, 0.0)?;
            shift.AddRepeat(delay + period, period)?;
            self.text.SetOffsetX(&shift)?;
            let left = d.CreateAnimation()?;
            left.AddCubic(0.0, 0.0, 0.0, 0.0, 0.0)?;
            left.AddCubic(delay, 0.0, 1.0 / LEFT_FADE_IN, 0.0, 0.0)?;
            left.End(delay + LEFT_FADE_IN as f64, 1.0)?;
            self.left_opacity.SetOpacity(&left)?;
        }
        self.surfaces = vec![text_surface, left, right];
        Ok(())
    }
}
