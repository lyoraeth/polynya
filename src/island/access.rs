// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The island in words: every control with a role, a name and its place. A screen reader gets this
//! list through UI Automation (`crate::a11y`), and the same names become tooltips on buttons that
//! show only an icon.

use proto::Row;
use windows::{
    Win32::Graphics::Direct2D::{Common::D2D_RECT_F, D2D1_ROUNDED_RECT},
    core::Result,
};

use super::{
    Island, Panel, Part,
    kit::{Font, Frame, HotKind, Rect, Style, draw_text, set_opacity, tween},
};
use crate::media::Cmd;

/// How long the cursor rests on a button before its tooltip shows.
const TIP_DELAY: f32 = 0.6;
const TIP_FADE: f32 = 0.12;
const TIP_PAD_X: f32 = 8.0;
const TIP_PAD_Y: f32 = 4.0;
const TIP_GAP: f32 = 6.0;
const TIP_RADIUS: f32 = 6.0;
/// Long texts wrap at this width.
const TIP_MAX_W: f32 = 360.0;
/// Keeps the tooltip off the window edge.
const TIP_MARGIN: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Button,
    Switch,
    TextInput,
    SearchInput,
    ListBox,
    Option,
    Group,
    Label,
    /// Read out on change: the newest notification.
    Status,
}

/// One control. Ids are stable while the control exists, so a screen reader keeps its place
/// when the island redraws.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub id: u64,
    /// 0 is the island itself.
    pub parent: u64,
    pub role: Role,
    pub name: String,
    pub description: String,
    /// The text of an input field.
    pub value: Option<String>,
    /// In window DIPs; `None` for a row scrolled out of view.
    pub rect: Option<Rect>,
    pub selected: Option<bool>,
    pub toggled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub elements: Vec<Element>,
    /// The control with keyboard focus while the island has it: the search line, a result or a row.
    pub focus: Option<u64>,
}

/// Tooltip state: which control it is for and how far it has faded in.
#[derive(Default)]
pub struct Tip {
    target: Option<(String, Rect)>,
    since: f32,
    alpha: f32,
    /// Hidden after a click until the cursor moves to another control.
    dismissed: Option<Rect>,
    /// Where it was drawn: the window region must include it.
    pub drawn: Option<Rect>,
}

const PANEL: u64 = 10;
const INPUT: u64 = 11;
const LIST: u64 = 12;
const STATUS: u64 = 9;

fn part_id(p: Part) -> u64 {
    match p {
        Part::Time => 1,
        Part::Weather => 2,
        Part::Player => 3,
        Part::Search => 4,
        Part::More => 5,
        Part::Note(n) => 100 + n as u64,
    }
}

fn hot_id(k: HotKind) -> u64 {
    match k {
        HotKind::Launch(i) => 1000 + i as u64,
        HotKind::LaunchAct(j) => 1100 + j as u64,
        HotKind::MenuRow(i) => 1200 + i as u64,
        HotKind::MenuBack => 1300,
        HotKind::Swatch(i) => 1400 + i as u64,
        HotKind::Media(Cmd::Prev) => 1500,
        HotKind::Media(Cmd::TogglePlay) => 1501,
        HotKind::Media(Cmd::Next) => 1502,
        HotKind::Media(Cmd::Seek(_)) | HotKind::Seek { .. } => 1503,
        HotKind::Dismiss => 1600,
        HotKind::OpenApp => 1601,
        HotKind::Note(p) => 1700 + part_id(p),
    }
}

fn element(id: u64, parent: u64, role: Role, name: impl Into<String>, rect: Option<Rect>) -> Element {
    Element { id, parent, role, name: name.into(), description: String::new(), value: None, rect, selected: None, toggled: None }
}

impl Island {
    fn hot_rect(&self, k: HotKind) -> Option<Rect> {
        self.hotspots.iter().find(|h| hot_id(h.kind) == hot_id(k)).map(|h| h.rect)
    }

    /// The name of a button that shows only an icon; `None` where the control speaks for itself.
    fn icon_name(&self, k: HotKind) -> Option<String> {
        let n = &crate::i18n::t().names;
        Some(
            match k {
                HotKind::Media(Cmd::Prev) => n.prev,
                HotKind::Media(Cmd::Next) => n.next,
                HotKind::Media(Cmd::TogglePlay) if self.playing => n.pause,
                HotKind::Media(Cmd::TogglePlay) => n.play,
                HotKind::Dismiss => n.close,
                HotKind::MenuBack => n.back,
                HotKind::Swatch(_) => n.copy_color,
                _ => return None,
            }
            .to_string(),
        )
    }

