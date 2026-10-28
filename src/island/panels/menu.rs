// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent module dropdown: renders `proto::Screen` data.

use proto::{Row, Screen};
use windows::{
    Win32::{
        Graphics::Direct2D::{Common::D2D_RECT_F, D2D1_ROUNDED_RECT, ID2D1SolidColorBrush},
        UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_UP},
    },
    core::Result,
};

use super::{Area, PanelView, Reveal};
use crate::{
    edit::{Edited, LineEdit},
    icons,
    island::{
        field::Field,
        kit::{Font, Frame, Glide, HotKind, Hotspot, Style, draw_text, ease_out_cubic, inside, set_opacity},
        segments::Data,
    },
};

const W: f32 = 300.0;
/// A page of search takes the launcher's width, so the dropdown does not jump when it replaces it.
const PAGE_W: f32 = 480.0;
/// A module dropdown under the search block: room for converted values and long hints.
const WIDE_W: f32 = 400.0;
const PAD_X: f32 = 20.0;
const PAD_T: f32 = 6.0;
const PAD_B: f32 = 12.0;
const ROW_INSET: f32 = 8.0;
const ITEM_H: f32 = 34.0;
const DIVIDER_H: f32 = 13.0;
const HEADER_H: f32 = 58.0;
const PAIR_H: f32 = 28.0;
const TITLE_H: f32 = 40.0;
const ART_PAD: f32 = 10.0;
const ART_LINE: f32 = 13.0;
const CONFIRM_SECS: f32 = 3.0;
const CONFIRM_BAR_H: f32 = 2.0;
const SLIDE_DUR: f32 = 0.22;
const SLIDE_DX: f32 = 24.0;
const PAIR_VALUE_SHARE: f32 = 0.66;
const MARQUEE_DELAY: f32 = 1.0;
const SWITCH_W: f32 = 36.0;
const SWITCH_H: f32 = 20.0;
const KNOB_INSET: f32 = 3.5;
const SWITCH_DUR: f32 = 0.24;
const DROP_STRETCH: f32 = 0.85;
const DROP_SQUASH: f32 = 0.18;
const SWITCH_OFF: f32 = 0.10;
const SECTION_H: f32 = 30.0;
const GAP_H: f32 = 12.0;
const TEXT_LINE: f32 = 17.0;
const TEXT_PAD: f32 = 8.0;
const BUTTON_FILL: f32 = 0.09;
const BUTTON_FILL_LIT: f32 = 0.16;
const BUTTON_H: f32 = 46.0;
const BUTTON_PILL: f32 = 34.0;
const CARET_H: f32 = 16.0;
const BACK: &str = "m15 18-6-6 6-6";
/// Where the keys column starts, as a share of the width: the same spot as in search.
const KEYS_AT: f32 = 0.56;
/// Input field: inset from the row and the text padding when the pill is narrow.
const FIELD_INSET: f32 = 3.0;
const FIELD_PAD: f32 = 10.0;

#[derive(Debug, PartialEq)]
pub enum MenuEffect {
    None,
    Redraw,
    Close,
    Select(String),
    /// A row bound to a module function: run it like from search, with `source: menu`.
    Run(String),
    Submit(String, String),
    Leave,
}

/// An input row being edited.
struct Edit {
    row: usize,
    line: LineEdit,
}

#[derive(Default)]
pub struct MenuPanel {
    pub module: String,
    stack: Vec<Screen>,
    sel: Option<usize>,
    confirm: Option<(usize, f32)>,
    slide: f32,
    dir: f32,
    now: f32,
    last_t: Option<f32>,
    glide: Glide,
    last_mouse: Option<(f32, f32)>,
    shown_at: f32,
    sel_seen: Option<usize>,
    sel_at: f32,
    edit: Option<Edit>,
    /// The edited input row as a field: scrolling, selection, the mouse, blinking.
    field: Field,
    pub page: bool,
    /// Wider than a taskbar menu: the module opened under the search block.
    pub wide: bool,
    switches: std::collections::HashMap<String, f32>,
}

fn selectable(r: &Row) -> bool {
    matches!(r, Row::Item { .. } | Row::Toggle { .. } | Row::Input { .. } | Row::Button { .. })
}

