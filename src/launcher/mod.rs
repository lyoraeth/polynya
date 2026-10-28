// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launcher: the input line and results: currencies, calculator, apps, files.
//!
//! Only the model lives here: text, caret, results and key handling. Drawing is in the island
//! panel, focus and launching in the main module.

pub mod apps;
mod calc;
pub mod files;
mod fuzzy;
pub mod icons;
pub mod money;
mod recent;

use windows::Win32::{
    Foundation::{HANDLE, HGLOBAL},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
    },
    UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_APPS, VK_DOWN, VK_ESCAPE, VK_F10, VK_LEFT, VK_RETURN, VK_UP},
};

use crate::edit::Edited;
use recent::Recent;

/// How long a calculation in the line waits for a return after the launcher closes.
const KEEP_CALC: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Whether the line is a calculation (including an unfinished one: "7 + 2 +"). It is worth keeping
/// when the launcher closes: adding numbers from different pages takes several opens in a row.
fn is_math(text: &str) -> bool {
    let t = text.trim();
    let done = t.trim_end_matches(|c: char| c.is_whitespace() || "+-*/×÷^%.,(".contains(c));
    !done.is_empty() && (calc::eval(done).is_some() || money::convert(t).is_some())
}

/// Result rows.
pub const MAX_ITEMS: usize = 6;
/// Files keep at least this many rows when there are many apps.
const MIN_FILES: usize = 2;
/// Files are searched from this query length: one letter matches half the disk.
const FILES_FROM: usize = 2;
/// Clipboard format for UTF-16 text.
const CF_UNICODETEXT: u32 = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Money,
    Calc,
    App,
    File,
    Folder,
    /// Search in Explorer, when Everything is not running.
    Windows,
    /// Module command.
    Module,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    pub name: String,
    /// Label on the right: the type or the file's folder.
    pub hint: String,
    /// What to run: an app id, a path, a search address, text to copy, or a module's
    /// `exe<TAB>command`.
    pub target: String,
    /// SVG path icon of module commands.
    pub glyph: Option<String>,
    /// Hotkey of a module command, shown in its own narrow column.
    pub keys: Option<String>,
    /// A module function's module name, drawn muted before the name: "Devtools › ".
    pub prefix: Option<String>,
}

/// Action from a row's menu (right click or the menu key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Open,
    Admin,
    Location,
    CopyPath,
    Forget,
    Uninstall,
}

/// Action menu opened under row `row`.
#[derive(Debug)]
pub struct Menu {
    pub row: usize,
    pub acts: Vec<Act>,
    pub sel: usize,
    /// File path of the row: a program's exe or a found file.
    path: Option<String>,
}

/// Files that make sense to run as admin.
fn runnable(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    [".exe", ".bat", ".cmd", ".msi"].iter().any(|e| p.ends_with(e))
}

/// What to do in response to a key.
#[derive(Debug, PartialEq)]
pub enum Effect {
    None,
    /// Text or selection changed: redraw.
    Redraw,
    /// Close; `restore` returns focus to the window the launcher was opened from.
    Close {
        restore: bool,
    },
    Launch(String),
    /// Run a module command: `exe<TAB>command`.
    Invoke(String),
    /// Run a file as admin.
    Admin(String),
    /// Show a file in Explorer.
    Reveal(String),
    /// Open a path or address.
    Open(String),
    /// Copy to the clipboard.
    Copy(String),
}

#[derive(Default)]
pub struct Launcher {
    /// The search line.
    pub line: crate::edit::LineEdit,
    pub items: Vec<Item>,
    pub sel: usize,
    /// The row's open action menu.
    pub menu: Option<Menu>,
    recent: Option<Recent>,
    /// The last input; how long a calculation is kept counts from it.
    last_input: Option<std::time::Instant>,
    /// Commands of enabled modules, reread on every open.
    commands: Vec<(String, crate::modules::Command)>,
    /// Generations of the app catalog, the Everything reply and the rates as of the results.
    generations: (u64, u64, u64),
}

