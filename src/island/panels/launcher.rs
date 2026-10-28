// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launcher panel: search input and results list.

use std::collections::HashMap;

use windows::{
    Win32::Graphics::{
        Direct2D::{Common::D2D_RECT_F, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, ID2D1Bitmap1},
        Gdi::DeleteObject,
    },
    core::Result,
};

use super::{Area, PanelView, Reveal};
use crate::{
    icons::{self, Icon},
    island::{
        field::Field,
        kit::{Font, Frame, Glide, HotKind, Hotspot, Style, draw_text, ease_out_cubic, inside, set_opacity},
        segments::Data,
    },
    launcher::{Act, Kind, Launcher, MAX_ITEMS, apps, icons as shell_icons},
};

const W: f32 = 480.0;
const INPUT_H: f32 = 44.0;
const PAD_X: f32 = 20.0;
/// Search icon size in input and text offset.
const INPUT_ICON: f32 = 16.0;
const TEXT_X: f32 = PAD_X + INPUT_ICON + 12.0;
const CARET_H: f32 = 18.0;
const LIST_PAD: f32 = 6.0;
/// More air below the last row than above the first, so the list does not butt against the edge.
const LIST_PAD_BOTTOM: f32 = 12.0;
const ROW_H: f32 = 34.0;
const ROW_INSET: f32 = 8.0;
const ROW_ICON: f32 = 20.0;
/// Delay before starting marquee on the selected long name, in seconds.
const NAME_MARQUEE_DELAY: f32 = 0.8;
/// Where the hotkey column starts, as a share of the row width: the same spot in every row, so
/// the keys line up as in a menu.
const KEYS_AT: f32 = 0.56;
/// Action menu row height, aligned with the name column.
const ACT_H: f32 = 30.0;
const ACT_PAD: f32 = 10.0;
const ACT_LINE_W: f32 = 1.0;
/// Max list height; scrolling occurs if expanded menu exceeds this.
const LIST_MAX: f32 = ROW_H * MAX_ITEMS as f32;
const MENU_DUR: f32 = 0.18;
const SCROLL_TAU: f32 = 0.06;

/// On-screen menu state. Persists during collapse animation.
struct Shown {
    row: usize,
    /// Target row owner. If results change, this row may no longer exist.
    owner: String,
    acts: Vec<Act>,
}

/// List row: either a result item or an expanded menu action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Item(usize),
    Act(usize),
}

#[derive(Default)]
pub struct LauncherPanel {
    pub model: Launcher,
    /// App icons by ID. `None` means no icon exists (prevents repeated queries).
    icons: HashMap<String, Option<ID2D1Bitmap1>>,
    /// List scroll position in DIPs: current and target.
    scroll: f32,
    scroll_to: f32,
    shown: Option<Shown>,
    /// Menu expansion progress, 0..1.
    menu_p: f32,
    /// Time of the last frame for animation stepping.
    last_t: Option<f32>,
    glide: Glide,
    /// Caret position in window DIPs: `(x, top, bottom)` for IME window placement.
    pub caret: Option<(f32, f32, f32)>,
    /// The search line as a field: scrolling, selection, the mouse, blinking.
    pub field: Field,
    /// Selected row and the time it was selected (triggers marquee).
    lit_row: Option<Row>,
    lit_since: f32,
    /// Mouse position from the last frame. Selection follows mouse only on movement.
    last_mouse: Option<(f32, f32)>,
    /// Cached selection and menu state to prevent scroll jumping during wheel events.
    fitted: Option<(usize, Option<(usize, usize)>)>,
}

impl LauncherPanel {
    fn rows(&self) -> usize {
        self.model.items.len().max(1)
    }

    fn acts(&self) -> usize {
        self.model.menu.as_ref().map_or(0, |m| m.acts.len())
    }

    fn next_h(&self, row: usize) -> f32 {
        if row + 1 < self.model.items.len() { ROW_H } else { 0.0 }
    }

    fn list_h(&self) -> (f32, f32) {
        let content = ROW_H * self.rows() as f32 + ACT_H * self.acts() as f32;
        let limit = match &self.model.menu {
            Some(m) => LIST_MAX.max(ROW_H + ACT_H * m.acts.len() as f32 + self.next_h(m.row)),
            None => LIST_MAX,
        };
        (content, content.min(limit))
    }

    pub fn wheel(&mut self, notches: f32) {
        self.scroll_to -= notches * ROW_H;
    }

    /// Pre-warms app catalog and recent icons in the background before first open.
    pub fn prewarm(&mut self, dpi: f32) {
        apps::refresh(crate::media_changed);
        let px = (ROW_ICON * dpi / 96.0).round() as i32;
        for id in self.model.recent_apps() {
            shell_icons::request(&apps::shell_name(&id), px, crate::media_changed);
        }
    }