    /// Everything a screen reader can reach right now.
    pub fn access(&self) -> Snapshot {
        let tr = crate::i18n::t();
        let n = &tr.names;
        let mut out = Vec::new();
        let (bar_t, bar_b) = self.bar();
        for (s, &(p, x, w)) in self.segments.iter().zip(&self.parts) {
            out.push(element(part_id(p), 0, Role::Button, s.label(), Some((x, bar_t, x + w, bar_b))));
        }
        let mut search = element(part_id(Part::Search), 0, Role::Button, n.search, Some(self.search));
        search.description = "Alt+Space".into();
        out.push(search);
        let mut newest = None;
        for &(p, r) in self.zone.parts() {
            let mut e = element(part_id(p), 0, Role::Button, n.more, Some(r));
            if let Some(note) = self.zone.note(p) {
                e.name = note.title.clone();
                e.description = note.body.clone();
                newest.get_or_insert_with(|| if note.body.is_empty() { note.title.clone() } else { format!("{}. {}", note.title, note.body) });
            }
            out.push(e);
        }
        // Not shown: a screen reader announces it when it changes.
        out.push(element(STATUS, 0, Role::Status, newest.unwrap_or_default(), None));

        let mut focus = None;
        if let Some(panel) = self.open {
            let title = match panel {
                Panel::Launcher => n.search.to_string(),
                Panel::Menu => self.menu.title().map_or_else(|| self.menu.module.clone(), String::from),
                Panel::Note | Panel::Stack => n.notifications.to_string(),
                Panel::Player => self.segments.iter().find(|s| s.part() == Part::Player).map(|s| s.label()).unwrap_or_default(),
                Panel::Weather => self.segments.iter().find(|s| s.part() == Part::Weather).map(|s| s.label()).unwrap_or_default(),
            };
            out.push(element(PANEL, 0, Role::Group, title, None));
            match panel {
                Panel::Launcher => focus = self.access_launcher(&mut out),
                Panel::Menu => focus = self.access_menu(&mut out),
                _ => {
                    // Other dropdowns have only buttons: player controls, close, swatches, opening the app.
                    for h in &self.hotspots {
                        let name = match h.kind {
                            HotKind::Seek { .. } => n.seek.to_string(),
                            HotKind::OpenApp => n.open_app.to_string(),
                            HotKind::Note(p) => self.zone.note(p).map_or_else(|| n.more.to_string(), |note| note.title.clone()),
                            k => match self.icon_name(k) {
                                Some(name) => name,
                                None => continue,
                            },
                        };
                        out.push(element(hot_id(h.kind), PANEL, Role::Button, name, Some(h.rect)));
                    }
                }
            }
        }
        Snapshot { elements: out, focus }
    }

    fn access_launcher(&self, out: &mut Vec<Element>) -> Option<u64> {
        let tr = crate::i18n::t();
        let m = &self.launcher.model;
        let mut input = element(INPUT, PANEL, Role::SearchInput, tr.names.search, None);
        input.value = Some(m.line.text.clone());
        out.push(input);
        out.push(element(LIST, PANEL, Role::ListBox, tr.names.results, None));
        for (i, item) in m.items.iter().enumerate() {
            let kind = HotKind::Launch(i);
            let name = item.prefix.as_ref().map_or_else(|| item.name.clone(), |p| format!("{p} › {}", item.name));
            let mut e = element(hot_id(kind), LIST, Role::Option, name, self.hot_rect(kind));
            e.description = item.keys.iter().chain([&item.hint]).filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(", ");
            e.selected = Some(i == m.sel && m.menu.is_none());
            out.push(e);
        }
        if let Some(menu) = &m.menu {
            for (j, act) in menu.acts.iter().enumerate() {
                let kind = HotKind::LaunchAct(j);
                let mut e = element(hot_id(kind), LIST, Role::Option, tr.launcher_acts[*act as usize], self.hot_rect(kind));
                e.selected = Some(j == menu.sel);
                out.push(e);
            }
            return Some(hot_id(HotKind::LaunchAct(menu.sel)));
        }
        Some(if m.items.is_empty() { INPUT } else { hot_id(HotKind::Launch(m.sel)) })
    }

