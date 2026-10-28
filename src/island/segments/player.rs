// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Player segment: visualizer and track title. On hover: controls, progress bar, and timestamps.

use windows::{
    Win32::Graphics::Direct2D::{Common::D2D_RECT_F, D2D1_ROUNDED_RECT},
    core::Result,
};

use super::{Data, Hover, Segment};
use crate::{
    audio,
    icons::Icon,
    island::{
        Panel, Part,
        kit::{Font, Frame, HotKind, Style, Text, draw_text, fmt_time, set_opacity},
        scroller::Request,
    },
    media::Cmd,
};

const MAX_W: f32 = 263.0;
const MARQUEE_DELAY: f32 = 1.2;
const BAR_W: f32 = 3.0;
const BAR_GAP: f32 = 2.0;
const VIZ_W: f32 = audio::BANDS as f32 * BAR_W + (audio::BANDS - 1) as f32 * BAR_GAP;
const TITLE_GAP: f32 = 10.0;
const BAR_MIN_H: f32 = 3.0;
const BAR_MAX_H: f32 = 14.0;
const ATTACK: f32 = 0.04;
const RELEASE: f32 = 0.18;
const SETTLED: f32 = 0.002;
const HOV_BUTTONS_W: f32 = 66.0;
const HOV_BAR_MIN_W: f32 = 36.0;
const HOV_SPACING: f32 = 8.0;

pub struct Player {
    title: Option<Text>,
    idle: Option<Text>,
    has_track: bool,
    playing: bool,
    overflow: bool,
    marquee_key: String,
    marquee_since: f32,
    viz: [f32; audio::BANDS],
    settled: bool,
    label: String,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            title: None,
            idle: None,
            has_track: false,
            playing: false,
            overflow: false,
            marquee_key: String::new(),
            marquee_since: 0.0,
            viz: [0.0; audio::BANDS],
            settled: true,
            label: String::new(),
        }
    }
}

impl Segment for Player {
    fn part(&self) -> Part {
        Part::Player
    }

    fn panel(&self) -> Option<Panel> {
        Some(Panel::Player)
    }