    pub fn moving(&self) -> bool {
        let target = if self.model.menu.is_some() { 1.0 } else { 0.0 };
        (self.scroll - self.scroll_to).abs() > 0.3 || self.menu_p != target || self.last_t.is_none() || self.glide.moving()
    }

    /// Fetches shell icon by name. Returns `None` while loading in the background.
    fn icon(&mut self, f: &Frame, key: &str) -> Option<ID2D1Bitmap1> {
        if let Some(b) = self.icons.get(key) {
            return b.clone();
        }
        match shell_icons::take(key) {
            Some(hbm) => {
                let bmp = hbm.and_then(|h| {
                    let b = f.gfx.bitmap_from_hbitmap(h).ok();
                    // SAFETY: `h` is an HBITMAP handed over by `icons::take`, deleted exactly once here.
                    unsafe {
                        let _ = DeleteObject(h.into());
                    }
                    b
                });
                self.icons.insert(key.to_string(), bmp.clone());
                bmp
            }
            None => {
                shell_icons::request(key, (ROW_ICON * f.gfx.dpi / 96.0).round() as i32, crate::media_changed);
                None
            }
        }
    }
}

impl PanelView for LauncherPanel {
    fn size(&self) -> (f32, f32) {
        (W, INPUT_H + LIST_PAD + LIST_PAD_BOTTOM + self.list_h().1)
    }

    fn draw(&mut self, f: &mut Frame, _data: &Data, area: Area, reveal: Reveal) -> Result<()> {
        let br = f.br;
        let (fg, dim, plate) = (br.fg.clone(), br.dim.clone(), br.select.clone());
        let tr = crate::i18n::t();
        let (l, r, top) = (area.left(), area.right(), area.top);

        // Input line: search icon, text/placeholder, caret.
        let (a0, r0) = reveal.row(0);
        let cy = top + INPUT_H / 2.0 + r0;
        set_opacity(&dim, a0);
        icons::draw(f.gfx, f.dc, Icon::Search, l + PAD_X + INPUT_ICON / 2.0, cy, INPUT_ICON, &dim)?;
        let tx = l + TEXT_X;
        let room = r - PAD_X - tx;
        if self.model.line.text.is_empty() {
            let hint = f.txt(tr.launcher_placeholder, Font::Input, Style::trim(room))?;
            set_opacity(&dim, a0);
            draw_text(f.dc, &hint, tx, cy - hint.h / 2.0, &dim);
        }
        let caret_x = self.field.draw(f, &mut self.model.line, Font::Input, tx, room, cy, CARET_H, true, a0)?;
        self.caret = Some((caret_x, cy - CARET_H / 2.0, cy + CARET_H / 2.0));
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            f.dc.FillRectangle(&D2D_RECT_F { left: l + ROW_INSET, top: top + INPUT_H, right: r - ROW_INSET, bottom: top + INPUT_H + 1.0 }, &f.shade(a0)?);
        }

        // Results list: empty state shows a hint.
        let list_top = top + INPUT_H + LIST_PAD;
        if self.model.items.is_empty() {
            let (a, rise) = reveal.row(1);
            let s = if self.model.line.text.trim().is_empty() { tr.launcher_empty } else { tr.launcher_nothing };
            let t = f.txt(s, Font::Small, Style::trim(W - PAD_X * 2.0))?;
            set_opacity(&dim, a);
            draw_text(f.dc, &t, l + PAD_X, list_top + ROW_H / 2.0 - t.h / 2.0 + rise, &dim);
        }
        // Animations: menu expansion and smooth scrolling.
        let dt = self.last_t.map_or(0.0, |t| (f.t - t).clamp(0.0, 0.1));
        self.last_t = Some(f.t);
        match &self.model.menu {
            Some(m) => self.shown = Some(Shown { row: m.row, owner: self.model.items[m.row].target.clone(), acts: m.acts.clone() }),
            None if self.shown.as_ref().is_some_and(|s| self.model.items.get(s.row).is_none_or(|i| i.target != s.owner)) => {
                self.shown = None;
                self.menu_p = 0.0;
            }
            None => {}
        }
        let step = dt / MENU_DUR;
        self.menu_p = if self.model.menu.is_some() { (self.menu_p + step).min(1.0) } else { (self.menu_p - step).max(0.0) };
        if self.menu_p == 0.0 {
            self.shown = None;
        }
        let open = ease_out_cubic(self.menu_p);
        let menu = self.shown.as_ref().map(|s| (s.row, s.acts.clone()));

        // Layout: items and menu actions (height scales with expansion).
        let act_h = ACT_H * open;
        let mut rows = Vec::new();
        let mut y = list_top;
        for i in 0..self.model.items.len() {
            rows.push((Row::Item(i), y));
            y += ROW_H;
            if let Some((row, acts)) = &menu
                && *row == i
            {
                for j in 0..acts.len() {
                    rows.push((Row::Act(j), y));
                    y += act_h;
                }
            }
        }
        let height = |r: &Row| if matches!(r, Row::Act(_)) { act_h } else { ROW_H };

