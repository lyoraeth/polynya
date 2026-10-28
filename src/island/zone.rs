// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Right zone: notification chips to the right of the pill.
//!
//! Chips do not displace the island: the zone sits at its right edge and follows it.
//! A single shared stack: new and updated notifications take the first position,
//! closest to the island. The first position is held for at least `HOLD`. Notifications
//! from other sources wait invisibly in the queue, while a series from the same source
//! does not wait: the new one takes first place immediately, pushing the old one to the stack.
//! Notifications with a TTL expire automatically, but the timer pauses while the cursor
//! is over them or they are expanded, resetting to the full duration afterward. Chips that
//! do not fit up to the window edge fold into "+N"; a hidden chip's timer is paused, so it
//! appears for its full duration once there is room.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use windows::{Win32::Graphics::Direct2D::D2D1_ELLIPSE, core::Result};
use windows_numerics::Vector2;

use super::{
    Part,
    kit::{Font, Frame, Rect, Style, Text, draw_text, ease_in_out, ease_out_cubic, inside, set_opacity, tween},
    segments::HOVER_MARQUEE_DELAY,
};
use crate::icons::Icon;

/// Duration the first position is held for a notification while others wait.
const HOLD: Duration = Duration::from_secs(3);
/// Zone gap from the pill and between chips.
pub const ZONE_GAP: f32 = 0.0;
const CHIP_GAP: f32 = 6.0;
/// Chip internal padding, icon size, and gap to text.
const PAD: f32 = 0.0;
const ICON: f32 = 14.0;
const ICON_GAP: f32 = 7.0;
const TEXT_X: f32 = PAD + ICON + ICON_GAP;
/// Max chip width at rest; expands to this on hover for the marquee.
const MAX_W: f32 = 200.0;
/// Enter animation: slide from left with fade. Leave animation: short fade with shift.
const ENTER: f32 = 0.35;
const ENTER_SHIFT: f32 = 10.0;
const LEAVE: f32 = 0.25;
const LEAVE_SHIFT: f32 = 6.0;
/// "+N" stack: number of icons to show, circle size, icon size, step, gap.
const STACK: usize = 3;
const STACK_D: f32 = 18.0;
const STACK_ICON: f32 = 11.0;
const STACK_STEP: f32 = 14.0;
const STACK_GAP: f32 = 6.0;

#[derive(Debug, Clone)]
pub struct Note {
    pub key: String,
    pub source: Option<String>,
    pub icon: Icon,
    pub title: String,
    pub body: String,
    pub meta: String,
    pub ttl: Option<Duration>,
    pub animated: bool,
    pub app: Option<String>,
    pub popup: bool,
    pub swatches: Vec<[u8; 3]>,
    pub mark: Option<usize>,
}

struct Chip {
    id: u32,
    note: Note,
    shown: f32,
    leaving: bool,
    hov: f32,
    deadline: Option<Instant>,
    held: bool,
    hidden: bool,
    waiting: bool,
    marquee: bool,
}

impl Chip {
    fn part(&self) -> Part {
        Part::Note(self.id)
    }

    /// Returns opacity, horizontal shift, and space occupancy ratio.
    fn presence(&self) -> (f32, f32) {
        if self.leaving {
            let q = (1.0 - self.shown).powi(2);
            (1.0 - q, -LEAVE_SHIFT * q)
        } else {
            let e = ease_out_cubic(self.shown);
            (e, -ENTER_SHIFT * (1.0 - e))
        }
    }

    fn source(&self) -> &str {
        self.note.source.as_deref().unwrap_or(&self.note.key)
    }

    fn arm(&mut self, now: Instant) {
        self.deadline = if self.held { None } else { self.note.ttl.map(|t| now + t) };
    }
}

#[derive(Default)]
pub struct Zone {
    chips: Vec<Chip>,
    next_id: u32,
    rects: Vec<(Part, Rect)>,
    more: More,
    queue: VecDeque<u32>,
    front: Option<(u32, Instant)>,
    popups: Vec<Part>,
}