/// Id of a row the user can act on.
fn row_id(r: &Row) -> Option<&str> {
    match r {
        Row::Item { id, .. } | Row::Toggle { id, .. } | Row::Input { id, .. } | Row::Button { id, .. } => Some(id),
        _ => None,
    }
}

fn row_h(r: &Row) -> f32 {
    match r {
        Row::Item { .. } | Row::Toggle { .. } | Row::Input { .. } => ITEM_H,
        Row::Button { .. } => BUTTON_H,
        Row::Section { title } if title.is_empty() => GAP_H,
        Row::Section { .. } => SECTION_H,
        Row::Text { text } => TEXT_PAD * 2.0 + TEXT_LINE * text.lines().count() as f32,
        Row::Divider => DIVIDER_H,
        Row::Header { .. } => HEADER_H,
        Row::Pair { .. } => PAIR_H,
        Row::Art { lines, .. } => ART_PAD * 2.0 + ART_LINE * lines.len() as f32,
    }
}

/// Longest text of a one-line row.
const MAX_ROW_CHARS: usize = 1000;

/// Rows are one line tall: a module's multi-line value (the clipboard, a log excerpt) would be
/// drawn past the dropdown. Line breaks become " · " everywhere except `text` and `art`, which
/// are multi-line by design, and very long text is cut.
fn flatten(screen: &mut Screen) {
    use crate::island::zone::one_line;
    let fix = |s: &mut String| {
        if s.contains('\n') {
            *s = one_line(s);
        }
        // a whole log in the clipboard: a row cannot show it, and laying it out is costly
        if s.chars().count() > MAX_ROW_CHARS {
            *s = s.chars().take(MAX_ROW_CHARS).collect::<String>() + "…";
        }
    };
    if let Some(t) = screen.title.as_mut() {
        fix(t);
    }
    for row in &mut screen.rows {
        match row {
            Row::Item { text, hint, .. } => {
                fix(text);
                fix(hint);
            }
            Row::Toggle { text, .. } | Row::Button { text, .. } => fix(text),
            Row::Input { text, placeholder, .. } => {
                fix(text);
                fix(placeholder);
            }
            Row::Section { title } => fix(title),
            Row::Header { title, subtitle } => {
                fix(title);
                fix(subtitle);
            }
            Row::Pair { label, value } => {
                fix(label);
                fix(value);
            }
            Row::Text { .. } | Row::Art { .. } | Row::Divider => {}
        }
    }
}

impl MenuPanel {
    #[allow(clippy::too_many_arguments)]
    fn label(&self, f: &mut Frame, i: usize, text: &str, l: f32, r: f32, cy: f32, brush: &ID2D1SolidColorBrush, a: f32) -> Result<()> {
        f.cut_if_long((l, cy - ITEM_H / 2.0, r, cy + ITEM_H / 2.0), text, Font::Text, r - l)?;
        if self.sel == Some(i) {
            let t = f.txt(text, Font::Text, Style::Plain)?;
            return f.marquee(&t, l, r, cy, brush, a, self.sel_at, MARQUEE_DELAY);
        }
        let t = f.txt(text, Font::Text, Style::trim(r - l))?;
        draw_text(f.dc, &t, l, cy - t.h / 2.0, brush);
        Ok(())
    }

    fn screen(&self) -> Option<&Screen> {
        self.stack.last()
    }

    fn first_item(&self) -> Option<usize> {
        self.screen()?.rows.iter().position(selectable)
    }

    pub fn show(&mut self, module: String, mut screen: Screen) {
        flatten(&mut screen);
        self.page = false;
        self.wide = false;
        self.module = module;
        self.stack = vec![screen];
        self.sel = self.first_item();
        self.confirm = None;
        self.edit = None;
        self.slide = 0.0;
        self.shown_at = self.now;
    }

