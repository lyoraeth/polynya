// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pill segments: components sharing the `Segment` contract.
//!
//! The island layout is agnostic to segment content; it arranges them by width,
//! tracks hover state, and passes frame data. A new segment requires a new
//! contract implementation and a registry entry.

mod player;
mod time;
mod weather;

pub use player::Player;
pub use time::{HOVER_MARQUEE_DELAY, Time};
pub use weather::Weather;

use windows::core::Result;

use super::{Action, Panel, Part, kit::Frame};
use crate::{media, weather as wx};

/// Data provided to segments for frame construction.
pub struct Data<'a> {
    pub media: &'a media::State,
    pub weather: Option<&'a wx::Weather>,
}

/// Segment hover state.
#[derive(Debug, Clone, Copy)]
pub struct Hover {
    /// Transition progress: 0.0 (default) to 1.0 (fully hovered).
    pub p: f32,
    /// Timestamp when hover began, used for marquee animation offsets.
    pub since: f32,
}

pub trait Segment {
    fn part(&self) -> Part;

    /// Panel opened by this segment. While open, the hover layer is suppressed.
    fn panel(&self) -> Option<Panel> {
        None
    }

    /// Action triggered by clicking the segment outside its buttons.
    fn click(&self) -> Action {
        match self.panel() {
            Some(p) => Action::Open(p, self.part()),
            None => Action::None,
        }
    }

    /// What the segment shows, in words: its name for screen readers. Updated in `measure`.
    fn label(&self) -> String {
        String::new()
    }

    /// Resting width. Called at the start of each frame.
    fn measure(&mut self, f: &mut Frame, data: &Data) -> Result<f32>;

    /// Width when fully hovered. Defaults to the resting width.
    fn hover_width(&self, rest: f32) -> f32 {
        rest
    }

    /// Indicates if the segment supports a hover layer based on the last frame's data.
    fn hoverable(&self) -> bool {
        false
    }

    /// Advances segment-specific animations. Returns `true` while motion is active.
    fn step(&mut self, _dt: f32) -> bool {
        false
    }

    /// Renders the segment within `[x, x + w]` centered vertically at `cy`.
    fn draw(&mut self, f: &mut Frame, data: &Data, x: f32, w: f32, cy: f32, hover: Hover) -> Result<()>;

    /// Requests high-frequency frames (e.g., for visualizers or marquees).
    fn wants_frames(&self, _hover: f32) -> bool {
        false
    }

    /// Requests once-per-second redraws (e.g., for ticking clocks).
    fn needs_ticks(&self, _hover: f32) -> bool {
        false
    }
}