impl Launcher {
    fn recent(&mut self) -> &mut Recent {
        self.recent.get_or_insert_with(Recent::load)
    }

    /// Recent apps: what search shows on an empty query.
    pub fn recent_apps(&mut self) -> Vec<String> {
        self.recent().latest().into_iter().filter(|id| !id.starts_with(CMD)).take(MAX_ITEMS).map(String::from).collect()
    }

    /// A clean line on every open.
    pub fn reset(&mut self) {
        self.commands = crate::modules::commands(&crate::modules::off(&crate::config::load()));
        // Our settings are a search command too; an empty exe means the core runs it itself.
        self.commands.push((String::new(), crate::settings::command()));
        // A recent calculation stays, with the caret at the end to type "+ 3" right away.
        let keep = self.last_input.is_some_and(|t| t.elapsed() < KEEP_CALC) && is_math(&self.line.text);
        let text = if keep { self.line.text.clone() } else { String::new() };
        self.line.set(&text);
        self.refresh();
    }

    /// The app catalog or the Everything reply was updated: recompute the results.
    /// Returns `true` if there was anything.
    pub fn sync(&mut self) -> bool {
        let g = (apps::generation(), files::generation(), money::generation());
        if g == self.generations {
            return false;
        }
        self.generations = g;
        self.refresh();
        true
    }