        // Scroll snapping: ensures selected item (and its menu) is fully visible.
        let (content_h, view_h) = self.list_h();
        let key = (self.model.sel, self.model.menu.as_ref().map(|m| (m.row, m.sel)));
        if self.fitted != Some(key) {
            self.fitted = Some(key);
            let (from, to) = match &self.model.menu {
                Some(m) => {
                    let from = m.row as f32 * ROW_H;
                    (from, from + ROW_H + ACT_H * m.acts.len() as f32 + self.next_h(m.row))
                }
                None => {
                    let from = self.model.sel as f32 * ROW_H;
                    (from, from + ROW_H)
                }
            };
            if from < self.scroll_to {
                self.scroll_to = from;
            } else if to > self.scroll_to + view_h {
                self.scroll_to = to - view_h;
            }
        }
        self.scroll_to = self.scroll_to.clamp(0.0, (content_h - view_h).max(0.0));
        self.scroll += (self.scroll_to - self.scroll) * (1.0 - (-dt / SCROLL_TAU).exp());
        if (self.scroll - self.scroll_to).abs() <= 0.3 {
            self.scroll = self.scroll_to;
        }

        // Visible rows: filtered by current panel depth and scroll position.
        let (view_t, view_b) = (list_top, area.top + area.d - LIST_PAD_BOTTOM);
        let rows: Vec<(Row, f32)> =
            rows.into_iter().map(|(row, t)| (row, t - self.scroll)).filter(|(row, t)| height(row) >= 1.0 && t + height(row) > view_t && *t < view_b).collect();
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe {
            f.dc.PushAxisAlignedClip(
                &D2D_RECT_F { left: l, top: view_t, right: r, bottom: view_b },
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED,
            );
        }
        let in_view = |(_, my): (f32, f32)| my >= view_t && my < view_b;
        let hovered = rows
            .iter()
            .find(|(row, rt)| f.mouse.filter(|m| in_view(*m)).is_some_and(|(mx, my)| inside((l + ROW_INSET, *rt, r - ROW_INSET, rt + height(row)), mx, my)))
            .map(|(row, _)| *row);
        // Mouse moved over item: transfer selection and keep it.
        let moved = f.mouse != self.last_mouse;
        self.last_mouse = f.mouse;
        if moved {
            match (hovered, self.model.menu.as_mut()) {
                (Some(Row::Act(j)), Some(m)) => m.sel = j,
                (Some(Row::Item(i)), None) => self.model.sel = i,
                _ => {}
            }
        }
        let lit = match &self.model.menu {
            Some(m) => Row::Act(m.sel),
            None => Row::Item(self.model.sel),
        };
        if self.lit_row != Some(lit) {
            self.lit_row = Some(lit);
            self.lit_since = f.t;
        }
        let lit_since = self.lit_since;
        let items = self.model.items.clone();
        let ix = l + PAD_X + ROW_ICON / 2.0;
        let nx = ix + ROW_ICON / 2.0 + 12.0;
        // Highlight plate: slides to the selected row.
        let lit_at = rows.iter().enumerate().find(|(_, (row, _))| *row == lit);
        let target = lit_at.map(|(k, (row, top_y))| {
            let rt = top_y + reveal.row(k + 1).1;
            let left = if matches!(row, Row::Act(_)) { nx - ACT_PAD } else { l + ROW_INSET };
            (left, rt, r - ROW_INSET, rt + height(row))
        });
        self.glide.step(target, f.t);
        if let Some((k, (row, _))) = lit_at {
            let a = reveal.row(k + 1).0 * if matches!(row, Row::Act(_)) { open } else { 1.0 };
            self.glide.draw(f.dc, &plate, a, 12.0);
        }
        for (k, (row, top_y)) in rows.iter().enumerate() {
            let (a, rise) = reveal.row(k + 1);
            let a = if matches!(row, Row::Act(_)) { a * open } else { a };
            let rt = top_y + rise;
            let h = height(row);
            let left = if matches!(row, Row::Act(_)) { nx - ACT_PAD } else { l + ROW_INSET };
            let rect = (left, rt, r - ROW_INSET, rt + h);
            let rcy = rt + h / 2.0;
            if a > 0.5 {
                // Partial visibility: only register hotspots for visible parts.
                let rect = (rect.0, rect.1.max(view_t), rect.2, rect.3.min(view_b));
                let kind = match row {
                    Row::Item(i) => HotKind::Launch(*i),
                    Row::Act(j) => HotKind::LaunchAct(*j),
                };
                f.hotspots.push(Hotspot { rect, kind });
            }
            let item = match row {
                Row::Act(j) => {
                    let act = menu.as_ref().unwrap().1[*j];
                    // Vertical line connecting actions to the parent row.
                    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a
                    // borrow that outlives the call.
                    unsafe {
                        f.dc.FillRectangle(&D2D_RECT_F { left: ix - ACT_LINE_W / 2.0, top: rt, right: ix + ACT_LINE_W / 2.0, bottom: rt + h }, &f.shade(a)?);
                    }
                    let t = f.txt(tr.launcher_acts[act as usize], Font::Text, Style::trim(r - PAD_X - nx))?;
                    let brush = if lit == *row { &fg } else { &dim };
                    set_opacity(brush, a);
                    draw_text(f.dc, &t, nx, rcy - t.h / 2.0, brush);
                    continue;
                }
                Row::Item(i) => &items[*i],
            };
            // Icon resolution: shell icon for apps/files, built-in for utilities.
            let shell = match item.kind {
                Kind::App => Some(apps::shell_name(&item.target)),
                Kind::File | Kind::Folder => Some(item.target.clone()),
                Kind::Calc | Kind::Money | Kind::Windows | Kind::Module => None,
            };
            if let Some(d) = &item.glyph {
                set_opacity(&fg, a);
                icons::draw_path(f.gfx, f.dc, d, ix, rcy, 18.0, &fg)?;
            } else if let Some(key) = shell {
                if let Some(bmp) = self.icon(f, &key) {
                    let dst = D2D_RECT_F { left: ix - ROW_ICON / 2.0, top: rcy - ROW_ICON / 2.0, right: ix + ROW_ICON / 2.0, bottom: rcy + ROW_ICON / 2.0 };
                    // SAFETY: Direct2D calls on live COM objects; every argument is a value or a
                    // borrow that outlives the call.
                    unsafe { f.dc.DrawBitmap(&bmp, Some(&dst), a, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None) };
                }
            } else {
                set_opacity(&fg, a);
                let icon = if item.kind == Kind::Windows { Icon::Search } else { Icon::Equals };
                icons::draw(f.gfx, f.dc, icon, ix, rcy, 16.0, &fg)?;
            }
            // A hotkey splits the row into three columns: name, keys, hint. Without one, the hint takes
            // at most 40% of the row.
            let kx = l + (r - l) * KEYS_AT;
            let keys = item.keys.as_deref().map(|k| f.txt(k, Font::Tiny, Style::Plain)).transpose()?;
            let hint_max = keys.as_ref().map_or((r - nx) * 0.4, |k| r - PAD_X - kx - k.w - 16.0);
            let hint = f.txt(&item.hint, Font::Small, Style::trim(hint_max.max(0.0)))?;
            let room = if keys.is_some() { kx - 12.0 - nx } else { r - PAD_X - hint.w - 16.0 - nx };
            // A module function: the module name muted in front, as a path to the same item in its menu.
            let prefix = item.prefix.as_ref().map(|p| f.txt(&format!("{p} › "), Font::Text, Style::trim(room * 0.5))).transpose()?;
            let px = prefix.as_ref().map_or(0.0, |t| t.w);
            let (name_x, name_room) = (nx + px, room - px);
            let full = item.prefix.as_ref().map_or_else(|| item.name.clone(), |p| format!("{p} › {}", item.name));
            f.cut_if_long((nx, rt, nx + room, rt + h), &full, Font::Text, room)?;
            f.cut_if_long((r - PAD_X - hint.w, rt, r - PAD_X, rt + h), &item.hint, Font::Small, hint_max.max(0.0))?;
            if let Some(t) = &prefix {
                set_opacity(&dim, a);
                draw_text(f.dc, t, nx, rcy - t.h / 2.0, &dim);
            }
            set_opacity(&fg, a);
            // Long names: marquee for selected, ellipsis for others.
            if lit == *row {
                let name = f.txt(&item.name, Font::Text, Style::Plain)?;
                f.marquee(&name, name_x, name_x + name_room, rcy, &fg, a, lit_since, NAME_MARQUEE_DELAY)?;
            } else {
                let name = f.txt(&item.name, Font::Text, Style::trim(name_room))?;
                draw_text(f.dc, &name, name_x, rcy - name.h / 2.0, &fg);
            }
            set_opacity(&dim, a);
            if let Some(k) = &keys {
                draw_text(f.dc, k, kx, rcy - k.h / 2.0, &dim);
            }
            draw_text(f.dc, &hint, r - PAD_X - hint.w, rcy - hint.h / 2.0, &dim);
        }
        // SAFETY: Direct2D calls on live COM objects; every argument is a value or a borrow that
        // outlives the call.
        unsafe { f.dc.PopAxisAlignedClip() };
        set_opacity(&fg, 1.0);
        set_opacity(&dim, 1.0);
        Ok(())
    }
}