#[derive(Default)]
struct More {
    count: usize,
    shown: f32,
    hov: f32,
    marquee: bool,
}

/// Collapses multiline text into a single line separated by " · " for chips and stacks.
pub fn one_line(s: &str) -> String {
    s.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// Calculates how many chips fit in `room`. Space for the hover growth of one chip is always
/// reserved, otherwise hovering would push neighbors past the edge. If not all fit, the "+N"
/// indicator of width `more_w` needs room at the end.
fn fit(sizes: &[(f32, f32)], room: f32, more_w: f32) -> usize {
    let mut used = 0.0;
    let mut growth = 0.0f32;
    for (i, &(rest, grow)) in sizes.iter().enumerate() {
        let g = growth.max(grow);
        let tail = if i + 1 < sizes.len() { CHIP_GAP + more_w } else { 0.0 };
        if used + rest + g + tail > room {
            return i;
        }
        used += rest + CHIP_GAP;
        growth = g;
    }
    sizes.len()
}

impl Zone {
    fn new_chip(&mut self, note: Note) -> Chip {
        self.next_id += 1;
        Chip { id: self.next_id, note, shown: 0.0, leaving: false, hov: 0.0, deadline: None, held: false, hidden: false, marquee: false, waiting: false }
    }

    fn chip(&self, id: u32) -> Option<&Chip> {
        self.chips.iter().find(|c| c.id == id)
    }

    pub fn notify(&mut self, note: Note) -> Part {
        self.notify_at(note, Instant::now())
    }

    fn notify_at(&mut self, note: Note, now: Instant) -> Part {
        let id = match self.chips.iter_mut().find(|c| c.note.key == note.key) {
            Some(c) => {
                c.note = note;
                c.leaving = false;
                c.arm(now);
                c.id
            }
            None => {
                let mut c = self.new_chip(note);
                c.waiting = true;
                let id = c.id;
                self.chips.push(c);
                id
            }
        };
        self.arrive(id, now);
        Part::Note(id)
    }

    fn arrive(&mut self, id: u32, now: Instant) {
        if self.queue.contains(&id) {
            return;
        }
        let Some(source) = self.chip(id).map(|c| c.source().to_owned()) else { return };
        if let Some(q) = self.queue.iter().position(|&w| self.chip(w).is_some_and(|c| c.source() == source)) {
            let old = std::mem::replace(&mut self.queue[q], id);
            self.settle(old, now);
            return;
        }
        let front = self.front.and_then(|(f, since)| Some((self.chip(f).filter(|c| !c.leaving)?, since)));
        let Some((series, since)) = front.map(|(c, since)| (c.id == id || c.source() == source, since)) else {
            return self.promote(id, now);
        };
        let held = now < since + HOLD;
        match (series, held) {
            (false, true) => self.queue.push_back(id),
            (true, true) if !self.queue.is_empty() => {
                self.promote(id, now);
                self.front = Some((id, since));
            }
            (true, false) if !self.queue.is_empty() => self.queue.push_back(id),
            _ => self.promote(id, now),
        }
    }

    fn promote(&mut self, id: u32, now: Instant) {
        let Some(i) = self.chips.iter().position(|c| c.id == id) else { return };
        let mut c = self.chips.remove(i);
        c.waiting = false;
        c.arm(now);
        if std::mem::take(&mut c.note.popup) {
            self.popups.push(c.part());
        }
        self.chips.insert(0, c);
        self.front = Some((id, now));
    }

    pub fn take_popups(&mut self) -> Vec<Part> {
        std::mem::take(&mut self.popups)
    }

    fn settle(&mut self, id: u32, now: Instant) {
        let Some(i) = self.chips.iter().position(|c| c.id == id) else { return };
        if !self.chips[i].waiting {
            return;
        }
        let mut c = self.chips.remove(i);
        c.waiting = false;
        c.arm(now);
        let at = self.front.and_then(|(f, _)| self.chips.iter().position(|c| c.id == f)).map_or(0, |p| p + 1);
        self.chips.insert(at, c);
    }

    fn free_at(&self, now: Instant) -> Option<Instant> {
        self.queue.front()?;
        match self.front.and_then(|(f, since)| self.chip(f).filter(|c| !c.leaving).map(|_| since)) {
            Some(since) => Some(since + HOLD),
            None => Some(now),
        }
    }

    fn advance(&mut self, now: Instant) -> bool {
        let chips = &self.chips;
        self.queue.retain(|&w| chips.iter().any(|c| c.id == w && !c.leaving));
        if self.free_at(now).is_some_and(|at| at <= now)
            && let Some(next) = self.queue.pop_front()
        {
            self.promote(next, now);
            return true;
        }
        false
    }

    pub fn note(&self, part: Part) -> Option<&Note> {
        self.chips.iter().find(|c| c.part() == part).map(|c| &c.note)
    }

    pub fn dismiss_key(&mut self, key: &str) -> Option<Part> {
        let part = self.chips.iter().find(|c| !c.leaving && c.note.key == key)?.part();
        self.dismiss(part);
        Some(part)
    }

    pub fn shows(&self, anim: super::logo::Anim) -> bool {
        self.chips.iter().any(|c| c.shown > 0.0 && super::logo::anim(&c.note) == Some(anim))
    }

    pub fn dismiss(&mut self, part: Part) {
        if let Some(c) = self.chips.iter_mut().find(|c| c.part() == part) {
            c.leaving = true;
            c.deadline = None;
        }
    }

    pub fn expire(&mut self, now: Instant) -> bool {
        let mut any = false;
        for c in self.chips.iter_mut().filter(|c| c.deadline.is_some_and(|d| d <= now)) {
            c.leaving = true;
            c.deadline = None;
            any = true;
        }
        self.advance(now) || any
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.chips.iter().filter_map(|c| c.deadline).chain(self.free_at(Instant::now())).min()
    }

    pub fn step(&mut self, dt: f32, hover: Option<Part>, pointer: Option<Part>, open: Option<Part>, fade: f32) -> bool {
        let now = Instant::now();
        let mut moving = self.advance(now);
        for c in self.chips.iter_mut().filter(|c| !c.waiting) {
            let part = Some(c.part());
            let held = pointer == part || open == part || c.hidden;
            if held != c.held {
                c.held = held;
                c.arm(now);
            }
            if c.hidden {
                continue;
            }
            let target = if hover == part && open != part && !c.leaving { 1.0 } else { 0.0 };
            moving |= tween(&mut c.hov, target, dt / fade);
            moving |= if c.leaving { tween(&mut c.shown, 0.0, dt / LEAVE) } else { tween(&mut c.shown, 1.0, dt / ENTER) };
        }
        self.chips.retain(|c| !(c.leaving && (c.shown <= 0.0 || c.hidden || c.waiting)));
        let more = &mut self.more;
        let more_hover = hover == Some(Part::More) && open != Some(Part::More);
        moving |= tween(&mut more.hov, if more_hover { 1.0 } else { 0.0 }, dt / fade);
        let hidden = self.chips.iter().filter(|c| c.hidden && !c.leaving && !c.waiting).count();
        if hidden > 0 {
            more.count = hidden;
        }
        moving |= if hidden > 0 { tween(&mut more.shown, 1.0, dt / ENTER) } else { tween(&mut more.shown, 0.0, dt / LEAVE) };
        moving
    }

    pub fn wants_frames(&self) -> bool {
        let more = (self.more.hov > 0.0 && self.more.marquee)
            || self.more.shown != if self.chips.iter().any(|c| c.hidden && !c.leaving && !c.waiting) { 1.0 } else { 0.0 };
        let chips = self.chips.iter().filter(|c| !c.hidden && !c.waiting).any(|c| (c.hov > 0.0 && c.marquee) || c.shown != if c.leaving { 0.0 } else { 1.0 });
        more || chips
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<Part> {
        self.rects.iter().find(|(_, r)| inside(*r, x, y)).map(|(p, _)| *p)
    }

    pub fn anchor(&self, part: Part) -> Option<(f32, f32)> {
        let hidden = self.chips.iter().any(|c| c.hidden && c.part() == part);
        let part = if hidden { Part::More } else { part };
        self.rects.iter().find(|(p, _)| *p == part).map(|(_, (l, _, r, _))| ((l + r) / 2.0, r - l))
    }

    pub fn hidden_notes(&self, max: usize) -> Vec<(Part, Note)> {
        self.chips.iter().filter(|c| c.hidden && !c.leaving && !c.waiting).map(|c| (c.part(), c.note.clone())).take(max).collect()
    }

    /// Chips and the "+N" pile as drawn, in order: the first is the newest.
    pub fn parts(&self) -> &[(Part, Rect)] {
        &self.rects
    }

    pub fn rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.rects.iter().map(|(_, r)| *r)
    }

    pub fn draw(&mut self, f: &mut Frame, pill_r: f32, limit_r: f32, top: f32, bottom: f32, hover_since: f32) -> Result<()> {
        self.rects.clear();
        let br = f.br;
        let (fg, black) = (br.fg.clone(), br.body.clone());
        let cy = (top + bottom) / 2.0;
        let x0 = pill_r + ZONE_GAP;

        let mut layouts: Vec<(f32, f32, Option<Text>, Text)> = Vec::with_capacity(self.chips.len());
        for c in &self.chips {
            let n = &c.note;
            let title = f.txt(&one_line(&n.title), Font::Text, Style::trim(MAX_W - TEXT_X - PAD))?;
            let rest = TEXT_X + title.w + PAD;
            layouts.push((rest, rest.max(MAX_W), Some(title), f.txt(&one_line(&n.body), Font::Text, Style::Plain)?));
        }

        let probe = f.txt("+9", Font::Text, Style::Plain)?;
        let live: Vec<usize> = (0..self.chips.len()).filter(|&i| !self.chips[i].leaving && !self.chips[i].waiting).collect();
        let sizes: Vec<(f32, f32)> = live.iter().map(|&i| (layouts[i].0, layouts[i].1 - layouts[i].0)).collect();
        let fits = fit(&sizes, limit_r - x0, PAD * 2.0 + probe.w);
        for (k, &i) in live.iter().enumerate() {
            let c = &mut self.chips[i];
            let hide = k >= fits;
            if hide && !c.hidden {
                c.shown = 0.0;
                c.hov = 0.0;
            }
            c.hidden = hide;
        }

        let mut x = x0;
        for (c, (rest, hover_w, title, detail)) in self.chips.iter_mut().zip(layouts) {
            if c.hidden || c.waiting {
                continue;
            }
            let (a, dx) = c.presence();
            let w = rest + (hover_w - rest) * ease_in_out(c.hov);
            let l = x + dx;
            let rect = (l, top, l + w, bottom);
            x += (w + CHIP_GAP) * a;
            if a <= 0.0 {
                continue;
            }
            if !c.leaving {
                self.rects.push((c.part(), rect));
            }
            let (def, hov) = (a * (1.0 - c.hov), a * c.hov);
            super::logo::note_icon(f, &c.note, l + PAD + ICON / 2.0, cy, ICON, a)?;
            if def > 0.0 {
                let t = title.as_ref().unwrap();
                set_opacity(&fg, def);
                draw_text(f.dc, t, l + TEXT_X, cy - t.h / 2.0, &fg);
            }
            let text_l = l + TEXT_X;
            let room = l + w - PAD - text_l;
            c.marquee = detail.w > room;
            if hov > 0.0 {
                set_opacity(&fg, hov);
                f.marquee(&detail, text_l, l + w - PAD, cy, &fg, hov, hover_since, HOVER_MARQUEE_DELAY)?;
            }
        }

        // "+N" stack rendering.
        let m = &mut self.more;
        if m.shown > 0.0 {
            let hidden: Vec<&Note> = self.chips.iter().filter(|c| c.hidden && !c.leaving && !c.waiting).map(|c| &c.note).collect();
            let label = f.txt(&m.count.to_string(), Font::Small, Style::Tabular { case: false })?;
            let titles: Vec<String> = hidden.iter().map(|n| one_line(&n.title)).collect();
            let detail = f.txt(&titles.join(" · "), Font::Text, Style::Plain)?;
            let icons = hidden.len().min(STACK);
            let stack_w = if icons > 0 { STACK_D + STACK_STEP * (icons - 1) as f32 + STACK_GAP } else { 0.0 };
            let rest = PAD * 2.0 + stack_w + label.w;
            let w = rest + (MAX_W.max(rest) - rest) * ease_in_out(m.hov);
            let a = ease_out_cubic(m.shown);
            let l = x + -ENTER_SHIFT * (1.0 - a);
            self.rects.push((Part::More, (l, top, l + w, bottom)));
            let (def, hov) = (a * (1.0 - m.hov), a * m.hov);
            if def > 0.0 {
                let plate = br.plate.clone();
                for (i, n) in hidden.iter().take(icons).enumerate().rev() {
                    let ix = l + PAD + STACK_D / 2.0 + STACK_STEP * i as f32;
                    let depth = def * (1.0 - 0.25 * i as f32);
                    let disc = |r: f32| D2D1_ELLIPSE { point: Vector2 { X: ix, Y: cy }, radiusX: r, radiusY: r };
                    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a
                    // borrow that outlives the call.
                    unsafe {
                        set_opacity(&plate, def);
                        f.dc.FillEllipse(&disc(STACK_D / 2.0), &plate);
                    }
                    super::logo::note_icon(f, n, ix, cy, STACK_ICON, depth)?;
                }
                set_opacity(&plate, 1.0);
                let dim = br.dim.clone();
                set_opacity(&dim, def);
                draw_text(f.dc, &label, l + PAD + stack_w, cy - label.h / 2.0, &dim);
                set_opacity(&dim, 1.0);
            }
            m.marquee = detail.w > w - PAD * 2.0;
            if hov > 0.0 {
                set_opacity(&fg, hov);
                f.marquee(&detail, l + PAD, l + w - PAD, cy, &fg, hov, hover_since, HOVER_MARQUEE_DELAY)?;
            }
        }
        set_opacity(&fg, 1.0);
        set_opacity(&black, 1.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(key: &str, ttl: Option<u64>) -> Note {
        Note {
            key: key.into(),
            source: None,
            app: None,
            popup: false,
            icon: Icon::Key,
            title: key.into(),
            body: String::new(),
            meta: String::new(),
            ttl: ttl.map(Duration::from_secs),
            animated: false,
            swatches: Vec::new(),
            mark: None,
        }
    }

    fn from(key: &str, source: &str) -> Note {
        Note { source: Some(source.into()), ..note(key, None) }
    }

    fn order(z: &Zone) -> Vec<&str> {
        z.chips.iter().filter(|c| !c.waiting).map(|c| c.note.key.as_str()).collect()
    }

    #[test]
    fn newest_first_after_hold() {
        let t = Instant::now();
        let mut z = Zone::default();
        z.notify_at(note("a", None), t);
        z.notify_at(note("b", None), t + Duration::from_secs(1));
        assert_eq!(order(&z), ["a"]);
        assert!(!z.expire(t + Duration::from_secs(2)));
        assert!(z.expire(t + HOLD));
        assert_eq!(order(&z), ["b", "a"]);
        z.notify_at(note("a", None), t + HOLD * 2);
        assert_eq!(order(&z), ["a", "b"]);
    }

    #[test]
    fn each_waits_its_turn() {
        let t = Instant::now();
        let mut z = Zone::default();
        for (i, k) in ["a", "b", "c"].iter().enumerate() {
            z.notify_at(note(k, None), t + Duration::from_millis(10 * i as u64));
        }
        z.expire(t + HOLD);
        assert_eq!(order(&z), ["b", "a"]);
        assert!(!z.expire(t + HOLD + Duration::from_secs(2)));
        z.expire(t + HOLD * 2);
        assert_eq!(order(&z), ["c", "b", "a"]);
    }

    #[test]
    fn series_does_not_wait() {
        let t = Instant::now();
        let mut z = Zone::default();
        z.notify_at(from("m1", "chat"), t);
        z.notify_at(from("m2", "chat"), t + Duration::from_millis(100));
        assert_eq!(order(&z), ["m2", "m1"]);
    }

    #[test]
    fn waiting_series_keeps_only_latest() {
        let t = Instant::now();
        let mut z = Zone::default();
        z.notify_at(note("front", None), t);
        z.notify_at(from("m1", "chat"), t);
        z.notify_at(from("m2", "chat"), t);
        assert_eq!(order(&z), ["front", "m1"]);
        z.expire(t + HOLD);
        assert_eq!(order(&z), ["m2", "front", "m1"]);
    }

    #[test]
    fn frequent_updates_do_not_starve_queue() {
        let t = Instant::now();
        let mut z = Zone::default();
        z.notify_at(note("agent", None), t);
        z.notify_at(note("chat", None), t + Duration::from_millis(100));
        for i in 1..=8 {
            z.notify_at(note("agent", None), t + Duration::from_millis(500 * i));
            z.expire(t + Duration::from_millis(500 * i));
        }
        assert_eq!(order(&z)[0], "chat");
    }

    #[test]
    fn popup_when_front() {
        let t = Instant::now();
        let mut z = Zone::default();
        z.notify_at(note("a", None), t);
        let b = z.notify_at(Note { popup: true, ..note("b", None) }, t);
        assert!(z.take_popups().is_empty());
        z.expire(t + HOLD);
        assert_eq!(z.take_popups(), [b]);
        z.notify_at(note("b", None), t + HOLD * 2);
        assert!(z.take_popups().is_empty());
    }

    #[test]
    fn dismissed_front_frees_place() {
        let t = Instant::now();
        let mut z = Zone::default();
        let a = z.notify_at(note("a", None), t);
        z.notify_at(note("b", None), t);
        z.dismiss(a);
        assert!(z.next_deadline().is_some_and(|d| d <= Instant::now()));
        z.step(0.01, None, None, None, 0.18);
        assert_eq!(order(&z)[0], "b");
    }

    #[test]
    fn same_key_replaces_in_place() {
        let mut z = Zone::default();
        let a = z.notify(note("caps", Some(3)));
        z.notify(note("num", Some(3)));
        assert_eq!(z.notify(note("caps", Some(3))), a);
        assert_eq!(z.chips.len(), 2);
    }

    #[test]
    fn multiline_joins_into_one() {
        assert_eq!(
            one_line(
                "Grade: #senior

  Salary: not specified 
Format"
            ),
            "Grade: #senior · Salary: not specified · Format"
        );
        assert_eq!(one_line("one"), "one");
    }

    #[test]
    fn fit_reserves_hover_and_more() {
        assert_eq!(fit(&[(100.0, 100.0), (60.0, 140.0)], 400.0, 30.0), 2);
        assert_eq!(fit(&[(100.0, 100.0), (100.0, 100.0), (100.0, 100.0)], 330.0, 30.0), 1);
        assert_eq!(fit(&[(100.0, 100.0)], 50.0, 30.0), 0);
    }

    #[test]
    fn hold_stops_timer_and_rearms() {
        let mut z = Zone::default();
        let p = z.notify(note("caps", Some(3)));
        assert!(z.next_deadline().is_some());
        z.step(0.01, None, Some(p), None, 0.18);
        assert!(z.next_deadline().is_none());
        z.step(0.01, None, None, None, 0.18);
        assert!(z.next_deadline().is_some());
        assert!(!z.expire(Instant::now()));
        assert!(z.expire(Instant::now() + Duration::from_secs(4)));
    }
}