    /// Recomputes the results; the selection sticks to the same item if it is still there.
    fn refresh(&mut self) {
        let selected = self.items.get(self.sel).map(|i| i.target.clone());
        let query = self.line.text.trim().to_string();
        let tr = crate::i18n::t();
        let mut items = Vec::new();

        if let Some(c) = money::convert(&query) {
            let hint = format!("{} {}", tr.launcher_rate, day_month(c.updated));
            items.push(Item {
                glyph: None,
                keys: None,
                prefix: None,
                kind: Kind::Money,
                name: format!("{} {}", money::format(c.value), c.to),
                hint,
                target: format!("{:.2}", c.value),
            });
        } else if let Some(v) = calc::eval(&query) {
            let text = v.text();
            items.push(Item { glyph: None, keys: None, prefix: None, kind: Kind::Calc, name: text.clone(), hint: tr.launcher_calc.into(), target: text });
        }

        // Apps and commands (picker, settings, module commands) are one result list, both in recents
        // and in match scoring with the frequency bonus.
        let catalog = apps::catalog();
        self.recent();
        let recent = self.recent.as_ref().unwrap();
        let found: Vec<Entry> = if query.is_empty() {
            recent
                .latest()
                .into_iter()
                .filter_map(|id| match id.strip_prefix(CMD) {
                    Some(_) => self.commands.iter().filter(|c| c.1.search).find(|c| cmd_key(&c.0, &c.1.id) == id).map(Entry::Cmd),
                    None => catalog.iter().find(|a| a.id == id).map(Entry::App),
                })
                .collect()
        } else {
            let cmds = self.commands.iter().filter(|c| c.1.search).filter_map(|c| {
                // A function is found by its own name and by "module name + its name".
                let full = c.1.prefix.as_ref().map(|p| format!("{p} {}", c.1.title).to_lowercase());
                let names = [Some(c.1.title.to_lowercase()), full].into_iter().flatten();
                let s = names.chain(c.1.keywords.iter().cloned()).filter_map(|k| fuzzy::score(&k, &query)).max()?;
                Some((s + recent.bonus(&cmd_key(&c.0, &c.1.id)), Entry::Cmd(c)))
            });
            let apps = catalog.iter().filter_map(|a| fuzzy::score(&a.name, &query).map(|s| (s + recent.bonus(&a.id), Entry::App(a))));
            let mut scored: Vec<(u32, Entry)> = cmds.chain(apps).collect();
            // On a tie a command ranks higher: typing "picker" you expect it, not a program.
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.is_cmd().cmp(&a.1.is_cmd())).then_with(|| a.1.name().cmp(b.1.name())));
            scored.into_iter().map(|(_, h)| h).collect()
        };

        let long = query.chars().count() >= FILES_FROM;
        let hits = if long {
            files::search(&query, crate::media_changed);
            files::latest()
        } else {
            Vec::new()
        };
        let everything = files::available();
        let tail = if long { if everything { hits.len().min(MIN_FILES) } else { 1 } } else { 0 };
        let app_room = MAX_ITEMS.saturating_sub(items.len() + tail);
        items.extend(found.into_iter().take(app_room).map(|h| match h {
            Entry::App(a) => {
                Item { glyph: None, keys: None, prefix: None, kind: Kind::App, name: a.name.clone(), hint: tr.launcher_app.into(), target: a.id.clone() }
            }
            Entry::Cmd((module, c)) => Item {
                kind: Kind::Module,
                prefix: c.prefix.clone(),
                name: c.title.clone(),
                hint: c.hint.clone(),
                target: format!("{module}\t{}", c.id),
                glyph: Some(c.icon.clone()),
                keys: crate::hotkeys::keys(module, &c.id),
            },
        }));

        if long && everything {
            let room = MAX_ITEMS.saturating_sub(items.len());
            items.extend(hits.into_iter().take(room).map(|h| Item {
                glyph: None,
                keys: None,
                prefix: None,
                kind: if h.is_folder { Kind::Folder } else { Kind::File },
                target: h.path(),
                name: h.name,
                hint: h.folder,
            }));
        } else if long {
            items.push(Item {
                glyph: None,
                keys: None,
                prefix: None,
                kind: Kind::Windows,
                name: format!("{} «{query}»", tr.launcher_windows),
                hint: "Windows".into(),
                target: format!("search-ms:query={}", percent(&query)),
            });
        }

        self.sel = selected.and_then(|t| items.iter().position(|i| i.target == t)).unwrap_or(0);
        self.items = items;
        // The results changed; the menu belonged to the previous row.
        self.menu = None;
    }

    /// A typed character (`WM_CHAR`). Control characters arrive here too; `key` handles them.
    pub fn char(&mut self, c: char) -> Effect {
        if self.line.char(c) == Edited::No {
            return Effect::None;
        }
        self.last_input = Some(std::time::Instant::now());
        self.refresh();
        Effect::Redraw
    }

    /// Runs the selected row.
    fn run(&mut self) -> Effect {
        let Some(item) = self.items.get(self.sel).cloned() else { return Effect::None };
        match item.kind {
            Kind::App => {
                self.recent().bump(&item.target);
                Effect::Launch(item.target)
            }
            Kind::File | Kind::Folder => {
                files::opened(&item.target);
                Effect::Open(item.target)
            }
            Kind::Windows => Effect::Open(item.target),
            Kind::Module => {
                self.recent().bump(&module_key(&item.target));
                Effect::Invoke(item.target)
            }
            Kind::Calc | Kind::Money => Effect::Copy(item.target),
        }
    }

    /// Opens the action menu under row `i`; utility rows have none.
    pub fn open_menu(&mut self, i: usize) -> Effect {
        let Some(item) = self.items.get(i).cloned() else { return Effect::None };
        // A command's whole menu is just "remove from recents".
        if item.kind == Kind::Module {
            let key = module_key(&item.target);
            if !self.recent().contains(&key) {
                return Effect::None;
            }
            self.sel = i;
            self.menu = Some(Menu { row: i, acts: vec![Act::Open, Act::Forget], sel: 0, path: None });
            return Effect::Redraw;
        }
        let path = match item.kind {
            Kind::App => apps::target_path(&item.target),
            Kind::File | Kind::Folder => Some(item.target.clone()),
            Kind::Calc | Kind::Money | Kind::Windows | Kind::Module => return Effect::None,
        };
        let mut acts = vec![Act::Open];
        if path.as_deref().is_some_and(runnable) {
            acts.push(Act::Admin);
        }
        if path.is_some() {
            acts.push(Act::Location);
        }
        match item.kind {
            Kind::App => {
                if self.recent().contains(&item.target) {
                    acts.push(Act::Forget);
                }
                acts.push(Act::Uninstall);
            }
            _ => acts.push(Act::CopyPath),
        }
        self.sel = i;
        self.menu = Some(Menu { row: i, acts, sel: 0, path });
        Effect::Redraw
    }

    /// Runs action `j` of the open menu.
    pub fn act(&mut self, j: usize) -> Effect {
        let Some(menu) = self.menu.take() else { return Effect::None };
        let (Some(&act), Some(item)) = (menu.acts.get(j), self.items.get(menu.row).cloned()) else { return Effect::None };
        let path = menu.path.unwrap_or_default();
        match act {
            Act::Open => {
                self.sel = menu.row;
                self.run()
            }
            Act::Admin => {
                if item.kind == Kind::App {
                    self.recent().bump(&item.target);
                }
                Effect::Admin(path)
            }
            Act::Location => Effect::Reveal(path),
            Act::CopyPath => Effect::Copy(path),
            Act::Forget => {
                let key = if item.kind == Kind::Module { module_key(&item.target) } else { item.target.clone() };
                self.recent().forget(&key);
                self.refresh();
                Effect::Redraw
            }
            // The system offers no way to open a specific program in "Installed apps".
            Act::Uninstall => Effect::Open("ms-settings:appsfeatures".into()),
        }
    }

    /// Keys of the open menu; `None` means the key is not its own: the menu is closed, handling goes on.
    fn menu_key(&mut self, vk: VIRTUAL_KEY, shift: bool) -> Option<Effect> {
        let m = self.menu.as_mut()?;
        let n = m.acts.len();
        match vk {
            VK_UP => m.sel = (m.sel + n - 1) % n,
            VK_DOWN => m.sel = (m.sel + 1) % n,
            VK_RETURN => {
                let j = m.sel;
                return Some(self.act(j));
            }
            VK_ESCAPE | VK_LEFT | VK_APPS => self.menu = None,
            VK_F10 if shift => self.menu = None,
            _ => {
                self.menu = None;
                return None;
            }
        }
        Some(Effect::Redraw)
    }

    /// A pressed key (`WM_KEYDOWN`).
    pub fn key(&mut self, vk: VIRTUAL_KEY, ctrl: bool, shift: bool) -> Effect {
        self.last_input = Some(std::time::Instant::now());
        if let Some(e) = self.menu_key(vk, shift) {
            return e;
        }
        match vk {
            VK_APPS => return self.open_menu(self.sel),
            VK_F10 if shift => return self.open_menu(self.sel),
            VK_ESCAPE => return Effect::Close { restore: true },
            VK_RETURN => return self.run(),
            VK_UP if !self.items.is_empty() => self.sel = (self.sel + self.items.len() - 1) % self.items.len(),
            VK_DOWN if !self.items.is_empty() => self.sel = (self.sel + 1) % self.items.len(),
            _ => match self.line.key(vk, ctrl, shift) {
                Edited::No => return Effect::None,
                Edited::Moved => {}
                Edited::Changed => self.refresh(),
            },
        }
        Effect::Redraw
    }

    /// Selects row `i` and runs it: a mouse click.
    pub fn activate(&mut self, i: usize) -> Effect {
        if i >= self.items.len() {
            return Effect::None;
        }
        self.sel = i;
        self.run()
    }
}