    /// Replaces the current screen in place. The selection and an input being edited follow their
    /// row by id, so a module can redraw the screen while the user works in it.
    pub fn replace(&mut self, mut screen: Screen) {
        flatten(&mut screen);
        let id_at = |s: Option<&Screen>, i: usize| s.and_then(|s| s.rows.get(i)).and_then(row_id).map(String::from);
        let sel_id = self.sel.and_then(|i| id_at(self.screen(), i));
        let edit_id = self.edit.as_ref().and_then(|e| id_at(self.screen(), e.row));
        if let Some(top) = self.stack.last_mut() {
            *top = screen;
        }
        let find = |id: &Option<String>| id.as_ref().and_then(|id| self.screen()?.rows.iter().position(|r| row_id(r) == Some(id.as_str())));
        let (sel_at, edit_at) = (find(&sel_id), find(&edit_id));
        let ok = |i: usize| self.screen().is_some_and(|s| s.rows.get(i).is_some_and(selectable));
        let sel = sel_at.or(self.sel).filter(|&i| ok(i)).or_else(|| self.first_item());
        let confirm_gone = self.confirm.is_some_and(|(i, _)| !ok(i));
        self.sel = sel;
        if confirm_gone {
            self.confirm = None;
        }
        match (edit_at, self.screen().and_then(|s| edit_at.and_then(|i| s.rows.get(i)))) {
            (Some(i), Some(Row::Input { .. })) => {
                if let Some(e) = &mut self.edit {
                    e.row = i;
                }
            }
            _ => self.edit = None,
        }
    }

    pub fn replace_root(&mut self, mut screen: Screen) {
        flatten(&mut screen);
        if self.stack.len() > 1 {
            self.stack[0] = screen;
        }
    }

    pub fn title(&self) -> Option<&str> {
        self.screen().and_then(|s| s.title.as_deref())
    }

    /// The screen on top and its selected row: what a screen reader reads.
    pub fn current(&self) -> Option<(&Screen, Option<usize>)> {
        self.screen().map(|s| (s, self.sel))
    }

    /// The text being typed in an input row, if any.
    pub fn editing_text(&self) -> Option<(usize, &str)> {
        self.edit.as_ref().map(|e| (e.row, e.line.text.as_str()))
    }

    pub fn editing(&self) -> bool {
        self.edit.is_some()
    }

    pub fn char(&mut self, c: char) -> MenuEffect {
        let Some(e) = &mut self.edit else { return MenuEffect::None };
        if e.line.char(c) == Edited::No { MenuEffect::None } else { MenuEffect::Redraw }
    }

    fn edit_key(&mut self, vk: VIRTUAL_KEY, ctrl: bool, shift: bool) -> MenuEffect {
        let Some(e) = &mut self.edit else { return MenuEffect::None };
        match vk {
            VK_RETURN => {
                let e = self.edit.take().unwrap();
                let Some(Row::Input { id, .. }) = self.screen().and_then(|s| s.rows.get(e.row)) else { return MenuEffect::Redraw };
                MenuEffect::Submit(id.clone(), e.line.text.trim().to_string())
            }
            VK_ESCAPE => {
                self.edit = None;
                MenuEffect::Redraw
            }
            _ if e.line.key(vk, ctrl, shift) == Edited::No => MenuEffect::None,
            _ => MenuEffect::Redraw,
        }
    }

    /// The mouse went down on row `i`: an input row starts editing with the caret under the cursor.
    /// `true` if it was an input row, so the click is not handled again on release.
    pub fn press(&mut self, i: usize, x: f32, shift: bool) -> bool {
        let Some(Row::Input { text, .. }) = self.screen().and_then(|s| s.rows.get(i)).cloned() else { return false };
        if self.edit.as_ref().is_none_or(|e| e.row != i) {
            self.sel = Some(i);
            self.edit = Some(Edit { row: i, line: LineEdit::new(&text) });
            self.field = Field::default();
        }
        self.field.press(x, shift);
        true
    }

    /// The mouse dragged with the button down after a press in the edited row.
    pub fn drag(&mut self, x: f32) {
        if self.edit.is_some() {
            self.field.drag(x);
        }
    }

    pub fn is_input(&self, i: usize) -> bool {
        matches!(self.screen().and_then(|s| s.rows.get(i)), Some(Row::Input { .. }))
    }

    /// Seconds until the caret of the edited row blinks next.
    pub fn next_blink(&self) -> Option<f32> {
        self.edit.as_ref().and(self.field.next_blink())
    }

    pub fn push(&mut self, mut screen: Screen) {
        flatten(&mut screen);
        self.stack.push(screen);
        self.go(1.0);
    }

    fn go(&mut self, dir: f32) {
        self.sel = self.first_item();
        self.confirm = None;
        self.edit = None;
        self.slide = 1.0;
        self.dir = dir;
        self.shown_at = self.now;
    }

