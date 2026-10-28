// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Weather forecast panel: current conditions, hourly, and daily outlook.

use windows::core::Result;

use super::{Area, PanelView, Reveal};
use crate::{
    icons,
    island::{
        kit::{Font, Frame, Style, draw_text, set_opacity},
        segments::Data,
    },
    weather as wx,
};

const PAD: f32 = 18.0;
/// Geolocation icon (Lucide navigation) prefixing the location name and its text gap.
const NAVIGATION: &str = "M3 11l19-9-9 19-2-8-8-2z";
const PIN: f32 = 11.0;
const PIN_GAP: f32 = 4.0;

#[derive(Default)]
pub struct WeatherPanel;

impl PanelView for WeatherPanel {
    fn size(&self) -> (f32, f32) {
        (340.0, 196.0)
    }

    fn draw(&mut self, f: &mut Frame, data: &Data, area: Area, reveal: Reveal) -> Result<()> {
        let (fg, dim, bg) = (f.br.fg.clone(), f.br.dim.clone(), f.br.body.clone());
        let (x0, x1, pt) = (area.left() + PAD, area.right() - PAD, area.top);
        let Some(w) = data.weather else {
            let (a, rise) = reveal.row(0);
            let t = f.txt(crate::i18n::t().loading_weather, Font::Small, Style::Plain)?;
            set_opacity(&dim, a);
            draw_text(f.dc, &t, x0, pt + 16.0 + rise, &dim);
            set_opacity(&dim, 1.0);
            return Ok(());
        };

        // Row 0: Large temperature, location, condition, and feels-like.
        let (a0, r0) = reveal.row(0);
        let big = f.txt(&wx::temp(w.now.temp), Font::Big, Style::Plain)?;
        let y0 = pt + 12.0 + r0;
        set_opacity(&fg, a0);
        draw_text(f.dc, &big, x0, y0, &fg);
        let side = x0 + big.w + 14.0;
        let room = x1 - side;
        let tr = crate::i18n::t();
        // Location name might be unresolved (geolocation without geocoder response).
        let line1 = if w.city.is_empty() { tr.describe(w.now.code).to_string() } else { format!("{} · {}", w.city, tr.describe(w.now.code)) };
        // Geolocation prefix: navigation icon before the text.
        let pin = if w.located { PIN + PIN_GAP } else { 0.0 };
        let line1 = f.txt(&line1, Font::Small, Style::trim(room - pin))?;
        let line2 = f.txt(&format!("{} {} · {} {:.0} {}", tr.feels_like, wx::temp(w.now.feels), tr.wind, w.now.wind, tr.ms), Font::Small, Style::trim(room))?;
        let ly = y0 + (big.h - (line1.h + 2.0 + line2.h)) / 2.0;
        if w.located {
            icons::draw_path(f.gfx, f.dc, NAVIGATION, side + PIN / 2.0, ly + line1.h / 2.0, PIN, &fg)?;
        }
        draw_text(f.dc, &line1, side + pin, ly, &fg);
        set_opacity(&dim, a0);
        draw_text(f.dc, &line2, side, ly + line1.h + 2.0, &dim);

        // Row 1: Hourly forecast (hour, icon, temperature).
        let (a1, r1) = reveal.row(1);
        let y1 = pt + 12.0 + big.h + 10.0 + r1;
        let col = (x1 - x0) / w.hours.len().max(1) as f32;
        for (i, h) in w.hours.iter().enumerate() {
            let cx = x0 + col * (i as f32 + 0.5);
            let label = f.txt(&format!("{:02}", h.hour), Font::Tiny, Style::Tabular { case: false })?;
            let tt = f.txt(&wx::temp(h.temp), Font::Small, Style::Plain)?;
            set_opacity(&dim, a1);
            draw_text(f.dc, &label, cx - label.w / 2.0, y1, &dim);
            set_opacity(&fg, a1);
            icons::weather(f.gfx, f.dc, wx::sky(h.code), h.is_day, cx, y1 + label.h + 11.0, 16.0, &fg, &bg)?;
            draw_text(f.dc, &tt, cx - tt.w / 2.0, y1 + label.h + 21.0, &fg);
        }

        // Row 2: Daily forecast (day of week, icon, description, temperature range).
        let (a2, r2) = reveal.row(2);
        let mut y2 = pt + 12.0 + big.h + 10.0 + 58.0 + r2;
        for d in &w.days {
            let dow = f.txt(&crate::i18n::capitalize(tr.dow[d.dow as usize % 7]), Font::Small, Style::Plain)?;
            let range = f.txt(&format!("{} … {}", wx::temp(d.min), wx::temp(d.max)), Font::Small, Style::Plain)?;
            let desc = f.txt(tr.describe(d.code), Font::Small, Style::trim(x1 - range.w - 12.0 - (x0 + 52.0)))?;
            set_opacity(&fg, a2);
            draw_text(f.dc, &dow, x0, y2, &fg);
            icons::weather(f.gfx, f.dc, wx::sky(d.code), true, x0 + 38.0, y2 + dow.h / 2.0, 14.0, &fg, &bg)?;
            set_opacity(&dim, a2);
            draw_text(f.dc, &desc, x0 + 52.0, y2, &dim);
            set_opacity(&fg, a2);
            draw_text(f.dc, &range, x1 - range.w, y2, &fg);
            y2 += dow.h + 4.0;
        }
        set_opacity(&fg, 1.0);
        set_opacity(&dim, 1.0);
        Ok(())
    }
}