    fn access_menu(&self, out: &mut Vec<Element>) -> Option<u64> {
        if let Some(r) = self.hot_rect(HotKind::MenuBack) {
            out.push(element(hot_id(HotKind::MenuBack), PANEL, Role::Button, crate::i18n::t().names.back, Some(r)));
        }
        let (screen, sel) = self.menu.current()?;
        let editing = self.menu.editing_text();
        for (i, row) in screen.rows.iter().enumerate() {
            let kind = HotKind::MenuRow(i);
            let rect = self.hot_rect(kind);
            let e = match row {
                Row::Item { text, hint, keys, confirm, .. } => {
                    let mut e = element(hot_id(kind), PANEL, Role::Button, text.clone(), rect);
                    e.description =
                        confirm.clone().unwrap_or_else(|| [keys, hint].into_iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(", "));
                    e
                }
                Row::Toggle { text, on, .. } => {
                    let mut e = element(hot_id(kind), PANEL, Role::Switch, text.clone(), rect);
                    e.toggled = Some(*on);
                    e
                }
                Row::Input { text, placeholder, .. } => {
                    let mut e = element(hot_id(kind), PANEL, Role::TextInput, placeholder.clone(), rect);
                    e.value = Some(editing.filter(|(r, _)| *r == i).map_or_else(|| text.clone(), |(_, t)| t.to_string()));
                    e
                }
                Row::Button { text, .. } => element(hot_id(kind), PANEL, Role::Button, text.clone(), rect),
                Row::Text { text } => element(hot_id(kind), PANEL, Role::Label, text.replace('\n', " "), None),
                Row::Section { title } if !title.is_empty() => element(hot_id(kind), PANEL, Role::Label, title.clone(), None),
                Row::Header { title, subtitle } => element(hot_id(kind), PANEL, Role::Label, format!("{title}, {subtitle}"), None),
                Row::Pair { label, value } => element(hot_id(kind), PANEL, Role::Label, format!("{label}: {value}"), None),
                Row::Section { .. } | Row::Divider | Row::Art { .. } => continue,
            };
            out.push(e);
        }
        Some(sel.map_or(PANEL, |i| hot_id(HotKind::MenuRow(i))))
    }

    /// The control under the cursor that has a tooltip: an icon button or the search block.
    fn tip_target(&self) -> Option<(String, Rect)> {
        let (x, y) = self.mouse?;
        // A cut text reads whole: it comes before the row's own button.
        if let Some((r, text)) = self.cut.iter().rev().find(|(r, _)| super::kit::inside(*r, x, y)) {
            return Some((text.clone(), *r));
        }
        if let Some(h) = self.hotspot_at(x, y) {
            return self.icon_name(h.kind).map(|name| (name, h.rect));
        }
        let n = &crate::i18n::t().names;
        super::kit::inside(self.search, x, y).then(|| (format!("{} · Alt+Space", n.search), self.search))
    }

    /// Advances the tooltip; `true` while it is waiting or fading.
    pub(super) fn step_tip(&mut self, dt: f32) -> bool {
        let target = self.tip_target();
        if target.as_ref().map(|t| t.1) != self.tip.target.as_ref().map(|t| t.1) || target.as_ref().map(|t| &t.0) != self.tip.target.as_ref().map(|t| &t.0) {
            if target.as_ref().map(|t| t.1) != self.tip.dismissed {
                self.tip.dismissed = None;
            }
            self.tip.target = target;
            self.tip.since = self.t;
            self.tip.alpha = 0.0;
        }
        let show = self.tip.target.is_some() && self.tip.dismissed.is_none() && self.t - self.tip.since >= TIP_DELAY;
        let fading = tween(&mut self.tip.alpha, if show { 1.0 } else { 0.0 }, dt / TIP_FADE);
        let waiting = self.tip.target.is_some() && self.tip.dismissed.is_none() && !show;
        fading || waiting
    }

    /// A click hides the tooltip until the cursor goes to another control.
    pub fn hide_tip(&mut self) {
        self.tip.dismissed = self.tip.target.as_ref().map(|t| t.1);
        self.tip.alpha = 0.0;
    }
}

/// Draws the tooltip next to its control, toward the screen: above for a bottom taskbar.
pub(super) fn draw_tip(tip: &mut Tip, f: &mut Frame, width: f32, win_h: f32, bottom: bool) -> Result<()> {
    tip.drawn = None;
    let Some((name, (l, t, r, b))) = &tip.target else { return Ok(()) };
    if tip.alpha <= 0.0 {
        return Ok(());
    }
    let text = f.txt(name, Font::Small, Style::wrap(TIP_MAX_W - TIP_PAD_X * 2.0))?;
    let (w, h) = (text.w + TIP_PAD_X * 2.0, text.h + TIP_PAD_Y * 2.0);
    let x = ((l + r) / 2.0 - w / 2.0).clamp(TIP_MARGIN, (width - w - TIP_MARGIN).max(TIP_MARGIN));
    let above = t - TIP_GAP - h;
    let below = b + TIP_GAP;
    let y = if bottom && above >= TIP_MARGIN || !bottom && below + h > win_h - TIP_MARGIN { above } else { below };
    let rect = (x, y, x + w, y + h);
    let (body, fg) = (&f.br.body, &f.br.fg);
    set_opacity(body, tip.alpha * 0.96);
    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
    // outlives the call.
    unsafe {
        f.dc.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: rect.0, top: rect.1, right: rect.2, bottom: rect.3 }, radiusX: TIP_RADIUS, radiusY: TIP_RADIUS },
            body,
        );
    }
    set_opacity(body, 1.0);
    set_opacity(fg, tip.alpha);
    draw_text(f.dc, &text, x + TIP_PAD_X, y + TIP_PAD_Y, fg);
    set_opacity(fg, 1.0);
    tip.drawn = Some(rect);
    Ok(())
}
