// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanded media player: cover art, metadata, progress bar, and controls.
//! Background features a blurred version of the cover art.

use windows::{
    Win32::Graphics::Direct2D::{
        Common::{D2D_RECT_F, D2D1_GRADIENT_STOP},
        D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_OPTIONS1_NONE,
        D2D1_LAYER_PARAMETERS1, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, ID2D1Bitmap1,
    },
    core::{Interface, Result},
};
use windows_numerics::{Matrix3x2, Vector2};

use super::{Area, PanelView, Reveal};
use crate::{
    icons::Icon,
    island::{
        kit::{Font, Frame, HotKind, Style, draw_text, fmt_time, gradient, set_opacity},
        segments::Data,
    },
    media::{self, Cmd},
    shape,
};

const SIZE: (f32, f32) = (400.0, 124.0);
const PAD: f32 = 18.0;
const COVER: f32 = 84.0;
const COVER_TOP: f32 = 20.0;
/// Background blur strength and cover brightness overlay.
const COVER_BLUR: f32 = 28.0;
const COVER_BG_OPACITY: f32 = 0.5;
/// Blurred cover is stored at this scale. Details are lost after blur, allowing smooth upscaling.
const BLUR_SCALE: f32 = 0.125;

pub struct PlayerPanel {
    /// Current track cover art and its revision number from the media state.
    cover: Option<ID2D1Bitmap1>,
    /// Blurred cover for the background, computed once per track.
    blurred: Option<ID2D1Bitmap1>,
    cover_rev: u64,
}

impl Default for PlayerPanel {
    fn default() -> Self {
        Self { cover: None, blurred: None, cover_rev: u64::MAX }
    }
}

impl PlayerPanel {
    /// Decodes the cover art only when the media session revision changes.
    fn sync_cover(&mut self, f: &Frame, m: &media::State) {
        if m.cover_rev != self.cover_rev {
            self.cover_rev = m.cover_rev;
            self.cover = m.cover.as_ref().and_then(|b| match f.gfx.decode(b) {
                Ok(cover) => Some(cover),
                Err(e) => {
                    proto::warn!("media", "cover decode ({} bytes): {e}", b.len());
                    None
                }
            });
            self.blurred = self.cover.as_ref().and_then(|c| f.gfx.blurred(c, COVER_BLUR, BLUR_SCALE).ok());
        }
    }

    /// Background: blurred cover, brighter near the panel, fading to the island body color at edges.
    fn background(&self, f: &Frame, blurred: &ID2D1Bitmap1, area: Area, alpha: f32) -> Result<()> {
        let dc = f.dc;
        let (_, pd) = SIZE;
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            // Blurred cover is stored downscaled; calculate original dimensions.
            let small = blurred.GetSize();
            let size = (small.width / BLUR_SCALE, small.height / BLUR_SCALE);
            // Scale up to cover the entire panel area with a margin.
            let k = (area.w / size.0).max(pd / size.1) * 1.3;
            let (ox, oy) = (area.cx - size.0 * k / 2.0, area.top + pd / 2.0 - size.1 * k / 2.0);
            let dst = D2D_RECT_F { left: ox, top: oy, right: ox + size.0 * k, bottom: oy + size.1 * k };
            dc.DrawBitmap(blurred, Some(&dst), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);

            // Darken the entire shape, including concave joints and taskbar overlap, to prevent
            // the raw cover from showing through unshaded areas.
            let (sl, st, sr, sb) = shape::flow_bounds(area.cx, area.w, area.d, area.top);
            let shape_rect = D2D_RECT_F { left: sl, top: st, right: sr, bottom: sb };
            let veil = &f.br.body;
            veil.SetOpacity(1.0 - COVER_BG_OPACITY * alpha);
            dc.FillRectangle(&shape_rect, veil);
            veil.SetOpacity(1.0);
            // Fade to the island body color (dark or light based on theme) at the edges.
            let body = |a: f32| {
                let mut c = veil.GetColor();
                c.a = a;
                c
            };
            let coll = gradient(
                dc,
                &[
                    D2D1_GRADIENT_STOP { position: 0.0, color: body(0.0) },
                    D2D1_GRADIENT_STOP { position: 0.45, color: body(0.45) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: body(1.0) },
                ],
            )?;
            let radial = dc.CreateRadialGradientBrush(
                &D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                    center: Vector2 { X: area.cx, Y: area.top },
                    gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
                    radiusX: area.w * 0.62,
                    radiusY: pd,
                },
                None,
                &coll,
            )?;
            dc.FillRectangle(&shape_rect, &radial);
        }
        Ok(())
    }

    /// Cover with rounded corners; non-square images are center-cropped.
    fn cover(&self, f: &Frame, cover: &ID2D1Bitmap1, rect: D2D_RECT_F, alpha: f32) -> Result<()> {
        // SAFETY: Direct2D calls on live COM objects; `params` and its mask outlive the calls, and the
        // mask is released exactly once at the end.
        unsafe {
            // Round corners via a layer with a rounded rectangle geometric mask.
            let rr = f.gfx.d2d.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect, radiusX: 10.0, radiusY: 10.0 })?;
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: rect,
                geometricMask: std::mem::ManuallyDrop::new(Some(rr.cast()?)),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: alpha,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            f.dc.PushLayer(&params, None);
            // Covers may be non-square (e.g., 16:9 video previews). Extract the center square.
            let size = cover.GetSize();
            let side = size.width.min(size.height);
            let src = D2D_RECT_F {
                left: (size.width - side) / 2.0,
                top: (size.height - side) / 2.0,
                right: (size.width + side) / 2.0,
                bottom: (size.height + side) / 2.0,
            };
            f.dc.DrawBitmap(cover, Some(&rect), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, Some(&src), None);
            f.dc.PopLayer();
            // Take the mask back out of the ManuallyDrop so the geometry is released.
            drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
        }
        Ok(())
    }
}