    pub fn back(&mut self) -> MenuEffect {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.go(-1.0);
            MenuEffect::Redraw
        } else if self.page {
            MenuEffect::Leave
        } else {
            MenuEffect::Close
        }
    }

    pub fn activate(&mut self, i: usize) -> MenuEffect {
        let row = self.screen().and_then(|s| s.rows.get(i)).cloned();
        let (id, confirm, command) = match row {
            Some(Row::Item { id, confirm, command, .. }) => (id, confirm, command),
            Some(Row::Toggle { id, .. }) | Some(Row::Button { id, .. }) => (id, None, String::new()),
            Some(Row::Input { text, .. }) => {
                self.sel = Some(i);
                self.edit = Some(Edit { row: i, line: LineEdit::new(&text) });
                return MenuEffect::Redraw;
            }
            _ => return MenuEffect::None,
        };
        self.edit = None;
        self.sel = Some(i);
        if confirm.is_some() && self.confirm.map(|c| c.0) != Some(i) {
            self.confirm = Some((i, self.now));
            return MenuEffect::Redraw;
        }
        self.confirm = None;
        if command.is_empty() { MenuEffect::Select(id) } else { MenuEffect::Run(command) }
    }

    pub fn key(&mut self, vk: VIRTUAL_KEY, ctrl: bool, shift: bool) -> MenuEffect {
        if self.edit.is_some() && !matches!(vk, VK_UP | VK_DOWN) {
            return self.edit_key(vk, ctrl, shift);
        }
        self.edit = None;
        let items: Vec<usize> = self.screen().map(|s| s.rows.iter().enumerate().filter(|(_, r)| selectable(r)).map(|(i, _)| i).collect()).unwrap_or_default();
        let at = self.sel.and_then(|s| items.iter().position(|&i| i == s));
        match vk {
            VK_UP | VK_DOWN if !items.is_empty() => {
                let n = items.len();
                let next = match (at, vk) {
                    (Some(p), VK_UP) => (p + n - 1) % n,
                    (Some(p), _) => (p + 1) % n,
                    (None, _) => 0,
                };
                self.sel = Some(items[next]);
                self.confirm = None;
                MenuEffect::Redraw
            }
            VK_RETURN => self.sel.map_or(MenuEffect::None, |i| self.activate(i)),
            VK_ESCAPE | VK_LEFT => self.back(),
            _ => MenuEffect::None,
        }
    }

    pub fn moving(&self) -> bool {
        let switching = self.screen().is_some_and(|s| {
            s.rows.iter().any(|r| matches!(r, Row::Toggle { id, on, .. } if self.switches.get(id).is_some_and(|p| *p != if *on { 1.0 } else { 0.0 })))
        });
        self.slide > 0.0 || self.confirm.is_some() || self.glide.moving() || switching
    }

    fn has_title(&self) -> bool {
        self.screen().is_some_and(|s| s.title.is_some()) || self.can_back()
    }

    fn can_back(&self) -> bool {
        self.stack.len() > 1 || self.page
    }
}

impl PanelView for MenuPanel {
    fn size(&self) -> (f32, f32) {
        let rows: f32 = self.screen().map_or(0.0, |s| s.rows.iter().map(row_h).sum());
        let w = if self.page {
            PAGE_W
        } else if self.wide {
            WIDE_W
        } else {
            W
        };
        (w, PAD_T + if self.has_title() { TITLE_H } else { 0.0 } + rows + PAD_B)
    }

