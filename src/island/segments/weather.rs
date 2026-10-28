// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Weather segment: icon and temperature. On hover: detailed conditions marquee.

use windows::core::Result;

use super::{Data, Hover, Segment, time::HOVER_MARQUEE_DELAY};
use crate::{
    icons,
    island::{
        Panel, Part,
        kit::{Font, Frame, Style, Text, draw_text, set_opacity},
    },
    weather as wx,
};

/// Hover width exception: native ~50 DIP is too narrow for readable marquee.
/// Segment expands rightward while the island remains stationary.
const HOVER_W: f32 = 150.0;
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 5.0;

#[derive(Default)]
pub struct Weather {
    temp: Option<Text>,
    has_data: bool,
    marquee: bool,
    label: String,
}

impl Segment for Weather {
    fn part(&self) -> Part {
        Part::Weather
    }

    fn panel(&self) -> Option<Panel> {
        Some(Panel::Weather)
    }

    fn measure(&mut self, f: &mut Frame, data: &Data) -> Result<f32> {
        let temp = f.txt(&data.weather.map_or("—".to_string(), |w| wx::temp(w.now.temp)), Font::Text, Style::Plain)?;
        self.has_data = data.weather.is_some();
        self.label = data.weather.map_or_else(
            || crate::i18n::t().loading_weather.to_string(),
            |w| format!("{}, {}, {}", w.city, wx::temp(w.now.temp), crate::i18n::t().describe(w.now.code)),
        );
        let w = if self.has_data { ICON + ICON_GAP + temp.w } else { temp.w };
        self.temp = Some(temp);
        Ok(w)
    }

    fn hover_width(&self, rest: f32) -> f32 {
        rest.max(HOVER_W)
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn hoverable(&self) -> bool {
        self.has_data
    }

    fn draw(&mut self, f: &mut Frame, data: &Data, x: f32, w: f32, cy: f32, hover: Hover) -> Result<()> {
        let (fg, dim, bg) = (f.br.fg.clone(), f.br.dim.clone(), f.br.body.clone());
        let temp = self.temp.as_ref().unwrap();
        self.marquee = false;
        let Some(wx) = data.weather else {
            draw_text(f.dc, temp, x, cy - temp.h / 2.0, &dim);
            return Ok(());
        };
        let wh = hover.p;
        if wh < 1.0 {
            set_opacity(&fg, 1.0 - wh);
            icons::weather(f.gfx, f.dc, wx::sky(wx.now.code), wx.now.is_day, x + ICON / 2.0, cy, ICON, &fg, &bg)?;
            draw_text(f.dc, temp, x + ICON + ICON_GAP, cy - temp.h / 2.0, &fg);
        }
        if wh > 0.0 {
            let tr = crate::i18n::t();
            let s = format!(
                "{} · {} {} · {} {:.0} {} · {} {:.0}%",
                tr.describe(wx.now.code),
                tr.feels_like,
                wx::temp(wx.now.feels),
                tr.wind,
                wx.now.wind,
                tr.ms,
                tr.humidity,
                wx.now.humidity
            );
            let s = f.txt(&s, Font::Text, Style::Plain)?;
            self.marquee = s.w > w;
            set_opacity(&dim, wh);
            f.marquee(&s, x, x + w, cy, &dim, wh, hover.since, HOVER_MARQUEE_DELAY)?;
        }
        set_opacity(&fg, 1.0);
        set_opacity(&dim, 1.0);
        Ok(())
    }

    fn wants_frames(&self, hover: f32) -> bool {
        hover > 0.0 && self.marquee
    }
}
