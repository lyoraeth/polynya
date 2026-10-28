// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expandable panels sharing the `PanelView` contract.
//!
//! The island handles the shape, springs and clipping to the outline; a panel only draws its
//! content in its area and reveals rows following the shared `Reveal` choreography.

mod launcher;
mod menu;
mod note;
mod player;
mod stack;
mod weather;

pub use launcher::LauncherPanel;
pub use menu::{MenuEffect, MenuPanel};
pub use note::NotePanel;
pub use player::PlayerPanel;
pub use stack::{ROWS as STACK_ROWS, StackPanel};
pub use weather::WeatherPanel;

use windows::core::Result;

use super::{
    kit::{Frame, ease_out_cubic},
    segments::Data,
};

pub const CONTENT_START: f32 = 0.12;
pub const CONTENT_DUR: f32 = 0.30;
pub const CONTENT_STAGGER: f32 = 0.045;
pub const STAGGER_ROWS: usize = 6;
const CONTENT_OUT: f32 = 0.10;
const CONTENT_RISE: f32 = 8.0;

#[derive(Debug, Clone, Copy)]
pub struct Area {
    pub cx: f32,
    pub w: f32,
    pub d: f32,
    pub top: f32,
}

impl Area {
    pub fn left(&self) -> f32 {
        self.cx - self.w / 2.0
    }

    pub fn right(&self) -> f32 {
        self.cx + self.w / 2.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Reveal {
    pub since: f32,
    pub opening: bool,
}

impl Reveal {
    pub fn row(&self, row: usize) -> (f32, f32) {
        if self.opening {
            let p = ((self.since - CONTENT_START - CONTENT_STAGGER * row.min(STAGGER_ROWS) as f32) / CONTENT_DUR).clamp(0.0, 1.0);
            let e = ease_out_cubic(p);
            (e, (1.0 - e) * CONTENT_RISE)
        } else {
            ((1.0 - self.since / CONTENT_OUT).clamp(0.0, 1.0), 0.0)
        }
    }
}

pub trait PanelView {
    fn size(&self) -> (f32, f32);

    fn draw(&mut self, f: &mut Frame, data: &Data, area: Area, reveal: Reveal) -> Result<()>;
}
