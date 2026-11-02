// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! DirectComposition marquee on flat backgrounds: text is rendered once and DWM shifts it.
//! On glass, a lane-sized transparent layer redraws the same alpha-masked text as the main renderer,
//! so no approximate copy of the panel background is needed.
//!
//! Layer structure: a clipped lane with global opacity (for hover transitions),
//! containing a surface with text rendered twice with a gap. On flat backgrounds, edge fading uses
//! gradients matching the pill color. The left edge fades in only when scrolling starts.

use windows::{
    Win32::Graphics::{
        Direct2D::{
            Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT},
            D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_OPTIONS1_NONE,
            D2D1_LAYER_PARAMETERS1, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, ID2D1Bitmap1, ID2D1LinearGradientBrush, ID2D1SolidColorBrush,
        },
        DirectComposition::{DCOMPOSITION_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, IDCompositionEffectGroup, IDCompositionSurface, IDCompositionVisual},
        DirectWrite::IDWriteRenderingParams,
        Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
    },
    core::{Interface, Result},
};
use windows_numerics::{Matrix3x2, Vector2};

use super::kit::{MARQUEE_FADE, MARQUEE_GAP, MARQUEE_SPEED, Text, draw_text, gradient};
use crate::gfx::Gfx;

/// Vertical padding above and below the text in the layer, in DIPs.
const PAD_Y: f32 = 2.0;
/// Duration for the left edge fade-in when scrolling starts, in seconds.
const LEFT_FADE_IN: f32 = 0.25;

fn color_key(brush: &ID2D1SolidColorBrush) -> [u32; 4] {
    // SAFETY: reading a live brush's color has no preconditions.
    let mut c = unsafe { brush.GetColor() };
    // Cached glyphs are drawn at full opacity; the current alpha is applied during composition.
    c.a = 0.0;
    [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]
}