/// Commands in recents carry a prefix so they do not mix with app ids.
const CMD: &str = "cmd:";

/// Every module has `main`: the key carries the module id too. Our settings have none.
fn cmd_key(module: &str, id: &str) -> String {
    if module.is_empty() { format!("{CMD}{id}") } else { format!("{CMD}{module}/{id}") }
}

/// Recents key for a command row: its target is `exe<TAB>id`.
fn module_key(target: &str) -> String {
    let (module, id) = target.split_once('\t').unwrap_or(("", target));
    cmd_key(module, id)
}

/// A result row before formatting: an app or a command.
enum Entry<'a> {
    App(&'a apps::App),
    Cmd(&'a (String, crate::modules::Command)),
}

impl Entry<'_> {
    fn is_cmd(&self) -> bool {
        matches!(self, Entry::Cmd(_))
    }

    fn name(&self) -> &str {
        match self {
            Entry::App(a) => &a.name,
            Entry::Cmd(c) => &c.1.title,
        }
    }
}

/// "19.10": the rate date in local time.
fn day_month(unix: u64) -> String {
    use windows::Win32::{
        Foundation::FILETIME,
        System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
    };
    // FILETIME counts hundreds of nanoseconds since 1601.
    let ft = (unix + 11_644_473_600) * 10_000_000;
    let ft = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut utc, mut local) = Default::default();
    // SAFETY: `ft`, `utc` and `local` are valid for the calls; a null time zone means the current one.
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return String::new();
        }
    }
    format!("{:02}.{:02}", local.wDay, local.wMonth)
}