    fn draw(&mut self, f: &mut Frame, _data: &Data, area: Area, reveal: Reveal) -> Result<()> {
        let dt = self.last_t.map_or(0.0, |t| (f.t - t).clamp(0.0, 0.1));
        self.last_t = Some(f.t);
        self.now = f.t;
        self.slide = (self.slide - dt / SLIDE_DUR).max(0.0);
        if self.confirm.is_some_and(|(_, t0)| f.t - t0 > CONFIRM_SECS) {
            self.confirm = None;
        }
        let Some(screen) = self.screen().cloned() else { return Ok(()) };
        let since = self.shown_at;
        let br = f.br;
        let (fg, dim, plate) = (br.fg.clone(), br.dim.clone(), br.select.clone());
        let p = ease_out_cubic(1.0 - self.slide);
        let ox = self.dir * SLIDE_DX * (1.0 - p);
        let (l, r) = (area.left() + ox, area.right() + ox);
        let mut y = area.top + PAD_T;

        if self.has_title() {
            let (a, rise) = reveal.row(0);
            let a = a * p;
            let cy = y + TITLE_H / 2.0 + rise;
            let mut tx = l + PAD_X;
            if self.can_back() {
                let rect = (l + ROW_INSET, y, l + PAD_X + 24.0, y + TITLE_H);
                let hover = f.mouse.is_some_and(|(mx, my)| inside(rect, mx, my));
                let b = if hover { &fg } else { &dim };
                set_opacity(b, a);
                icons::draw_path(f.gfx, f.dc, BACK, tx + 6.0, cy, 18.0, b)?;
                if a > 0.5 {
                    f.hotspots.push(Hotspot { rect, kind: HotKind::MenuBack });
                }
                tx += 22.0;
            }
            if let Some(title) = &screen.title {
                let t = f.txt(title, Font::Bold, Style::trim(r - PAD_X - tx))?;
                set_opacity(&fg, a);
                draw_text(f.dc, &t, tx, cy - t.h / 2.0, &fg);
            }
            y += TITLE_H;
        }

        let hovered = screen.rows.iter().scan(y, |top, row| {
            let t = *top;
            *top += row_h(row);
            Some(t)
        });
        let tops: Vec<f32> = hovered.collect();
        let hover =
            screen.rows.iter().enumerate().find(|(i, row)| {
                selectable(row) && f.mouse.is_some_and(|(mx, my)| inside((l + ROW_INSET, tops[*i], r - ROW_INSET, tops[*i] + ITEM_H), mx, my))
            });
        // Mouse moved over item: transfer selection and keep it.
        let moved = f.mouse != self.last_mouse;
        self.last_mouse = f.mouse;
        if let Some((i, _)) = hover.filter(|_| moved)
            && self.sel != Some(i)
        {
            self.sel = Some(i);
            self.confirm = self.confirm.filter(|c| c.0 == i);
        }
        if self.sel != self.sel_seen {
            self.sel_seen = self.sel;
            self.sel_at = f.t;
        }
        // A filled button and an input field show their own state instead of the selection plate.
        let lit = self.sel.filter(|&i| !matches!(screen.rows.get(i), Some(Row::Button { plain: false, .. } | Row::Input { .. })));
        let target = lit.map(|i| {
            let top = tops[i] + reveal.row(i + 1).1;
            match screen.rows.get(i) {
                Some(Row::Button { .. }) => {
                    let cy = top + BUTTON_H / 2.0;
                    (l + ROW_INSET, cy - BUTTON_PILL / 2.0, r - ROW_INSET, cy + BUTTON_PILL / 2.0)
                }
                _ => (l + ROW_INSET, top, r - ROW_INSET, top + ITEM_H),
            }
        });
        self.glide.step(target, f.t);
        if let Some(i) = lit {
            self.glide.draw(f.dc, &plate, reveal.row(i + 1).0 * p, 12.0);
        }

        for (i, row) in screen.rows.iter().enumerate() {
            let (a, rise) = reveal.row(i + 1);
            let a = a * p;
            let top = tops[i] + rise;
            let h = row_h(row);
            match row {
                Row::Item { text, hint, keys, confirm, .. } => {
                    let tx = l + PAD_X;
                    let rect = (l + ROW_INSET, top, r - ROW_INSET, top + h);
                    if a > 0.5 {
                        f.hotspots.push(Hotspot { rect, kind: HotKind::MenuRow(i) });
                    }
                    let cy = top + h / 2.0;
                    let waiting = self.confirm.filter(|c| c.0 == i).map(|(_, t0)| 1.0 - (f.t - t0) / CONFIRM_SECS);
                    let label = match (waiting, confirm) {
                        (Some(_), Some(c)) => c.as_str(),
                        _ => text.as_str(),
                    };
                    // Keys: a column a bit right of the middle when there is a hint, else at the right edge.
                    let keys_t = (!keys.is_empty() && waiting.is_none()).then(|| f.txt(keys, Font::Tiny, Style::Plain)).transpose()?;
                    let kx = keys_t.as_ref().map(|k| if hint.is_empty() { r - PAD_X - k.w } else { l + (r - l) * KEYS_AT });
                    let hint_max = match (&keys_t, kx) {
                        (Some(k), Some(kx)) => r - PAD_X - kx - k.w - 16.0,
                        _ => (r - l) * 0.4,
                    };
                    let hint_t = (!hint.is_empty() && waiting.is_none()).then(|| f.txt(hint, Font::Small, Style::trim(hint_max.max(0.0)))).transpose()?;
                    let room = match kx {
                        Some(kx) => kx - 12.0 - tx,
                        None => r - PAD_X - hint_t.as_ref().map_or(0.0, |t| t.w + 16.0) - tx,
                    };
                    if let Some(ht) = &hint_t {
                        f.cut_if_long((r - PAD_X - ht.w, top, r - PAD_X, top + h), hint, Font::Small, hint_max.max(0.0))?;
                    }
                    if let (Some(k), Some(kx)) = (&keys_t, kx) {
                        set_opacity(&dim, a);
                        draw_text(f.dc, k, kx, cy - k.h / 2.0, &dim);
                    }
                    set_opacity(&fg, a);
                    self.label(f, i, label, tx, tx + room, cy, &fg, a)?;
                    if let Some(ht) = &hint_t {
                        set_opacity(&dim, a);
                        draw_text(f.dc, ht, r - PAD_X - ht.w, cy - ht.h / 2.0, &dim);
                    }
                    if let Some(left) = waiting {
                        let (bl, br_) = (l + PAD_X, r - PAD_X);
                        let by = top + h - 7.0;
                        set_opacity(&fg, a * 0.7);
                        // SAFETY: Direct2D calls on live COM objects; every argument is a value or
                        // a borrow that outlives the call.
                        unsafe {
                            f.dc.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT {
                                    rect: D2D_RECT_F { left: bl, top: by, right: bl + (br_ - bl) * left.clamp(0.0, 1.0), bottom: by + CONFIRM_BAR_H },
                                    radiusX: 1.0,
                                    radiusY: 1.0,
                                },
                                &fg,
                            );
                        }
                    }
                }
                Row::Toggle { id, text, on } => {
                    let rect = (l + ROW_INSET, top, r - ROW_INSET, top + h);
                    if a > 0.5 {
                        f.hotspots.push(Hotspot { rect, kind: HotKind::MenuRow(i) });
                    }
                    let cy = top + h / 2.0;
                    set_opacity(&fg, a);
                    self.label(f, i, text, l + PAD_X, r - PAD_X - SWITCH_W - 12.0, cy, &fg, a)?;
                    // "Drop" animation: track and knob stretch/squash during transition.
                    let target = if *on { 1.0 } else { 0.0 };
                    let pos = self.switches.entry(id.clone()).or_insert(target);
                    crate::island::kit::tween(pos, target, dt / SWITCH_DUR);
                    let t = crate::island::kit::ease_in_out(*pos);
                    let (sl, sr) = (r - PAD_X - SWITCH_W, r - PAD_X);
                    let track_rr = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: sl, top: cy - SWITCH_H / 2.0, right: sr, bottom: cy + SWITCH_H / 2.0 },
                        radiusX: SWITCH_H / 2.0,
                        radiusY: SWITCH_H / 2.0,
                    };
                    let d = SWITCH_H - KNOB_INSET * 2.0;
                    let travel = SWITCH_W - SWITCH_H;
                    let mid = (t * std::f32::consts::PI).sin();
                    let len = d + travel * DROP_STRETCH * mid;
                    let kh = d * (1.0 - DROP_SQUASH * mid);
                    let centre = sl + SWITCH_H / 2.0 + travel * t;
                    let (kl, kr) = ((centre - len / 2.0).max(sl + KNOB_INSET), (centre + len / 2.0).min(sr - KNOB_INSET));
                    let knob = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: kl, top: cy - kh / 2.0, right: kr, bottom: cy + kh / 2.0 },
                        radiusX: kh / 2.0,
                        radiusY: kh / 2.0,
                    };
                    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a
                    // borrow that outlives the call.
                    unsafe {
                        set_opacity(&fg, a * SWITCH_OFF * (1.0 - t));
                        f.dc.FillRoundedRectangle(&track_rr, &fg);
                        let vivid = br.vivid.clone();
                        set_opacity(&vivid, a * t);
                        f.dc.FillRoundedRectangle(&track_rr, &vivid);
                        set_opacity(&vivid, 1.0);
                        set_opacity(&dim, a * (1.0 - t));
                        f.dc.FillRoundedRectangle(&knob, &dim);
                        set_opacity(&fg, a * t);
                        f.dc.FillRoundedRectangle(&knob, &fg);
                    }
                    set_opacity(&fg, 1.0);
                    set_opacity(&dim, 1.0);
                }
                Row::Input { text, placeholder, .. } => {
                    let rect = (l + ROW_INSET, top, r - ROW_INSET, top + h);
                    if a > 0.5 {
                        f.hotspots.push(Hotspot { rect, kind: HotKind::MenuRow(i) });
                    }
                    let cy = top + h / 2.0;
                    let editing = self.edit.as_ref().is_some_and(|e| e.row == i);
                    // A pill like the time and search plates, outlined like the dropdown and the
                    // taskbar edge. Hovered or selected it takes the accent, and keeps it while typing.
                    let (fl, fr) = (l + ROW_INSET + FIELD_INSET, r - ROW_INSET - FIELD_INSET);
                    let (ft, fb) = (top + FIELD_INSET, top + h - FIELD_INSET);
                    let radius = (fb - ft) / 2.0;
                    let well = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: fl, top: ft, right: fr, bottom: fb }, radiusX: radius, radiusY: radius };
                    let fill = if editing || self.sel == Some(i) { &f.br.plate } else { &f.br.surface };
                    set_opacity(fill, a);
                    set_opacity(&f.br.edge, a);
                    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a
                    // borrow that outlives the call.
                    unsafe {
                        f.dc.FillRoundedRectangle(&well, fill);
                        f.dc.DrawRoundedRectangle(&well, &f.br.edge, 1.0, None);
                    }
                    set_opacity(fill, 1.0);
                    set_opacity(&f.br.edge, 1.0);
                    let tx = fl + radius.max(FIELD_PAD);
                    // While typing, the right end says how to finish.
                    let enter = editing.then(|| f.txt("Enter", Font::Tiny, Style::Plain)).transpose()?;
                    let end = fr - radius.max(FIELD_PAD) - enter.as_ref().map_or(0.0, |t| t.w + 8.0);
                    if let Some(t) = &enter {
                        set_opacity(&dim, a);
                        draw_text(f.dc, t, fr - radius.max(FIELD_PAD) - t.w, cy - t.h / 2.0, &dim);
                    }
                    let value = self.edit.as_ref().filter(|e| e.row == i).map_or(text.as_str(), |e| e.line.text.as_str()).to_string();
                    if value.is_empty() {
                        let t = f.txt(placeholder, Font::Text, Style::trim(end - tx))?;
                        set_opacity(&dim, a);
                        draw_text(f.dc, &t, tx, cy - t.h / 2.0, &dim);
                    }
                    match self.edit.as_mut().filter(|e| e.row == i) {
                        Some(e) => {
                            self.field.draw(f, &mut e.line, Font::Text, tx, end - tx, cy, CARET_H, true, a)?;
                        }
                        None if !value.is_empty() => {
                            f.cut_if_long((tx, ft, end, fb), &value, Font::Text, end - tx)?;
                            let t = f.txt(&value, Font::Text, Style::trim(end - tx))?;
                            set_opacity(&fg, a);
                            draw_text(f.dc, &t, tx, cy - t.h / 2.0, &fg);
                        }
                        None => {}
                    }
                    set_opacity(&fg, 1.0);
                }
                Row::Text { text } => {
                    set_opacity(&dim, a);
                    for (k, line) in text.lines().enumerate() {
                        let t = f.txt(line, Font::Small, Style::trim(r - l - PAD_X * 2.0))?;
                        draw_text(f.dc, &t, l + PAD_X, top + TEXT_PAD + TEXT_LINE * k as f32 + (TEXT_LINE - t.h) / 2.0, &dim);
                    }
                }
                Row::Button { text, plain, .. } => {
                    let cy = top + h / 2.0;
                    let rect = (l + ROW_INSET, cy - BUTTON_PILL / 2.0, r - ROW_INSET, cy + BUTTON_PILL / 2.0);
                    if a > 0.5 {
                        f.hotspots.push(Hotspot { rect, kind: HotKind::MenuRow(i) });
                    }
                    let lit = self.sel == Some(i);
                    let pill = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: rect.0, top: rect.1, right: rect.2, bottom: rect.3 },
                        radiusX: BUTTON_PILL / 2.0,
                        radiusY: BUTTON_PILL / 2.0,
                    };
                    if !*plain {
                        set_opacity(&fg, a * if lit { BUTTON_FILL_LIT } else { BUTTON_FILL });
                        // SAFETY: Direct2D calls on live COM objects; every argument is a value or
                        // a borrow that outlives the call.
                        unsafe { f.dc.FillRoundedRectangle(&pill, &fg) };
                    }
                    let t = f.txt(text, Font::Bold, Style::trim(r - l - PAD_X * 2.0))?;
                    set_opacity(&fg, a);
                    draw_text(f.dc, &t, (l + r) / 2.0 - t.w / 2.0, cy - t.h / 2.0, &fg);
                }
                Row::Divider => {
                    let shade = f.shade(a)?;
                    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a
                    // borrow that outlives the call.
                    unsafe {
                        f.dc.FillRectangle(&D2D_RECT_F { left: l + PAD_X, top: top + h / 2.0, right: r - PAD_X, bottom: top + h / 2.0 + 1.0 }, &shade);
                    }
                }
                Row::Section { title } => {
                    let t = f.txt(title, Font::Small, Style::trim(r - l - PAD_X * 2.0))?;
                    set_opacity(&dim, a);
                    draw_text(f.dc, &t, l + PAD_X, top + h - t.h - 4.0, &dim);
                }
                Row::Header { title, subtitle } => {
                    let room = r - l - PAD_X * 2.0;
                    let t = f.txt(title, Font::Input, Style::trim(room))?;
                    let s = f.txt(subtitle, Font::Small, Style::Plain)?;
                    let ty = top + (h - t.h - 2.0 - s.h) / 2.0;
                    set_opacity(&fg, a);
                    draw_text(f.dc, &t, l + PAD_X, ty, &fg);
                    set_opacity(&dim, a);
                    f.marquee(&s, l + PAD_X, r - PAD_X, ty + t.h + 2.0 + s.h / 2.0, &dim, a, since, MARQUEE_DELAY)?;
                }
                Row::Art { lines, colors } => {
                    let cw = f.txt("M", Font::Mono, Style::Plain)?.w;
                    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * cw;
                    let x0 = ((l + r) / 2.0 - width / 2.0).round();
                    // Own brush: the shared text brush must not be recolored.
                    // SAFETY: `f.dc` is a valid device context.
                    let brush = unsafe { f.dc.CreateSolidColorBrush(&crate::island::kit::rgb(0xff, 0xff, 0xff), None)? };
                    for (i, line) in lines.iter().enumerate() {
                        let y = top + ART_PAD + ART_LINE * i as f32;
                        for (j, ch) in line.chars().enumerate() {
                            if ch == ' ' {
                                continue;
                            }
                            let color = colors.get(i).and_then(|c| c.get(j)).and_then(|c| proto::parse_hex(c));
                            // SAFETY: Direct2D call on a live brush.
                            let mut c = color.map_or(unsafe { f.br.fg.GetColor() }, |[r, g, b]| crate::island::kit::rgb(r, g, b));
                            c.a = a;
                            // SAFETY: Direct2D calls on live COM objects; every argument is a value
                            // or a borrow that outlives the call.
                            unsafe { brush.SetColor(&c) };
                            let t = f.txt(&ch.to_string(), Font::Mono, Style::Plain)?;
                            draw_text(f.dc, &t, x0 + cw * j as f32, y, &brush);
                        }
                    }
                }
                Row::Pair { label, value } => {
                    let cy = top + h / 2.0;
                    let max_w = (r - l - PAD_X * 2.0) * PAIR_VALUE_SHARE;
                    let v = f.txt(value, Font::Text, Style::Plain)?;
                    let vw = v.w.min(max_w);
                    f.cut_if_long((r - PAD_X - vw, top, r - PAD_X, top + h), value, Font::Text, max_w)?;
                    let k = f.txt(label, Font::Small, Style::trim(r - l - PAD_X * 2.0 - vw - 12.0))?;
                    set_opacity(&dim, a);
                    draw_text(f.dc, &k, l + PAD_X, cy - k.h / 2.0, &dim);
                    set_opacity(&fg, a);
                    f.marquee(&v, r - PAD_X - vw, r - PAD_X, cy, &fg, a, since, MARQUEE_DELAY)?;
                }
            }
        }
        set_opacity(&fg, 1.0);
        set_opacity(&dim, 1.0);
        Ok(())
    }
}