impl PanelView for PlayerPanel {
    fn size(&self) -> (f32, f32) {
        SIZE
    }

    fn draw(&mut self, f: &mut Frame, data: &Data, area: Area, reveal: Reveal) -> Result<()> {
        let m = data.media;
        self.sync_cover(f, m);
        let (fg, dim) = (f.br.fg.clone(), f.br.dim.clone());
        let (pl, pt) = (area.left(), area.top);

        let (bg_alpha, _) = reveal.row(0);
        if let (Some(blurred), true) = (&self.blurred, bg_alpha > 0.0) {
            self.background(f, blurred, area, bg_alpha)?;
        }
        // Panel edge blends into the track background.
        f.draw_edge(false);

        let Some(track) = &m.track else {
            let (a, rise) = reveal.row(0);
            let t = f.txt(crate::i18n::t().no_player, Font::Small, Style::Plain)?;
            set_opacity(&dim, a);
            draw_text(f.dc, &t, pl + PAD, pt + PAD + rise, &dim);
            set_opacity(&dim, 1.0);
            return Ok(());
        };

        // Row 0: Cover art and metadata.
        let (a0, r0) = reveal.row(0);
        let cover_rect = D2D_RECT_F { left: pl + PAD, top: pt + COVER_TOP + r0, right: pl + PAD + COVER, bottom: pt + COVER_TOP + COVER + r0 };
        match &self.cover {
            Some(cover) => self.cover(f, cover, cover_rect, a0)?,
            // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow
            // that outlives the call.
            None => unsafe {
                f.br.track.SetOpacity(a0);
                f.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: cover_rect, radiusX: 10.0, radiusY: 10.0 }, &f.br.track);
            },
        }
        let bx = pl + PAD + COVER + 14.0;
        let br = area.right() - PAD;
        let meta_s = match (track.artist.is_empty(), track.album.is_empty()) {
            (false, false) => format!("{} — {}", track.artist, track.album),
            (false, true) => track.artist.clone(),
            _ => track.album.clone(),
        };
        let meta = f.txt(&meta_s, Font::Small, Style::trim(br - bx))?;
        set_opacity(&dim, a0);
        draw_text(f.dc, &meta, bx, pt + COVER_TOP + r0, &dim);

        // Row 1: Progress bar and timestamps.
        let (a1, r1) = reveal.row(1);
        let bar_y = pt + COVER_TOP + meta.h + 12.0 + r1;
        set_opacity(&fg, a1);
        f.progress(m, bx, br, bar_y, 4.0, &fg, a1);
        let cur = f.txt(&fmt_time(m.position()), Font::Tiny, Style::Tabular { case: false })?;
        let dur = f.txt(&fmt_time(m.duration), Font::Tiny, Style::Tabular { case: false })?;
        set_opacity(&dim, a1);
        draw_text(f.dc, &cur, bx, bar_y + 6.0, &dim);
        draw_text(f.dc, &dur, br - dur.w, bar_y + 6.0, &dim);

        // Row 2: Playback controls.
        let (a2, r2) = reveal.row(2);
        set_opacity(&fg, a2);
        let row_y = pt + 86.0 + r2;
        let mid = (bx + br) / 2.0;
        let play = if m.playing { Icon::Pause } else { Icon::Play };
        for (icon, cmd, x) in [(Icon::Prev, Cmd::Prev, mid - 44.0), (play, Cmd::TogglePlay, mid), (Icon::Next, Cmd::Next, mid + 44.0)] {
            f.button(icon, HotKind::Media(cmd), x, row_y, 18.0, 14.0, &fg, a2)?;
        }
        set_opacity(&fg, 1.0);
        set_opacity(&dim, 1.0);
        Ok(())
    }
}
