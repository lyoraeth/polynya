// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanded notification: icon, title, details, source, and close button.

use windows::{
    Win32::Graphics::{
        Direct2D::{Common::D2D_RECT_F, D2D1_ELLIPSE, D2D1_ROUNDED_RECT},
        DirectWrite::IDWriteFactory,
    },
    core::Result,
};

use super::{Area, PanelView, Reveal};
use crate::{
    icons::Icon,
    island::{
        kit::{Font, Frame, HotKind, Hotspot, Style, Texts, draw_text, inside, rgb, set_opacity},
        logo,
        segments::Data,
        zone::Note,
    },
};

const W: f32 = 380.0;
const PAD_X: f32 = 18.0;
const PAD_T: f32 = 14.0;
const PAD_B: f32 = 16.0;
const BOX: f32 = 30.0;
const BOX_R: f32 = 8.0;
const BOX_GAP: f32 = 12.0;
const CLOSE: f32 = 22.0;
const CLOSE_GAP: f32 = 12.0;
const TEXT_W: f32 = W - PAD_X * 2.0 - BOX - BOX_GAP - CLOSE - CLOSE_GAP;
const TITLE_GAP: f32 = 2.0;
const META_GAP: f32 = 6.0;
const SWATCH_GAP: f32 = 12.0;
const SWATCH_SPACE: f32 = 6.0;
const SWATCH_MAX: f32 = 22.0;
const SWATCH_W: f32 = W - PAD_X * 2.0 - BOX - BOX_GAP;
const RING_GAP: f32 = 2.5;
const RING_W: f32 = 1.5;
const APP_LINES: usize = 4;

fn swatch_d(n: usize) -> f32 {
    ((SWATCH_W - SWATCH_SPACE * n.saturating_sub(1) as f32) / n.max(1) as f32).min(SWATCH_MAX)
}

#[derive(Default)]
pub struct NotePanel {
    note: Option<Note>,
    depth: f32,
}

impl NotePanel {
    /// Sets content and recalculates depth based on word-wrapped text.
    pub fn set(&mut self, texts: &Texts, dw: &IDWriteFactory, note: &Note) -> Result<()> {
        let mut note = note.clone();
        if note.app.is_some() {
            note.body = texts.clamp_lines(dw, &note.body, Font::Small, TEXT_W, APP_LINES)?;
        }
        let mut h = texts.height(dw, &note.title, Font::Bold, TEXT_W)?;
        if !note.body.is_empty() {
            h += TITLE_GAP + texts.height(dw, &note.body, Font::Small, TEXT_W)?;
        }
        if !note.meta.is_empty() {
            h += META_GAP + texts.height(dw, &note.meta, Font::Tiny, TEXT_W)?;
        }
        if !note.swatches.is_empty() {
            h += SWATCH_GAP + swatch_d(note.swatches.len());
        }
        self.depth = PAD_T + h.max(BOX) + PAD_B;
        self.note = Some(note);
        Ok(())
    }

    /// Marks color `i` as selected and returns its `#rrggbb` hex string.
    pub fn pick(&mut self, i: usize) -> Option<String> {
        let n = self.note.as_mut()?;
        let c = *n.swatches.get(i)?;
        n.mark = Some(i);
        Some(proto::hex(c))
    }
}

impl PanelView for NotePanel {
    fn size(&self) -> (f32, f32) {
        (W, self.depth)
    }

    fn draw(&mut self, f: &mut Frame, _data: &Data, area: Area, reveal: Reveal) -> Result<()> {
        let Some(n) = &self.note else { return Ok(()) };
        let br = f.br;
        let (fg, dim, soft, plate) = (br.fg.clone(), br.dim.clone(), br.soft.clone(), br.select.clone());
        let (l, r, top) = (area.left() + PAD_X, area.right() - PAD_X, area.top + PAD_T);
        let tx = l + BOX + BOX_GAP;

        // Row 0: Icon box, title, close button.
        let (a0, r0) = reveal.row(0);
        let y0 = top + r0;
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            set_opacity(&plate, a0);
            f.dc.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: y0, right: l + BOX, bottom: y0 + BOX }, radiusX: BOX_R, radiusY: BOX_R },
                &plate,
            );
            set_opacity(&plate, 1.0);
        }
        logo::note_icon(f, n, l + BOX / 2.0, y0 + BOX / 2.0, 16.0, a0)?;
        set_opacity(&fg, a0);
        let title = f.txt(&n.title, Font::Bold, Style::wrap(TEXT_W))?;
        draw_text(f.dc, &title, tx, y0, &fg);
        set_opacity(&dim, a0);
        f.button(Icon::Close, HotKind::Dismiss, r - CLOSE / 2.0, y0 + CLOSE / 2.0, 12.0, CLOSE / 2.0, &dim, a0)?;
        let mut y = top + title.h;

        // Row 1: Body details.
        if !n.body.is_empty() {
            let (a1, r1) = reveal.row(1);
            let body = f.txt(&n.body, Font::Small, Style::wrap(TEXT_W))?;
            set_opacity(&soft, a1);
            draw_text(f.dc, &body, tx, y + TITLE_GAP + r1, &soft);
            y += TITLE_GAP + body.h;
        }

        // Row 2: Source meta.
        if !n.meta.is_empty() {
            let (a2, r2) = reveal.row(2);
            let meta = f.txt(&n.meta, Font::Tiny, Style::wrap(TEXT_W))?;
            set_opacity(&dim, a2);
            draw_text(f.dc, &meta, tx, y + META_GAP + r2, &dim);
            y += META_GAP + meta.h;
        }

        // Row 3: Color swatches.
        if !n.swatches.is_empty() {
            let (a3, r3) = reveal.row(3);
            let d = swatch_d(n.swatches.len());
            let cy = y + SWATCH_GAP + d / 2.0 + r3;
            for (i, &[cr, cg, cb]) in n.swatches.iter().enumerate() {
                let cx = tx + d / 2.0 + i as f32 * (d + SWATCH_SPACE);
                let rect = (cx - d / 2.0, cy - d / 2.0, cx + d / 2.0, cy + d / 2.0);
                let hover = f.mouse.is_some_and(|(mx, my)| inside(rect, mx, my));
                let mut c = rgb(cr, cg, cb);
                c.a = a3;
                // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow
                // that outlives the call.
                unsafe {
                    let brush = f.dc.CreateSolidColorBrush(&c, None)?;
                    f.dc.FillEllipse(&D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: cx, Y: cy }, radiusX: d / 2.0, radiusY: d / 2.0 }, &brush);
                    let ring = if n.mark == Some(i) {
                        Some(&fg)
                    } else if hover {
                        Some(&dim)
                    } else {
                        None
                    };
                    if let Some(ring) = ring {
                        set_opacity(ring, a3);
                        let rr = d / 2.0 + RING_GAP;
                        f.dc.DrawEllipse(&D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: cx, Y: cy }, radiusX: rr, radiusY: rr }, ring, RING_W, None);
                    }
                }
                if a3 > 0.5 {
                    f.hotspots.push(Hotspot { rect, kind: HotKind::Swatch(i) });
                }
            }
        }
        // Click outside close button opens the source app.
        if n.app.is_some() && a0 > 0.5 {
            f.hotspots.push(Hotspot { rect: (area.left(), area.top, area.right(), area.top + self.depth), kind: HotKind::OpenApp });
        }
        set_opacity(&fg, 1.0);
        set_opacity(&dim, 1.0);
        set_opacity(&soft, 1.0);
        Ok(())
    }
}