/// Percent-encoding for a search address: everything except letters and digits.
fn percent(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The clipboard text, if there is any.
pub fn clipboard_text() -> Option<String> {
    // SAFETY: the clipboard is opened and closed on this thread; the locked CF_UNICODETEXT data is
    // null-terminated and read only between `GlobalLock` and `GlobalUnlock`.
    unsafe {
        OpenClipboard(None).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
            let mem = HGLOBAL(h.0);
            let p = GlobalLock(mem) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut len = 0;
            while *p.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(mem);
            Some(s)
        });
        let _ = CloseClipboard();
        text
    }
}

/// Puts text on the clipboard.
pub fn copy(text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: the clipboard is opened and closed on this thread; `p` points to `wide.len()` u16s just
    // allocated and locked, and ownership passes to the system only through `SetClipboardData`.
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let p = GlobalLock(mem) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                let _ = GlobalUnlock(mem);
                // After a successful hand-over the memory belongs to the system.
                let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_BACK, VK_DELETE, VK_HOME, VK_LEFT, VK_Z};

    #[test]
    fn command_recent_key() {
        assert_eq!(module_key("picker\tmain"), "cmd:picker/main");
        // Our settings: an empty exe.
        assert_eq!(module_key("\tsettings"), "cmd:settings");
    }

    fn typed(s: &str) -> Launcher {
        let mut l = Launcher { recent: Some(Recent::default()), ..Default::default() };
        for c in s.chars() {
            l.char(c);
        }
        l
    }

    #[test]
    fn calc_survives_reopen() {
        let mut l = typed("7 + 2 +");
        l.reset();
        assert_eq!((l.line.text.as_str(), l.line.caret), ("7 + 2 +", 7));
        let mut l = typed("telegram");
        l.reset();
        assert!(l.line.text.is_empty());
    }

    #[test]
    fn editing() {
        let mut l = typed("привет мир");
        l.key(VK_BACK, true, false);
        assert_eq!(l.line.text, "привет ");
        l.key(VK_LEFT, false, false);
        l.key(VK_BACK, false, false);
        assert_eq!(l.line.text, "приве ");
        l.key(VK_HOME, false, false);
        l.key(VK_DELETE, false, false);
        assert_eq!(l.line.text, "риве ");
        assert_eq!(l.line.caret_utf16(), 0);
    }

    #[test]
    fn selection_and_undo() {
        let mut l = typed("hello world");
        l.key(VK_LEFT, true, true);
        assert_eq!(l.line.selection_utf16(), Some((6, 11)));
        l.char('x');
        assert_eq!(l.line.text, "hello x");
        l.key(VK_A, true, false);
        l.key(VK_BACK, false, false);
        assert_eq!(l.line.text, "");
        l.key(VK_Z, true, false);
        assert_eq!(l.line.text, "hello x");
        l.key(VK_Z, true, false);
        assert_eq!(l.line.text, "hello world");
    }

    #[test]
    fn control_chars_ignored() {
        let mut l = typed("a");
        assert_eq!(l.char('\u{8}'), Effect::None);
        assert_eq!(l.line.text, "a");
    }

    #[test]
    fn calc_row_first_and_copies() {
        let mut l = typed("2+2*2");
        assert_eq!(l.items[0].kind, Kind::Calc);
        assert_eq!(l.key(VK_RETURN, false, false), Effect::Copy("6".into()));
    }
}
