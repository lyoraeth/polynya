// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Drawing a one-line text field ([`LineEdit`]): text that scrolls to keep the caret in view, the
//! selection, a caret that blinks at the system rate, and the mouse. A click puts the caret under
//! the cursor, a drag selects, a double click selects a word. The search line and the input rows of
//! module screens draw through it.

use std::{sync::OnceLock, time::Instant};

use windows::{
    Win32::{
        Graphics::{Direct2D::Common::D2D_RECT_F, DirectWrite::DWRITE_HIT_TEST_METRICS},
        UI::{Input::KeyboardAndMouse::GetDoubleClickTime, WindowsAndMessaging::GetCaretBlinkTime},
    },
    core::{BOOL, Result},
};

use super::kit::{Font, Frame, Rect, Style, Text, draw_text, inside, set_opacity};
use crate::edit::LineEdit;

const CARET_W: f32 = 1.5;
/// Double clicks also need the cursor to stay within this distance.
const DOUBLE_SLOP: f32 = 4.0;

/// Seconds since start: blinking runs on wall time, the island's own clock stops at rest.
fn now() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// Half a blink period from the system setting; `None` when blinking is turned off in Windows.
fn blink() -> Option<f32> {
    static HALF: OnceLock<Option<f32>> = OnceLock::new();
    *HALF.get_or_init(|| {
        // SAFETY: `GetCaretBlinkTime` has no preconditions.
        let ms = unsafe { GetCaretBlinkTime() };
        (ms != 0 && ms != u32::MAX).then(|| ms as f32 / 1000.0)
    })
}

enum Pointer {
    Press { x: f32, shift: bool, double: bool },
    Drag { x: f32 },
}

/// View state of a field: where it is, how far it scrolled, and the mouse waiting for the next frame
/// (a position becomes a character only with the text layout at hand).
#[derive(Default)]
pub struct Field {
    scroll: f32,
    pointer: Option<Pointer>,
    /// Caret, length and selection as last drawn: a change restarts the blink.
    seen: (usize, usize, Option<(usize, usize)>),
    since: f32,
    last_press: Option<(f32, f32)>,
    /// The text area as last drawn, for hit testing.
    rect: Option<Rect>,
}

impl Field {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.rect.is_some_and(|r| inside(r, x, y))
    }

    /// The mouse went down in the field.
    pub fn press(&mut self, x: f32, shift: bool) {
        let t = now();
        // SAFETY: `GetDoubleClickTime` has no preconditions.
        let gap = unsafe { GetDoubleClickTime() } as f32 / 1000.0;
        let double = self.last_press.is_some_and(|(t0, x0)| t - t0 <= gap && (x - x0).abs() <= DOUBLE_SLOP);
        self.last_press = (!double).then_some((t, x));
        self.pointer = Some(Pointer::Press { x, shift, double });
    }

    /// The mouse moved with the button down after a press in the field.
    pub fn drag(&mut self, x: f32) {
        if !matches!(self.pointer, Some(Pointer::Press { .. })) {
            self.pointer = Some(Pointer::Drag { x });
        }
    }

    /// Seconds until the caret blinks next: the island sleeps until then.
    pub fn next_blink(&self) -> Option<f32> {
        let half = blink()?;
        let t = now() - self.since;
        Some(half - t % half)
    }

    /// Draws `line` from `x` within `room`, centered on `cy`; the caret only if `active`. Returns the
    /// caret's x, for placing the IME window.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, f: &mut Frame, line: &mut LineEdit, font: Font, x: f32, room: f32, cy: f32, caret_h: f32, active: bool, a: f32) -> Result<f32> {
        let text = (!line.text.is_empty()).then(|| f.txt(&line.text, font, Style::Plain)).transpose()?;
        if let Some(p) = self.pointer.take() {
            let px = match p {
                Pointer::Press { x: px, .. } | Pointer::Drag { x: px } => px - x + self.scroll,
            };
            let at = text.as_ref().map_or(0, |t| hit(t, px));
            match p {
                Pointer::Press { shift, double, .. } => line.press(at, shift, double),
                Pointer::Drag { .. } => line.drag(at),
            }
        }
        let seen = (line.caret, line.text.len(), line.selection());
        if seen != self.seen {
            self.seen = seen;
            self.since = now();
        }
        let pos = |t: &Text, i: u32| {
            let (mut px, mut py, mut m) = (0.0, 0.0, DWRITE_HIT_TEST_METRICS::default());
            // SAFETY: DirectWrite call on a live layout; the out pointers outlive the call.
            let _ = unsafe { t.layout.HitTestTextPosition(i, false, &mut px, &mut py, &mut m) };
            px
        };
        let caret = text.as_ref().map_or(0.0, |t| pos(t, line.caret_utf16()));
        // Scroll just enough to keep the caret in view, and no further than the end of the text.
        let width = text.as_ref().map_or(0.0, |t| t.w);
        if caret - self.scroll > room - CARET_W {
            self.scroll = caret - room + CARET_W;
        }
        if caret < self.scroll {
            self.scroll = caret;
        }
        self.scroll = self.scroll.min((width + CARET_W - room).max(0.0)).max(0.0);
        let (top, bottom) = (cy - caret_h / 2.0, cy + caret_h / 2.0);
        self.rect = Some((x, top - 6.0, x + room, bottom + 6.0));
        let (fg, sel) = (&f.br.fg, &f.br.blue);
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            f.dc.PushAxisAlignedClip(
                &D2D_RECT_F { left: x, top: top - 6.0, right: x + room, bottom: bottom + 6.0 },
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED,
            );
        }
        if let (Some(t), Some((s0, s1))) = (&text, line.selection_utf16()) {
            set_opacity(sel, 0.4 * a);
            // SAFETY: as above.
            unsafe {
                f.dc.FillRectangle(&D2D_RECT_F { left: x - self.scroll + pos(t, s0), top, right: x - self.scroll + pos(t, s1), bottom }, sel);
            }
            set_opacity(sel, 1.0);
        }
        if let Some(t) = &text {
            set_opacity(fg, a);
            draw_text(f.dc, t, x - self.scroll, cy - t.h / 2.0, fg);
        }
        let cx = x + caret - self.scroll;
        let lit = blink().is_none_or(|half| (((now() - self.since) / half) as u32).is_multiple_of(2));
        if active && lit && line.selection().is_none() {
            set_opacity(fg, a);
            // SAFETY: as above.
            unsafe { f.dc.FillRectangle(&D2D_RECT_F { left: cx, top, right: cx + CARET_W, bottom }, fg) };
        }
        // SAFETY: Direct2D call on a live context; pops the clip pushed above.
        unsafe { f.dc.PopAxisAlignedClip() };
        set_opacity(fg, 1.0);
        Ok(cx)
    }
}

/// Character position (UTF-16) nearest to `x` in a laid-out line.
fn hit(t: &Text, x: f32) -> u32 {
    let (mut trailing, mut inside_, mut m) = (BOOL(0), BOOL(0), DWRITE_HIT_TEST_METRICS::default());
    // SAFETY: DirectWrite call on a live layout; the out pointers outlive the call.
    let _ = unsafe { t.layout.HitTestPoint(x, t.h / 2.0, &mut trailing, &mut inside_, &mut m) };
    m.textPosition + if trailing.as_bool() { m.length } else { 0 }
}