/// Request to show a scrolling text that doesn't fit in the lane `[l, r]` centered at `cy`.
pub struct Request {
    pub key: String,
    pub text: Text,
    /// Text color and opacity source for this particular lane.
    pub brush: ID2D1SolidColorBrush,
    pub l: f32,
    pub r: f32,
    pub cy: f32,
    pub alpha: f32,
    /// Delay before scrolling starts, in seconds.
    pub delay: f32,
    /// Start time used by the transparent software-masked layer.
    pub since: f32,
    /// Background color under the text, used for edge fading.
    pub bg: [f32; 3],
    /// Draw with an alpha mask instead of opaque edge fades on glass.
    pub alpha_mask: bool,
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
    /// Cached text and lane width to avoid redundant redraws on a flat background.
    painted: Option<(String, u32, [u32; 3], [u32; 4])>,
    /// Reused lane-sized surface for transparent masked frames.
    alpha_surface: Option<(u32, u32, IDCompositionSurface)>,
    /// Both copies of the text, rasterized only when its content or color changes.
    alpha_text: Option<(String, [u32; 4], ID2D1Bitmap1)>,
    /// Gradient mask cached by lane width, opacity and left-edge fade state.
    alpha_gradient: Option<(u32, u32, u32, ID2D1LinearGradientBrush)>,
    /// Whether the last frame used the transparent mask path.
    alpha_mode: Option<bool>,
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
            Ok(Self {
                lane,
                text,
                fade_l,
                fade_r,
                opacity,
                left_opacity,
                surfaces: Vec::new(),
                painted: None,
                alpha_surface: None,
                alpha_text: None,
                alpha_gradient: None,
                alpha_mode: None,
                placed: None,
            })
        }
    }

    /// Shows the scrolling text. Flat layers repaint only on content changes; transparent masked
    /// layers repaint their small lane on every frame. Changes are committed by the caller.
    pub fn show(&mut self, gfx: &Gfx, req: &Request, t: f32, hinted: Option<&IDWriteRenderingParams>) -> Result<()> {
        let k = gfx.dpi / 96.0;
        let lane_w = req.r - req.l;
        let h = req.text.h + PAD_Y * 2.0;
        if self.alpha_mode != Some(req.alpha_mask) {
            // SAFETY: DirectComposition calls on live objects; every argument outlives its call.
            unsafe {
                if req.alpha_mask {
                    self.fade_l.SetContent(None::<&windows::core::IUnknown>)?;
                    self.fade_r.SetContent(None::<&windows::core::IUnknown>)?;
                    self.text.SetOffsetX2(0.0)?;
                }
            }
            self.painted = None;
            self.alpha_mode = Some(req.alpha_mask);
        }
        if req.alpha_mask {
            self.paint_alpha(gfx, req, &req.brush, lane_w, h, t, hinted)?;
        } else {
            // Background and text colors are part of the key: look changes require repainting.
            let key = (req.key.clone(), lane_w.to_bits(), req.bg.map(f32::to_bits), color_key(&req.brush));
            if self.painted.as_ref() != Some(&key) {
                self.repaint(gfx, req, &req.brush, lane_w, h)?;
                self.painted = Some(key);
            }
        }
        // Lane must be aligned to integer pixels to prevent text blurring. The masked path applies
        // opacity to the text itself rather than to the whole lane.
        let opacity = if req.alpha_mask { 1.0 } else { req.alpha };
        let place = ((req.l * k).round(), ((req.cy - h / 2.0) * k).round(), opacity);
        if self.placed != Some(place) {
            // SAFETY: DirectComposition calls on live objects; every argument outlives its call.
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

    /// Removes a dynamically owned lane from the composition tree instead of leaking hidden visuals.
    pub fn remove(mut self, gfx: &Gfx) -> Result<()> {
        self.hide()?;
        // SAFETY: `lane` is a live child of this root and is removed exactly once.
        unsafe { gfx.root.RemoveVisual(&self.lane)? };
        Ok(())
    }

    fn text_for_alpha(&mut self, gfx: &Gfx, req: &Request, fg: &ID2D1SolidColorBrush, h: f32, hinted: Option<&IDWriteRenderingParams>) -> Result<ID2D1Bitmap1> {
        let color = color_key(fg);
        if let Some((key, cached_color, bitmap)) = &self.alpha_text
            && key == &req.key
            && cached_color == &color
        {
            return Ok(bitmap.clone());
        }
        let k = gfx.dpi / 96.0;
        let cycle = req.text.w + MARQUEE_GAP;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: gfx.dpi,
            dpiY: gfx.dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            ..Default::default()
        };
        // SAFETY: Direct2D calls use live objects owned by `gfx`; the bitmap target is cleared and
        // released on every path after `BeginDraw`, and text parameters are restored before reuse.
        let bitmap = unsafe {
            let bitmap = gfx.dc.CreateBitmap(D2D_SIZE_U { width: ((cycle + req.text.w) * k).ceil() as u32, height: (h * k).ceil() as u32 }, None, 0, &props)?;
            gfx.dc.SetTarget(&bitmap);
            gfx.dc.BeginDraw();
            gfx.dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            fg.SetOpacity(1.0);
            if hinted.is_some() {
                gfx.dc.SetTextRenderingParams(None);
            }
            draw_text(&gfx.dc, &req.text, 0.0, PAD_Y, fg);
            draw_text(&gfx.dc, &req.text, cycle, PAD_Y, fg);
            if let Some(params) = hinted {
                gfx.dc.SetTextRenderingParams(Some(params));
            }
            let end = gfx.dc.EndDraw(None, None);
            gfx.dc.SetTarget(None);
            end?;
            bitmap
        };
        self.alpha_text = Some((req.key.clone(), color, bitmap.clone()));
        Ok(bitmap)
    }

    fn mask_for_alpha(&mut self, gfx: &Gfx, lane_w: f32, alpha: f32, left_fade: f32) -> Result<ID2D1LinearGradientBrush> {
        let key = (lane_w.to_bits(), alpha.to_bits(), left_fade.to_bits());
        if let Some((w, a, left, mask)) = &self.alpha_gradient
            && (*w, *a, *left) == key
        {
            return Ok(mask.clone());
        }
        let f = MARQUEE_FADE / lane_w;
        let black = |a| D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a };
        let coll = gradient(
            &gfx.dc,
            &[
                D2D1_GRADIENT_STOP { position: 0.0, color: black(alpha * (1.0 - left_fade)) },
                D2D1_GRADIENT_STOP { position: f, color: black(alpha) },
                D2D1_GRADIENT_STOP { position: 1.0 - f, color: black(alpha) },
                D2D1_GRADIENT_STOP { position: 1.0, color: black(0.0) },
            ],
        )?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: 0.0, Y: 0.0 }, endPoint: Vector2 { X: lane_w, Y: 0.0 } };
        // SAFETY: the device context and gradient collection are alive for this call.
        let mask = unsafe { gfx.dc.CreateLinearGradientBrush(&props, None, &coll)? };
        self.alpha_gradient = Some((key.0, key.1, key.2, mask.clone()));
        Ok(mask)
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_alpha(
        &mut self,
        gfx: &Gfx,
        req: &Request,
        fg: &ID2D1SolidColorBrush,
        lane_w: f32,
        h: f32,
        t: f32,
        hinted: Option<&IDWriteRenderingParams>,
    ) -> Result<()> {
        let key = (lane_w.to_bits(), h.to_bits());
        if self.alpha_surface.as_ref().is_none_or(|(w, h, _)| (*w, *h) != key) {
            let (surface, _) = gfx.layer_surface(lane_w, h)?;
            self.alpha_surface = Some((key.0, key.1, surface));
        }
        let surface = self.alpha_surface.as_ref().map(|(_, _, surface)| surface.clone()).unwrap();
        let bitmap = self.text_for_alpha(gfx, req, fg, h, hinted)?;
        let k = gfx.dpi / 96.0;
        let px = (((lane_w * k).ceil() as u32).max(1), ((h * k).ceil() as u32).max(1));
        let cycle = req.text.w + MARQUEE_GAP;
        let run = (t - req.since - req.delay).max(0.0);
        let off = (run * MARQUEE_SPEED) % cycle;
        let left_fade = (run * 4.0).min(1.0);
        let mask = self.mask_for_alpha(gfx, lane_w, req.alpha, left_fade)?;
        let bitmap_w = cycle + req.text.w;
        // SAFETY: Direct2D calls on live COM objects; the layer and its opacity brush are paired.
        gfx.paint(&surface, px, |dc| unsafe {
            let Ok(mask) = mask.cast() else { return };
            let rect = D2D_RECT_F { left: 0.0, top: 0.0, right: lane_w, bottom: h };
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: rect,
                geometricMask: std::mem::ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(Some(mask)),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            dc.PushLayer(&params, None);
            let text_rect = D2D_RECT_F { left: -off, top: 0.0, right: bitmap_w - off, bottom: h };
            dc.DrawBitmap(&bitmap, Some(&text_rect), req.alpha, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            dc.PopLayer();
            drop(std::mem::ManuallyDrop::into_inner(params.opacityBrush));
        })?;
        // SAFETY: DirectComposition calls on live objects; every argument outlives its call.
        unsafe {
            self.text.SetContent(&surface)?;
            self.lane.SetClip2(&D2D_RECT_F { left: 0.0, top: 0.0, right: (lane_w * k).round(), bottom: (h * k).round() })?;
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
