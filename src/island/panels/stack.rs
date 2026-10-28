// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanded notification stack: list of notifications that didn't fit in the right zone.
//! Clicking a row opens the notification details.

use windows::core::Result;

use super::{Area, PanelView, Reveal};
use crate::island::{
    Part,
    kit::{Font, Frame, HotKind, Hotspot, Style, draw_text, inside, set_opacity},
    logo,
    segments::Data,
    zone::Note,
};

const W: f32 = 360.0;
/// Number of rows that fit, matching the height of an expanded notification.
pub const ROWS: usize = 3;
const PAD_Y: f32 = 12.0;
const ROW_H: f32 = 24.0;
const ROW_INSET: f32 = 8.0;
const ROW_PAD: f32 = 8.0;
const ICON: f32 = 14.0;
const ICON_GAP: f32 = 8.0;
const TEXT_GAP: f32 = 8.0;

#[derive(Default)]
pub struct StackPanel {
    glide: crate::island::kit::Glide,
    /// Row under the mouse; remains selected when the mouse leaves.
    sel: Option<usize>,
    notes: Vec<(Part, Note)>,
}

impl StackPanel {
    pub fn set(&mut self, notes: Vec<(Part, Note)>) {
        self.notes = notes;
    }

    pub fn rows(&self) -> usize {
        self.notes.len()
    }

    /// Highlight plate is still animating, requiring frames.
    pub fn moving(&self) -> bool {
        self.glide.moving()
    }
}

impl PanelView for StackPanel {
    fn size(&self) -> (f32, f32) {
        (W, PAD_Y * 2.0 + ROW_H * self.notes.len().max(1) as f32)
    }

    fn draw(&mut self, f: &mut Frame, _data: &Data, area: Area, reveal: Reveal) -> Result<()> {
        let br = f.br;
        let (fg, dim, plate) = (br.fg.clone(), br.dim.clone(), br.select.clone());
        let (l, r) = (area.left() + ROW_INSET, area.right() - ROW_INSET);
        let row_rect = |i: usize| {
            let top = area.top + PAD_Y + ROW_H * i as f32 + reveal.row(i).1;
            (l, top, r, top + ROW_H)
        };
        if let Some(i) = (0..self.notes.len()).find(|&i| f.mouse.is_some_and(|(mx, my)| inside(row_rect(i), mx, my))) {
            self.sel = Some(i);
        }
        self.sel = self.sel.filter(|&i| i < self.notes.len());
        self.glide.step(self.sel.map(row_rect), f.t);
        if let Some(i) = self.sel {
            self.glide.draw(f.dc, &plate, reveal.row(i).0, 8.0);
        }
        for (i, (part, n)) in self.notes.iter().enumerate() {
            let (a, rise) = reveal.row(i);
            let top = area.top + PAD_Y + ROW_H * i as f32 + rise;
            let rect = (l, top, r, top + ROW_H);
            let cy = top + ROW_H / 2.0;
            if a > 0.5 {
                f.hotspots.push(Hotspot { rect, kind: HotKind::Note(*part) });
            }
            let x = l + ROW_PAD;
            logo::note_icon(f, n, x + ICON / 2.0, cy, ICON, a)?;
            let tx = x + ICON + ICON_GAP;
            let title = f.txt(&crate::island::zone::one_line(&n.title), Font::Text, Style::trim((r - ROW_PAD - tx) * 0.6))?;
            set_opacity(&fg, a);
            draw_text(f.dc, &title, tx, cy - title.h / 2.0, &fg);
            let bx = tx + title.w + TEXT_GAP;
            if !n.body.is_empty() && bx < r - ROW_PAD {
                let body = f.txt(&crate::island::zone::one_line(&n.body), Font::Small, Style::trim(r - ROW_PAD - bx))?;
                set_opacity(&dim, a);
                draw_text(f.dc, &body, bx, cy - body.h / 2.0, &dim);
            }
        }
        set_opacity(&fg, 1.0);
        set_opacity(&dim, 1.0);
        Ok(())
    }
}