    fn measure(&mut self, f: &mut Frame, data: &Data) -> Result<f32> {
        let m = data.media;
        self.has_track = m.track.is_some();
        self.playing = m.playing;
        self.label = match &m.track {
            Some(tr) if tr.artist.is_empty() => tr.title.clone(),
            Some(tr) => format!("{} · {}", tr.title, tr.artist),
            None => crate::i18n::t().nothing_playing.to_string(),
        };
        self.title = match &m.track {
            Some(tr) => {
                let s = if tr.artist.is_empty() { tr.title.clone() } else { format!("{} · {}", tr.title, tr.artist) };
                if s != self.marquee_key {
                    self.marquee_key = s.clone();
                    self.marquee_since = f.t;
                }
                let from = tr.title.encode_utf16().count() as u32;
                Some(f.txt(&s, Font::Text, Style::Title { from })?)
            }
            None => None,
        };
        let room = MAX_W - VIZ_W - TITLE_GAP;
        self.overflow = self.title.as_ref().is_some_and(|t| t.w > room);
        Ok(match &self.title {
            Some(t) => VIZ_W + TITLE_GAP + t.w.min(room),
            None => {
                let idle = f.txt(crate::i18n::t().nothing_playing, Font::Text, Style::Plain)?;
                let w = idle.w;
                self.idle = Some(idle);
                w
            }
        })
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn hoverable(&self) -> bool {
        self.has_track
    }

    fn step(&mut self, dt: f32) -> bool {
        let levels = if self.playing && self.has_track { audio::levels() } else { [0.0; audio::BANDS] };
        let mut settled = true;
        for (v, target) in self.viz.iter_mut().zip(levels) {
            let tau = if target > *v { ATTACK } else { RELEASE };
            *v += (target - *v) * (1.0 - (-dt / tau).exp());
            settled &= (target - *v).abs() < SETTLED;
        }
        self.settled = settled;
        false
    }

    fn draw(&mut self, f: &mut Frame, data: &Data, x: f32, w: f32, cy: f32, hover: Hover) -> Result<()> {
        let (fg, dim) = (f.br.fg.clone(), f.br.dim.clone());
        let Some(title) = self.title.clone() else {
            let idle = self.idle.as_ref().unwrap();
            draw_text(f.dc, idle, x, cy - idle.h / 2.0, &dim);
            return Ok(());
        };
        let m = data.media;
        let hov = hover.p;

        // Default layer
        if hov < 1.0 {
            set_opacity(&fg, 1.0 - hov);
            set_opacity(&dim, 1.0 - hov);
            for (i, level) in self.viz.iter().enumerate() {
                let hgt = BAR_MIN_H + (BAR_MAX_H - BAR_MIN_H) * level;
                let bx = x + i as f32 * (BAR_W + BAR_GAP);
                // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow
                // that outlives the call.
                unsafe {
                    f.dc.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: bx, top: cy - hgt / 2.0, right: bx + BAR_W, bottom: cy + hgt / 2.0 },
                            radiusX: 1.5,
                            radiusY: 1.5,
                        },
                        &fg,
                    );
                }
            }
            let (l, r) = (x + VIZ_W + TITLE_GAP, x + w);
            if self.overflow && f.glass {
                // On glass the background under the title is uneven: solid edges of the composition layer
                // showed as rectangles, so the edges fade with a mask, at the cost of frames while
                // it scrolls.
                set_opacity(&fg, 1.0 - hov);
                f.marquee(&title, l, r, cy, &fg, 1.0 - hov, self.marquee_since, MARQUEE_DELAY)?;
                set_opacity(&fg, 1.0);
            } else if self.overflow {
                *f.scroll =
                    Some(Request { key: self.marquee_key.clone(), text: title.clone(), l, r, cy, alpha: 1.0 - hov, delay: MARQUEE_DELAY, bg: f.backdrop });
            } else {
                draw_text(f.dc, &title, l, cy - title.h / 2.0, &fg);
            }
        }

        // Hover layer; a narrow segment simplifies it: the progress goes first, then the time.
        if hov > 0.0 {
            set_opacity(&fg, hov);
            set_opacity(&dim, hov);
            let el = f.txt(&fmt_time(m.position()), Font::Tiny, Style::Tabular { case: false })?;
            let with_time = w >= HOV_BUTTONS_W + HOV_SPACING + el.w;
            let with_bar = w >= HOV_BUTTONS_W + HOV_SPACING * 2.0 + HOV_BAR_MIN_W + el.w;
            let play = if m.playing { Icon::Pause } else { Icon::Play };
            let mut bx = if with_time { x + w - HOV_BUTTONS_W } else { x + (w - HOV_BUTTONS_W) / 2.0 } + 11.0;
            for (icon, cmd) in [(Icon::Prev, Cmd::Prev), (play, Cmd::TogglePlay), (Icon::Next, Cmd::Next)] {
                f.button(icon, HotKind::Media(cmd), bx, cy, 14.0, 11.0, &fg, hov)?;
                bx += 22.0;
            }
            if with_time {
                draw_text(f.dc, &el, x, cy - el.h / 2.0, &dim);
            }
            if with_bar {
                f.progress(m, x + el.w + HOV_SPACING, x + w - HOV_BUTTONS_W - HOV_SPACING, cy, 3.0, &fg, hov);
            }
        }
        set_opacity(&fg, 1.0);
        set_opacity(&dim, 1.0);
        Ok(())
    }

    fn wants_frames(&self, hover: f32) -> bool {
        let default_layer_visible = self.has_track && hover < 1.0;
        default_layer_visible && (self.playing || !self.settled)
    }

    fn needs_ticks(&self, hover: f32) -> bool {
        self.playing && hover > 0.0
    }
}
