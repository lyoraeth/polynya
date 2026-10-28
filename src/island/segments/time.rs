// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Time and date segment. On hover: sunrise/sunset marquee.
//! On minute change, only modified digits roll vertically.

use windows::{
    Win32::{
        Graphics::{
            Direct2D::{Common::D2D_RECT_F, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_NONE, ID2D1SolidColorBrush},
            DirectWrite::DWRITE_HIT_TEST_METRICS,
        },
        System::SystemInformation::GetLocalTime,
    },
    core::Result,
};
use windows_numerics::Vector2;

use super::{Data, Hover, Segment};
use crate::island::{
    Action, Part,
    kit::{Font, Frame, Style, Text, draw_text, set_opacity},
};

const DATE_GAP: f32 = 7.0;
pub const HOVER_MARQUEE_DELAY: f32 = 0.35;
const ROLL_DUR: f32 = 0.55;
const ROLL_SHIFT: f32 = 0.38;

fn ease_out_quint(p: f32) -> f32 {
    1.0 - (1.0 - p).powi(5)
}

/// Returns horizontal boundaries for each character in the text layout.
fn cells(t: &Text, len: usize) -> Vec<(f32, f32)> {
    (0..len as u32)
        .map(|i| {
            let (mut x, mut y, mut m) = (0.0, 0.0, DWRITE_HIT_TEST_METRICS::default());
            // SAFETY: DirectWrite call on a live layout; the out pointers outlive the call.
            let _ = unsafe { t.layout.HitTestTextPosition(i, false, &mut x, &mut y, &mut m) };
            (x, x + m.width)
        })
        .collect()
}

#[derive(Default)]
pub struct Time {
    clock: String,
    prev: String,
    rolling: f32,
    whole: Option<Text>,
    prev_whole: Option<Text>,
    cells: Vec<(f32, f32)>,
    date: Option<Text>,
    has_weather: bool,
    marquee: bool,
    label: String,
}

impl Time {
    /// Renders the clock. During roll animation, each character cell is drawn separately but
    /// always from the whole layout: a separate layout per character rasterizes differently,
    /// and the digits looked bolder and doubled.
    fn draw_clock(&self, f: &Frame, x: f32, cy: f32, alpha: f32) {
        let fg = &f.br.fg;
        let Some(whole) = &self.whole else { return };
        let y = clock_y(whole, cy);
        let rolling = self.rolling > 0.0 && self.prev_whole.is_some() && self.prev.chars().count() == self.cells.len();
        if !rolling {
            set_opacity(fg, alpha);
            draw_text(f.dc, whole, x, y, fg);
            set_opacity(fg, 1.0);
            return;
        }
        let prev = self.prev_whole.as_ref().unwrap();
        let p = ease_out_quint(1.0 - self.rolling / ROLL_DUR);
        for (i, (c_old, c_new)) in self.prev.chars().zip(self.clock.chars()).enumerate() {
            let (l, r) = self.cells[i];
            let cell = D2D_RECT_F { left: x + l, top: y, right: x + r, bottom: y + whole.h };
            // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow
            // that outlives the call.
            unsafe { f.dc.PushAxisAlignedClip(&cell, D2D1_ANTIALIAS_MODE_ALIASED) };
            if c_old == c_new {
                set_opacity(fg, alpha);
                draw_at(f, whole, x, y, fg);
            } else {
                set_opacity(fg, alpha * (1.0 - p));
                draw_at(f, prev, x, y - whole.h * ROLL_SHIFT * p, fg);
                set_opacity(fg, alpha * p);
                draw_at(f, whole, x, y + whole.h * ROLL_SHIFT * (1.0 - p), fg);
            }
            // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow
            // that outlives the call.
            unsafe { f.dc.PopAxisAlignedClip() };
        }
        set_opacity(fg, 1.0);
    }
}

fn clock_y(whole: &Text, cy: f32) -> f32 {
    (cy - whole.h / 2.0).round()
}

/// Draws text without pixel snapping: during the roll, snapping would make it jerk pixel by pixel.
fn draw_at(f: &Frame, t: &Text, x: f32, y: f32, brush: &ID2D1SolidColorBrush) {
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe { f.dc.DrawTextLayout(Vector2 { X: x, Y: y }, &t.layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE) };
}

impl Segment for Time {
    fn part(&self) -> Part {
        Part::Time
    }

    fn click(&self) -> Action {
        Action::NotificationCenter
    }

    fn measure(&mut self, f: &mut Frame, data: &Data) -> Result<f32> {
        // SAFETY: `GetLocalTime` has no preconditions.
        let now = unsafe { GetLocalTime() };
        let clock = format!("{:02}:{:02}", now.wHour, now.wMinute);
        if clock != self.clock {
            if !self.clock.is_empty() {
                self.prev = std::mem::replace(&mut self.clock, clock);
                self.prev_whole = self.whole.take();
                self.rolling = ROLL_DUR;
            } else {
                self.clock = clock;
            }
        }
        let whole = f.txt(&self.clock, Font::Time, Style::Tabular { case: true })?;
        self.cells = cells(&whole, self.clock.chars().count());
        let date_s = crate::i18n::t().date(now.wDayOfWeek as usize, now.wDay, now.wMonth as usize + 11);
        self.label = format!("{}, {date_s}", self.clock);
        let date = f.txt(&date_s.to_uppercase(), Font::Date, Style::Plain)?;
        let w = whole.w + DATE_GAP + date.w;
        self.whole = Some(whole);
        self.date = Some(date);
        self.has_weather = data.weather.is_some();
        Ok(w)
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn hoverable(&self) -> bool {
        self.has_weather
    }

    fn step(&mut self, dt: f32) -> bool {
        if self.rolling > 0.0 {
            self.rolling = (self.rolling - dt).max(0.0);
            return true;
        }
        false
    }

    fn draw(&mut self, f: &mut Frame, data: &Data, x: f32, w: f32, cy: f32, hover: Hover) -> Result<()> {
        let dim = f.br.dim.clone();
        let th = hover.p;
        if th < 1.0 {
            self.draw_clock(f, x, cy, 1.0 - th);
            let (whole, date) = (self.whole.as_ref().unwrap(), self.date.as_ref().unwrap());
            set_opacity(&dim, 1.0 - th);
            let y = clock_y(whole, cy) + whole.baseline - date.baseline;
            draw_text(f.dc, date, x + whole.w + DATE_GAP, y, &dim);
        }
        self.marquee = false;
        if let (Some(wx), true) = (data.weather, th > 0.0) {
            let tr = crate::i18n::t();
            let s = f.txt(&format!("{} {} · {} {}", tr.sunrise, wx.sunrise, tr.sunset, wx.sunset), Font::Text, Style::Plain)?;
            self.marquee = s.w > w;
            set_opacity(&dim, th);
            f.marquee(&s, x, x + w, cy, &dim, th, hover.since, HOVER_MARQUEE_DELAY)?;
        }
        set_opacity(&dim, 1.0);
        Ok(())
    }

    fn wants_frames(&self, hover: f32) -> bool {
        self.rolling > 0.0 || (hover > 0.0 && self.marquee)
    }
}
